# Solution: Event-Driven Order Processing

## Architecture Overview

```
Client ──POST──▶ API (C#) ──publish──▶ RabbitMQ ──consume──▶ Processor (Rust) ──write──▶ PostgreSQL
   │                                  │    ▲                       │                  ▲
   │                                  │    │    retry exchange     │                  │
   │                                  │    └───────────────────────┘                  │
   │                                  │                                               │
   │                                  └──▶ Dead-Letter Queue                          │
   │                                                                                  │
   └──── GET /orders/{id} ───────────────────────────────────────── read ─────────────┘
```

Three services, two data stores, one message broker. The API and the Processor
never talk to each other directly — they only agree on a message contract.
This is the defining design decision.

The API has a second responsibility: it serves read-only status queries against
the same PostgreSQL database the processor writes to. This is a one-way read
path — the API never writes to the database.

---

## Service Breakdown

### 1. API — `OrderProcessing.Api` (C# / ASP.NET 8)

**Role**: Accept HTTP submissions (write path), serve order status queries (read path).
**Protocol**: Synchronous HTTP in, asynchronous AMQP out (writes), synchronous PostgreSQL queries (reads).
**Response**: Always `202 Accepted` for submissions (never `200 OK` — the order is not yet processed).

**Endpoints**:

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/orders` | Submit an order. Returns `202` with `correlation_id` + `order_id`. |
| `GET`  | `/api/orders/{id}` | Query order status by `correlation_id` or `order_id`. Returns `200` with full order detail, or `404`. |

**Status lifecycle visible via GET**:

```
accepted → processing → completed
                      → failed (after max retries → DLQ)
```

- `accepted` — the idempotency key has been claimed but the order is not yet persisted (or the order exists in `idempotency_keys` with status `processing`).
- `completed` — the order row exists with status `completed` and all fields populated.
- `failed` — the idempotency key is marked `failed`; `retry_count` and `last_error` are populated.

**Key choices**:

| Choice | Rationale |
|--------|-----------|
| `202 Accepted` not `200 OK` | The client must understand the order is *accepted*, not *complete*. This teaches the right mental model. |
| `X-Correlation-Id` header | Clients can supply their own idempotency key. If absent, the API generates one. Either way, the key flows through the entire system. |
| `mandatory: true` on publish | The broker must confirm a queue is bound to the routing key. If the topology is missing, the publisher gets an error at write time, not silently later. |
| Snake-case JSON serialization | Matches the Rust processor's `serde` conventions, avoiding mismatch errors. |
| **Read-only database access** | The API queries `orders`, `idempotency_keys`, and `processing_attempts` for status checks. It uses raw `Npgsql` (no ORM) to keep the dependency surface small. It never opens a write transaction. |
| Singleton `RabbitMqConnection` | One long-lived AMQP connection per process (not per request). Avoids the handshake tax on every publish. Fresh channels per publish are cheap and thread-safe. |
| Scoped `OrderReader` | A new `NpgsqlConnection` per request, matching ASP.NET's scoped lifetime. No connection pooling overhead because Npgsql pools internally. |
| `AcceptedAtAction` with route name | The `202` response includes a `Location` header pointing to the GET endpoint. The client can follow it without constructing the URL manually. |

**What the API does NOT do**:
- Does not write to the database. The read path is strictly `SELECT`-only.
- Does not validate business rules beyond shape (quantity > 0, non-empty strings).
- Does not retry — if the broker is down, the HTTP request fails with a 5xx and the caller retries.
- Does not know whether an order will ultimately succeed at submission time.

---

### 2. Processor — `order-processor` (Rust / Tokio)

**Role**: Consume messages, enforce idempotency, validate business rules, persist.
**Protocol**: AMQP consumer with manual acknowledgements.
**Concurrency**: One message per spawned Tokio task — bounded by the prefetch count.

**Key choices**:

| Choice | Rationale |
|--------|-----------|
| **Rust** | The processor owns database writes and retry state machines. Memory safety without a GC means predictable latency under load. `sqlx` gives compile-time-checked SQL queries. |
| **Manual acks** (`no_ack: false`) | The processor must not acknowledge a message until it's safely persisted. If the process crashes between consume and write, the broker redelivers. |
| **Idempotency via `INSERT … ON CONFLICT DO NOTHING`** | A single atomic operation. No `SELECT`-then-`INSERT` race. PostgreSQL's unique constraint on `correlation_id` is the lock. |
| **Three error categories** | `Transient` (DB deadlock, connection reset → retry), `Permanent` (empty customer name → DLQ), `AlreadyProcessed` (duplicate → ack and discard). The handler dispatches on the enum variant. |
| **Exponential backoff via broker TTL** | Retries use a dedicated retry exchange + queue. Messages are published with `expiration = base * 2^retry_count` ms. When TTL expires, RabbitMQ dead-letters them back to the main exchange. The processor does not sleep — it stays free for other work. |
| **Fire-and-forget failure marking** | If `insert_order` fails after `try_claim_idempotency` succeeded, a spawned task marks the key as `failed` (best-effort). This prevents the key from remaining stuck in `processing` forever. |
| **Structured JSON logging** (`tracing`) | Every log line includes `correlation_id`. Grep-friendly in production. |

**Retry topology**:

```
orders.exchange ──▶ orders.created.queue ──▶ Processor
       ▲                                          │
       │                          transient error │
       │                                          ▼
       │                              orders.retry (exchange)
       │                                    │
       │                          orders.created.retry (queue)
       │                          TTL: base * 2^retry ms
       │                                    │
       │                          TTL expires → dead-letter
       └────────────────────────────────────┘
       
