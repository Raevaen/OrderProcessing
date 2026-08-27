//! Rust data contracts used across the processor.
//! These structs use strong types (`Uuid`, `Decimal`, `DateTime`) so the message
//! format is precise and the compiler prevents accidental misuse of IDs, money,
//! and timestamps. Strings and nested JSON values allocate on the heap; the
//! struct still remains cheap to move around by value.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The canonical message contract between the C# API and this processor.
/// Serialized as JSON (snake_case) over RabbitMQ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderMessage {
    pub correlation_id: Uuid,
    pub order_id: Uuid,
    pub customer_name: String,
    pub product: String,
    pub quantity: i32,

    /// Matches C# `decimal`, serialized as a JSON number (e.g. 99.99).
    #[serde(with = "rust_decimal::serde::float")]
    pub total_amount: Decimal,

    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

/// Row from the `idempotency_keys` table.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct IdempotencyKey {
    pub correlation_id: Uuid,
    pub status: String,
    pub processed_at: Option<DateTime<Utc>>,
}

/// Row from the `processing_attempts` table.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct ProcessingAttempt {
    pub id: i64,
    pub correlation_id: Uuid,
    pub attempt: i32,
    pub error_message: Option<String>,
}

/// Row from the `orders` table.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct OrderRecord {
    pub id: Uuid,
    pub correlation_id: Uuid,
    pub customer_name: String,
    pub product: String,
    pub quantity: i32,
    pub total_amount: Decimal,
    pub status: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}
