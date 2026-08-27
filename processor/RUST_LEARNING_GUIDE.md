# Rust learning guide for this project

This project is a small asynchronous Rust service that reads orders from RabbitMQ, validates them, stores them in PostgreSQL, and applies retry and dead-letter patterns. The code is intentionally written to teach good Rust habits: explicit ownership, strong typing, explicit errors, and concurrency controls.

## 1. Why this syntax is used

### `#[tokio::main]`

Rust does not have a hidden runtime like many other languages. The code decides exactly which async runtime it uses. In this project the runtime is Tokio, so the entry point is:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
```

This tells Rust:
- use Tokio as the async executor,
- allow `async fn` and `.await` expressions,
- generate the runtime boilerplate that starts the event loop.

This is a different mental model from languages that have a global event loop hidden behind the scenes.

### `Result<T, E>` and `?`

Rust does not use exceptions. Errors are explicit values. This project uses `anyhow::Result<()>` for the top-level app and `Result<(), ProcessingError>` for domain logic.

```rust
let pool = db::create_pool(&config.database_url).await?;
```

The `?` operator means:
- if the value is `Ok`, continue,
- if the value is `Err`, return early from the current function.

This makes failures visible in the type system and prevents accidental silent failures.

### `match` and `if let`

Rust prefers structural pattern matching over broad exception-based control flow. For example, message handling does:

```rust
match processor::process_order(pool, &order).await {
    Ok(()) => { /* success */ }
    Err(e) if e.is_transient() => { /* retry */ }
    Err(e) => { /* permanent failure */ }
}
```

This is the Rust way to express "choose behavior based on state" without hidden control flow.

## 2. Why these objects exist

### `Config`

`Config` is a plain Rust struct used to hold runtime options:

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub rabbitmq_url: String,
    pub database_url: String,
    pub main_queue: String,
    // ...
}
```

A struct is used because the app needs many values with a clear shape. The `Clone` derive matters because the config is shared across spawned async tasks. Sharing an immutable value safely is a common pattern in Rust.

### `Arc<T>`

`Arc` stands for atomically reference counted pointer. It allows multiple tasks to share the same data without moving ownership between them.

```rust
let channel = Arc::new(channel);
let pool = Arc::new(pool);
let config = Arc::new(config);
```

Each spawned task gets a clone of the pointer:

```rust
let ch = Arc::clone(&channel);
```

This is the Rust way to share data across threads/tasks while keeping the data alive until all references are dropped.

### `OrderMessage`

This struct represents the message contract from the API to the processor:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderMessage {
    pub correlation_id: Uuid,
    pub order_id: Uuid,
    pub customer_name: String,
    pub product: String,
    pub quantity: i32,
    pub total_amount: Decimal,
    pub timestamp: DateTime<Utc>,
}
```

The `Serialize` and `Deserialize` derives make it easy to convert this object to JSON and back. This is important because RabbitMQ messages are bytes, and JSON is the wire format. `Uuid`, `Decimal`, and `DateTime` are chosen to avoid ad hoc string conversions and preserve type safety.

### `ProcessingError`

The project uses an enum instead of a single generic error string:

```rust
#[derive(Error, Debug)]
pub enum ProcessingError {
    Transient(String),
    Permanent(String),
    AlreadyProcessed(Uuid),
}
```

This makes the failure intent explicit. A retryable database problem is not the same as an invalid order. The type system makes that distinction obvious, and the `is_transient` method is used to decide whether to retry.

## 3. How memory is handled in Rust

Rust has a memory model built around ownership, borrowing, and lifetimes.

### Ownership

Every value in Rust has one owner. When a value is moved, the old binding stops being valid.

```rust
let config = config::Config::from_env();
let config = Arc::new(config);
```

This is different from garbage-collected languages where a value may be referenced by many places without a clear owner. Rust's rule is simple:

- each value has one owner,
- when the owner goes out of scope, the value is dropped,
- there is no hidden garbage collector.

### Borrowing

A function can borrow a value instead of taking ownership:

```rust
fn validate(order: &OrderMessage) -> Result<(), ProcessingError>
```

The `&` means "borrow this value for a while". The function can read the object without taking ownership of it. This is why the code can pass references to `order` and `config` to many helper functions while still owning them elsewhere.

### Stack vs heap

Rust values live either on the stack or on the heap:
- small fixed-size values can live on the stack,
- `String`, `Vec`, `Box`, `Arc`, and many other types allocate on the heap.

Examples from this project:
- `String` stores the text in heap memory,
- `Uuid` is a value type but internally carries a byte array,
- `Arc<T>` is a heap pointer that allows shared ownership.

Because the compiler tracks ownership, there is no need for a runtime garbage collector and no use-after-free bug by default.

### `Arc` and concurrency safety

The processor creates multiple async tasks that need access to the same database pool and RabbitMQ channel. These shared resources are wrapped in `Arc`:

```rust
let channel = Arc::new(channel);
let pool = Arc::new(pool);
```

`Arc<T>` is safe to share between threads/tasks because it uses atomic reference counting. It does not allow mutation by itself. If a value needs mutation, the usual pattern is `Arc<Mutex<T>>` or `Arc<RwLock<T>>`. In this project, the database and channel are mostly used immutably; the mutation is handled by the library or by the connection internals.

### `Result` and memory safety

The compiler prevents invalid states at compile time. For example, dangling references cannot happen because references must always be valid for their lifetime. This is the central reason Rust is considered memory-safe.

## 4. File-by-file explanation

### `main.rs`

This is the application entry point. It sets up logging, loads environment variables, opens the database pool, connects to RabbitMQ, declares the queue topology, and starts the consumer loop.

The important Rust choices here are:
- `#[tokio::main]` for the async runtime,
- `Arc` for shared ownership across tasks,
- `tokio::select!` for waiting on two sources at once: incoming messages and shutdown signals.