After max_retries → nack → orders.dlx → orders.created.dlq
```

---

### 3. Database — PostgreSQL 16

**Schema**: Three tables supporting the processing pipeline.

| Table | Purpose |
|-------|---------|
| `orders` | Canonical order storage. `correlation_id` has a `UNIQUE` constraint. `payload` stores the full original message as JSONB for audit/replay. |
| `idempotency_keys` | The barrier. A row here means the `correlation_id` has been seen. Status tracks `processing → completed` or `processing → failed`. |
| `processing_attempts` | Append-only audit log. Every retry writes a row with the error message. Essential for debugging DLQ messages. |

**Key choices**:

| Choice | Rationale |
|--------|-----------|
| `correlation_id UNIQUE` on orders | Second line of defense. Even if the idempotency table check is somehow bypassed, the database rejects duplicate orders at the constraint level. |
| JSONB `payload` column | Full message fidelity for replay and debugging. No information loss. |
| `updated_at` trigger | Automatic timestamp maintenance, no application code required. |
| `ON DELETE SET NULL` on `idempotency_keys.order_id` | If an order is ever deleted, the idempotency record survives (with a null reference) — you still know the correlation was seen. |

---

### 4. Message Broker — RabbitMQ 3

**Topology declared at startup by both services** (idempotent declarations):

| Artifact | Type | Purpose |
|----------|------|---------|
| `orders.exchange` | Topic exchange | Entry point for all order events |
| `orders.created.queue` | Durable queue | Main work queue, bound to `order.created` |
| `orders.retry` | Topic exchange | Retry staging — messages expire here |
| `orders.created.retry` | Queue (with DLX → `orders.exchange`) | Holds messages until TTL fires |
| `orders.dlx` | Topic exchange | Final resting place for exhausted messages |
| `orders.created.dlq` | Durable queue | Dead-letter queue for manual inspection |

**Key choices**:

| Choice | Rationale |
|--------|-----------|
| **RabbitMQ over Kafka** | Kafka is a log, not a queue. This system needs per-message acknowledgements, selective retries, and TTL-based backoff — all native RabbitMQ primitives. Kafka would require external state management for the same guarantees. |
| **TTL-based retry, not scheduler-based** | The processor never blocks on a timer. It publishes to the retry exchange and acks the original message immediately. RabbitMQ handles the delay. Simpler code, fewer failure modes. |
| **Durable everything** | Queues and exchanges survive broker restarts. Messages are published with `delivery_mode=2` (persistent). This costs disk I/O but survives crashes. |
| **No quorum queues** | Classic mirrored queues are simpler and adequate for this scale. Quorum queues (Raft-based) add complexity and latency for a workload that doesn't need them. |

---

## The Idempotency Contract

This is the system's most important guarantee:

```
For a given correlation_id, exactly one of these outcomes occurs:
  1. The order is inserted into the `orders` table exactly once.
  2. The message is dead-lettered after exhausting retries.
  3. The message is dead-lettered immediately (permanent validation failure).

