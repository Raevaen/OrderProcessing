# Solution Architecture

## High-Level Design

```
Client ──► C# API ──► RabbitMQ ──► Rust Processor ──► PostgreSQL
              │            │              │
              │       orders.retry        │
              │       (exponential        │
              │        backoff)           │
              │            │              │
              │       orders.dlx ───► Dead-Letter Queue
              │                         (manual inspection)
              │
              └── 202 Accepted (immediately)
```

The system is split into four runtimes: two applications and two infrastructure
services. The API publishes; the processor consumes. They share no in-memory
state and communicate only through the broker and the database.

---

## Language Choices

### C# / ASP.NET Core — API Layer

**Why C#.** ASP.NET Core's controller-model-binding-attribute-validation pipeline is a
mature, low-ceremony way to stand up a JSON API. The `[ApiController]`,
`[Required]`, and `[Range]` attributes handle input validation before a single
line of business logic runs. Serialization defaults (`System.Text.Json` with
`SnakeCaseLower` naming) align with the Rust side's serde expectations.

**Why not Rust (here too).** A Rust HTTP framework (axum, actix-web) would also
work, but the controller pattern with declarative validation attributes is more
concise in C#. The API has no business logic to speak of — it translates HTTP
into AMQP — so the language choice is driven by developer ergonomics, not
performance.

**Tradeoff.** Two languages means two build toolchains (dotnet + cargo) and two
Dockerfiles to maintain. The boundary is sharp enough that this cost is low:
the JSON contract is the only shared surface, and it changes rarely.

### Rust — Event Processor

**Why Rust.** The processor holds the business rules and the database writes.
It runs the idempotency check, validation, and persistence in a single async
flow. Rust's ownership model eliminates whole classes of concurrency bugs that
matter when you are spawning one tokio task per message. The
`INSERT ... ON CONFLICT DO NOTHING` at the database level is the final barrier,
but Rust's type system catches message parsing and validation errors at compile
time.

**Why not C# (here too).** C# could run this logic, but the processor is a
pure consumer — no HTTP, no middleware, no framework overhead. Rust's binary
size and startup time are smaller, and its async runtime (tokio) gives
predictable task scheduling without a garbage collector pause interfering with
ack deadlines.

**Tradeoff.** Async Rust has a learning curve. The `lapin` crate's API (AMQP
types, `FieldTable` ergonomics, stream combinators) required more explicit type
annotations and trait imports than the equivalent C# RabbitMQ client.

---

## Message Broker: RabbitMQ

**Why RabbitMQ.** The retry pattern — exponential backoff via per-message TTL —
maps directly onto RabbitMQ's dead-letter exchange primitives. A message
published to the retry exchange with `expiration=N` expires after N
milliseconds and is dead-lettered back to the main exchange. No application-level
sleep, no cron job, no scheduler table.

RabbitMQ's topic exchanges also give us routing flexibility: the `order.created`
routing key could later fan out to multiple queues for different consumers
(notification, analytics, fraud check) without changing the publisher.

**Why not Kafka.** Kafka's consumer-group model is pull-based and stores
offsets in an internal topic. Implementing per-message retry with backoff in
Kafka requires either a separate retry topic with a delay processor or external
state. Kafka is better for high-throughput event streaming; this system values
per-message reliability and dead-letter inspection over raw throughput.

**Tradeoff.** RabbitMQ is push-based. If the Rust processor falls behind,
messages accumulate in the queue (or in memory if not rate-limited). The
mitigation is a prefetch limit (`basic_qos`), which we would add in production.
The current backbone uses the default (unlimited) for simplicity.

---

## Idempotency Strategy

### Database-Level Barrier

```
INSERT INTO idempotency_keys (correlation_id, status)
VALUES ($1, 'processing')
ON CONFLICT (correlation_id) DO NOTHING
```

This is the single most important line in the system. It is:

- **Atomic.** Postgres guarantees that two concurrent inserts for the same key
  will result in exactly one row. No distributed lock, no consensus algorithm.
- **Durable.** The idempotency record lives in the same database as the order
  data. No external cache to invalidate.
- **Simple.** The processor checks `rows_affected() > 0`. If zero, the key was
  already claimed — ack the message and move on.

**Why not application-level dedup.** An in-memory set of seen correlation IDs
would not survive a restart. A Redis-based approach with `SETNX` would work
but introduces a second stateful service and a split-brain risk if Redis and
Postgres disagree. Keeping the barrier in the same transaction context as the
order insert eliminates the dual-write problem.

