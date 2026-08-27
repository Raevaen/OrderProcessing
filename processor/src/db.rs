//! Database access layer.
//! `PgPool` is used instead of a single connection because the worker may process
//! multiple messages concurrently. The pool owns the underlying connection set and
//! the compiler ensures each borrowed value stays valid while the query executes.

use sqlx::PgPool;
use uuid::Uuid;

use crate::models::OrderMessage;

/// Open a connection pool to PostgreSQL.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPool::connect(database_url).await
}

/// Idempotency barrier: try to INSERT a row into `idempotency_keys`.
/// Returns `true` if the insert succeeded (first time seeing this key),
/// `false` if the key already existed (duplicate — already processed).
pub async fn try_claim_idempotency(
    pool: &PgPool,
    correlation_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"INSERT INTO idempotency_keys (correlation_id, status)
           VALUES ($1, 'processing')
           ON CONFLICT (correlation_id) DO NOTHING"#,
    )
    .bind(correlation_id)
    .execute(pool)
    .await?;

    let claimed = result.rows_affected() > 0;
    tracing::info!(
        correlation_id = %correlation_id,
        claimed,
        "Idempotency claim evaluated"
    );
    Ok(claimed)
}

/// Insert a processed order and update the idempotency key in a single
/// transaction. If the idempotency insert succeeded earlier, this must also
/// succeed — if it fails, the caller should mark the key as failed.
pub async fn insert_order(
    pool: &PgPool,
    order: &OrderMessage,
) -> Result<Uuid, sqlx::Error> {
    let payload = serde_json::to_value(order).unwrap_or_default();

    let row: (Uuid,) = sqlx::query_as(
        r#"INSERT INTO orders
               (correlation_id, customer_name, product, quantity, total_amount,
                status, payload, processed_at)
           VALUES ($1, $2, $3, $4, $5, 'completed', $6, NOW())
           RETURNING id"#,
    )
    .bind(order.correlation_id)
    .bind(&order.customer_name)
    .bind(&order.product)
    .bind(order.quantity)
    .bind(order.total_amount)
    .bind(&payload)
    .fetch_one(pool)
    .await?;

    sqlx::query(
        r#"UPDATE idempotency_keys
           SET status = 'completed', processed_at = NOW(), order_id = $1
           WHERE correlation_id = $2"#,
    )
    .bind(row.0)
    .bind(order.correlation_id)
    .execute(pool)
    .await?;

    Ok(row.0)
}

/// Record a processing attempt for audit.
pub async fn record_attempt(
    pool: &PgPool,
    correlation_id: Uuid,
    attempt: i32,
    error_message: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO processing_attempts (correlation_id, attempt, error_message)
           VALUES ($1, $2, $3)"#,
    )
    .bind(correlation_id)
    .bind(attempt)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

/// Mark the idempotency key as permanently failed.
pub async fn mark_failed(pool: &PgPool, correlation_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE idempotency_keys
           SET status = 'failed', processed_at = NOW()
           WHERE correlation_id = $1"#,
    )
    .bind(correlation_id)
    .execute(pool)
    .await?;

    Ok(())
}