There is no fourth outcome. Duplicate submissions with the same
correlation_id are acknowledged and discarded — they consume no
database writes and no retry budget.
```

The implementation uses PostgreSQL's `INSERT … ON CONFLICT DO NOTHING` on the
`idempotency_keys` table, which is a single atomic operation. The `rows_affected()`
return value tells the processor whether it's the first claimant. This is the
same pattern used by Stripe's idempotency layer.

---

## Tradeoffs

### What we gain

| Property | How |
|----------|-----|
| **Write decoupling** | The API's p99 latency is the broker publish time (~1 ms local), not the business logic + DB write time (~10–50 ms). |
| **Load shedding** | Under a spike, the broker buffers messages. The processor consumes at its own pace. The API never queues in-process. |
| **Exactly-once processing** | Idempotency barrier + manual acks + persistent messages. At-least-once delivery from the broker, exactly-once effect in the database. |
| **Retry isolation** | A single poisoned message does not block the queue. It moves through the retry topology independently. |
| **Observability** | Every message has a `correlation_id` that flows from the HTTP header through the broker properties through the database rows. The `processing_attempts` table is a complete retry timeline. |
| **Independent deployability** | The API, processor, and database can be deployed, scaled, and restarted independently. The broker decouples their lifecycles. |
| **Technology fit** | C# for the API (good HTTP/JSON ergonomics, broad ecosystem). Rust for the processor (performance, safety, compile-time SQL checks). |
| **Client-visible status** | The GET endpoint gives the client a way to track order progress. The `Location` header on `202` responses makes this discoverable. The read path queries the same tables the processor writes — no separate read model needed. |

### What we lose

| Cost | Detail |
|------|--------|
| **Operational complexity** | Three services + two infrastructure components. Local development requires Docker Compose. Monitoring must span HTTP, AMQP, and PostgreSQL. |
| **Eventual consistency** | The API returns `202` before the order exists in the database. The client must poll or wait for a webhook/event to know the final status. This is correct for async processing but unfamiliar to developers used to CRUD APIs. |
| **No synchronous validation** | The API only validates shape. Business rule violations (empty product after trimming, etc.) are caught asynchronously. The client can't get a `400` for business logic errors — it gets a `202` followed by a `failed` status on the GET endpoint. |
| **RabbitMQ as SPOF** | If the broker is down, the entire write path stops. The API rejects requests. This is explicit — we chose not to fall back to direct DB writes because that would bypass the idempotency barrier. |
| **Retry amplification** | Each retry publishes a new message with a TTL. Under sustained failures, the number of in-flight messages grows. The broker's memory/disk usage must be monitored. |
| **Language split** | The message contract (JSON schema) is duplicated across C# and Rust. A change requires coordinated updates. There is no shared schema repository or code generation. |
| **DLQ requires manual intervention** | Messages that exhaust retries sit in the DLQ until a human inspects them. There is no automated replay mechanism (though the `payload` JSONB column in `orders` makes this possible to build). |
| **API now depends on PostgreSQL** | The GET endpoint adds a database dependency to the API service. The API must wait for PostgreSQL to be healthy before starting (`depends_on` in Docker Compose). A database outage now affects status queries in addition to the write path. |

---

## Why Not Alternatives

### Why not a monolith?

A monolith that does `INSERT INTO orders` during the HTTP request would be simpler to deploy. But it couples client latency to database write time, makes the database the bottleneck under load, and provides no natural retry mechanism when a downstream dependency fails. The idempotency barrier would need to be bolted on as an afterthought (an idempotency table checked inside the same request). This works at low scale but degrades under contention.

### Why not Kafka?

Kafka excels at high-throughput, partitioned, replayable logs. But for this workload — per-message acknowledgements, selective retries with backoff, dead-letter routing — Kafka requires building a lot of state machinery externally (consumer offsets, retry topics with pause/resume, a separate dead-letter mechanism). RabbitMQ's TTL + DLX primitives give us retry with backoff in ~30 lines of topology declaration. The tradeoff is throughput: Kafka would handle millions of messages/second; RabbitMQ handles tens of thousands. For order processing, the latter is plenty.

### Why not a single language?

The API is C# because ASP.NET Minimal APIs give us model binding, validation attributes, and JSON serialization with very little code. The processor is Rust because it owns the critical path — database writes, retry state, concurrency — where memory safety and predictable performance matter. A single-language codebase (all Rust or all C#) would reduce cognitive overhead but would force one ecosystem's weaknesses into the other's domain: Rust HTTP services are more verbose; C# async database code with manual retry logic is harder to get right than Rust's `sqlx` + `thiserror`.

### Why not have the API write to the database?

If the API wrote to PostgreSQL directly (e.g., inserting into `idempotency_keys` and `orders` in one request), it would need its own database connection pool, its own idempotency logic, and its own retry handling. The processor would still exist for async work, and now two services write to the same tables — a coordination problem. Keeping the API write-free avoids split-brain scenarios.

The read path (GET endpoint) intentionally stays `SELECT`-only. It queries the tables the processor owns. This is a pragmatic compromise: the API gets enough database access to serve status queries, but not enough to create write conflicts.

### Why not a separate read service (CQRS)?

A purist CQRS approach would add a fourth service — a read-only query service with its own connection pool, deployed independently. For this scale, that's over-engineering. The API's read path is a handful of `SELECT` queries with no write transactions. It can be extracted into a separate service later if traffic demands it. Until then, co-locating reads in the API keeps the deployment count at three services instead of four.

---

## Failure Modes & Recovery

| Scenario | Behavior |
|----------|----------|
| **API crashes mid-publish** | Broker never gets the message. Client gets a 5xx and retries with the same `correlation_id`. Idempotency handles the duplicate. |
| **Broker crashes** | API returns 5xx on POST. GET continues to work (it reads PostgreSQL directly). Processor reconnects automatically. Messages on durable queues survive. |
| **Processor crashes mid-write** | Message is not acked. Broker redelivers to another consumer (or the restarted one). `try_claim_idempotency` returns `false` for the redelivery → acked as duplicate. |
| **Database deadlock** | Caught as `Transient` error. Message goes through the retry exchange with exponential backoff. GET requests may time out or return a stale status while the lock is held. |
| **Network partition (processor ↔ DB)** | Same as database deadlock — transient errors, retried with backoff. After max retries, DLQ. |
| **Poison message (invalid JSON)** | Caught at parse time. `nack_dead_letter` immediately — no retry budget wasted. |
| **Max retries exhausted** | `mark_failed` in `idempotency_keys`, record attempt, `nack_dead_letter`. Message lands in DLQ with full audit trail. GET endpoint returns status `failed` with `last_error`. |
| **Database outage during GET** | `OrderReader` throws. ASP.NET returns `500`. The write path is also affected (processor can't persist). |

---

## Scaling

- **API**: Stateless for writes. Read path adds per-request database connections (Npgsql pools internally). Scale horizontally behind a load balancer. No session affinity needed.
- **Processor**: Scale by adding instances. RabbitMQ round-robins messages across consumers on the same queue. Each instance needs its own DB connection pool. The idempotency barrier (PostgreSQL unique constraint) serializes conflicting writes safely.
- **Database**: The bottleneck. Read replicas for queries (the GET endpoint could target a replica). Connection pooling tuned to processor count × instance count + API instance count. For very high volume, partition `orders` by time.
- **Broker**: RabbitMQ clustering with mirrored queues for HA. The retry topology adds queue depth — monitor `orders.created.retry` message count as a leading indicator of downstream issues.

---

## Summary

This is an event-driven, at-least-once delivery, exactly-once processing pipeline
with a synchronous read path for status queries. The API accepts and forgets
(write path) then queries the database for progress (read path). The broker
buffers and routes. The processor validates and persists. The database enforces
uniqueness and serves both the processor's writes and the API's reads.

The architecture is not novel — it's the standard pattern for payment systems,
order management, and any domain where submissions must be accepted quickly and
processed reliably. What makes this implementation notable is that it gets the
details right: the idempotency barrier is a single atomic INSERT, retries use
broker-native TTL rather than in-process timers, and the error taxonomy
(transient/permanent/already-processed) maps cleanly to the broker's ack/nack/DLX
primitives.