This is a classic Rust design for an event-driven system.

### `config.rs`

This file reads values from environment variables into a strongly typed `Config` structure. Using environment variables is a common 12-factor style pattern for containerized services.

The `unwrap_or_else` fallback values provide defaults for Docker/local development. This keeps the application runnable even when a `.env` file is not present.

### `models.rs`

This file defines the data contract for messages and database rows.

The `Serialize` and `Deserialize` derives are chosen because RabbitMQ messages are bytes and the system uses JSON. The `Decimal` type is used instead of `f64` to match financial precision requirements; floating-point numbers can introduce rounding errors in money values.

### `db.rs`

This file handles PostgreSQL operations. `PgPool` is chosen over opening a single connection per request because a worker service needs concurrency and reuse. `sqlx` uses compile-time checked SQL queries, which makes this pattern safer than manually concatenating SQL strings.

The `INSERT ... ON CONFLICT DO NOTHING` pattern is an idempotency guard. It prevents duplicate processing when the same `correlation_id` arrives multiple times.

### `messaging.rs`

This file is responsible for RabbitMQ topology and message handling. It declares:
- the main exchange,
- the DLX and DLQ,
- the retry exchange and retry queue,
- the queue bindings.

The project uses `FieldTable` and `AMQPValue` to attach metadata like retry counters and expiration values. This is important because the message itself carries the retry state in headers.

### `error.rs`

This file models domain-specific errors with a typed enum. This is a very Rust-idiomatic pattern: instead of a free-form string, the compiler can branch on exact variants and decisions can be made cleanly.

`ProcessingError::Transient` means retry. `Permanent` means no retry. `AlreadyProcessed` is a special idempotent case that safely acknowledges a duplicate message.

### `processor.rs`

This file contains business rules, idempotency, and persistence orchestration.

The validation function checks the message is semantically valid. If it is invalid, the code returns a permanent failure so that the message goes directly to the DLQ. If the order is valid, the code claims the idempotency key and then persists the order.

This design prevents duplicate processing even when the same event is retried or redelivered by RabbitMQ.

### `handler.rs`

This file is the message lifecycle handler. It converts the incoming bytes to UTF-8, then to JSON, then decides the next state:
- ack the message,
- retry after a delay,
- send to the DLQ,
- ignore a duplicate already-processed message.

This is where Rust's `match` expressions shine. Each outcome is explicitly handled instead of implicitly falling through.

## 5. The core Rust idea behind the whole project

The project is designed to teach a beginner the following Rust principles:

1. Ownership is explicit and safe.
2. Borrowing avoids copying large data and keeps lifetimes valid.
3. Enums and `match` are the primary way to model state.
4. `Result` is the normal error-handling mechanism.
5. `Arc` is the standard way to share data across async tasks.
6. `async` functions and Tokio let the program remain concurrent without sacrificing memory safety.
7. Strong types prevent mistakes in messages, IDs, money, and retry state.

In short, this code is not just a service. It is a small Rust classroom showing how an application is structured in a memory-safe, concurrent, typed way.