**Tradeoff.** The idempotency table grows unboundedly. For a high-volume system,
a partitioning or TTL strategy would be needed (partition by date, drop old
partitions). This backbone defers that concern — the table is indexed and a
Postgres `BIGINT` can handle millions of rows before it becomes an operational
concern.

---

## Retry Topology

```
orders.created.queue
  │  x-dead-letter-exchange: orders.dlx
  │  x-dead-letter-routing-key: order.created
  │
  ├─ process success ──► ACK
  │
  ├─ permanent error ──► NACK ──► orders.dlx ──► orders.created.dlq
  │
  └─ transient error ──► publish to orders.retry
                            │  expiration = 1000ms × 2^retry_count
                            │
                            ▼
                       orders.created.retry (queue)
                            │  x-dead-letter-exchange: orders.exchange
                            │  x-dead-letter-routing-key: order.created
                            │
                            ▼ (after TTL)
                       orders.exchange ──► orders.created.queue (retry)
```

**Why not NACK with requeue.** `basic_nack` with `requeue=true` sends the
message back to the head of the queue immediately. This creates a tight
retry loop with no backoff, starving other messages. The retry exchange with
per-message TTL decouples the delay from the consumer.

**Why not a delay queue per retry level.** Some systems create N queues
(`retry-1s`, `retry-5s`, `retry-30s`) and route messages between them.
The per-message TTL approach is simpler — one retry queue, the backoff is
a function of the retry count embedded in the `x-retry-count` header.

**Tradeoff.** If the retry queue grows large (a systemic outage), messages
expire at different times and may arrive at the main queue in a different order
than they were originally published. Ordering is not a requirement for this
system, so the tradeoff is acceptable. If strict FIFO were needed, a different
pattern (single-message prefetch, synchronous retry) would apply.

---

## Error Taxonomy

The processor distinguishes three outcomes:

| Outcome | Action | Example |
|---|---|---|
| Success | ACK, persist | Valid order |
| AlreadyProcessed | ACK, no-op | Duplicate correlation_id |
| Transient | Publish to retry (with backoff) | DB deadlock, connection reset |
| Permanent | NACK → DLQ | Empty customer_name, negative quantity |

**Transient vs permanent.** The distinction is made in validation (permanent)
and in error handling (transient is the default for database errors). The
caller can influence this: a `500` from an external service might be transient
(if it could come back) or permanent (if the response says "invalid account").

**Tradeoff.** Misclassifying a permanent error as transient wastes retry budget.
Misclassifying a transient error as permanent orphans a valid message. The
current taxonomy is conservative: parse errors and validation failures are
permanent; everything from the database is transient. A production system
would inspect error codes from external services to make finer-grained calls.

---

## What Is Missing (by Design)

- **Authentication/authorization.** The system is a backbone. Auth is
  orthogonal — it can be layered in front of the API with a reverse proxy
  (nginx, Envoy) or added to the controller later without changing the
  processor.
- **Frontend.** Same reasoning. A UI consumes the API; the API doesn't care
  whether the client is a web app, a mobile app, or a CLI.
- **Outbox pattern.** The API publishes to RabbitMQ before any database write.
  If the broker accepts the message but the API process crashes before
  returning `202`, the message will be processed (the broker has it), but the
  client doesn't know. An outbox table in the database (write the message,
  then a background worker polls and publishes) would close this gap at the
  cost of added latency and complexity.
- **Dead-letter replay.** Messages in the DLQ can be inspected in the RabbitMQ
  management UI, but there is no automated replay mechanism. A production
  system would add a DLQ consumer that can re-publish messages to the main
  exchange after manual review.
- **Observability.** Structured logging (JSON via `tracing-subscriber`) is
  present, but metrics (prometheus) and distributed tracing (OpenTelemetry)
  are not. The correlation_id flows through the AMQP header and the database;
  adding trace propagation is a well-understood incremental step.

---

## Scaling Characteristics

The current backbone is single-instance per service. The architecture scales
horizontally at two points:

- **API.** Stateless. Multiple instances behind a load balancer can all publish
  to the same exchange. The only shared state is the broker.
- **Processor.** Multiple consumers on the same queue compete for messages
  (RabbitMQ's round-robin dispatch). The idempotency barrier in the database
  ensures that even if two instances process the same message (rare, due to
  network partitions), only one succeeds.

The database is the scaling bottleneck. In its current form it is a single
Postgres instance. Read replicas and connection pooling (PgBouncer) are the
standard next steps before sharding becomes necessary.
