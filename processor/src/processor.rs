//! Business logic and validation for each order.
//! The core function uses references (`&OrderMessage`) to avoid moving the message
//! around while still allowing safe validation and persistence. The compiler
//! enforces that all borrowed data is valid for the duration of the function.

use sqlx::PgPool;
use tracing;

use crate::db;
use crate::error::ProcessingError;
use crate::models::OrderMessage;

/// Core business logic: validate the order, claim idempotency, persist.
///
/// # Returns
/// - `Ok(())` — order was processed and persisted.
/// - `Err(ProcessingError::AlreadyProcessed)` — duplicate correlation_id.
/// - `Err(ProcessingError::Transient)` — retryable failure (DB hiccup, etc.).
/// - `Err(ProcessingError::Permanent)` — invalid data, should not be retried.
pub async fn process_order(
    pool: &PgPool,
    order: &OrderMessage,
) -> Result<(), ProcessingError> {
    // --- Validation ---
    validate(order)?;

    // --- Idempotency barrier ---
    let claimed = db::try_claim_idempotency(pool, order.correlation_id)
        .await
        .map_err(|e| ProcessingError::Transient(format!("DB error claiming idempotency: {e}")))?;

    if !claimed {
        return Err(ProcessingError::AlreadyProcessed(order.correlation_id));
    }

    // --- Persist ---
    let order_id = db::insert_order(pool, order)
        .await
        .map_err(|e| {
            // If the insert fails, mark the key as failed so it can be retried.
            // Fire-and-forget — best effort.
            let pool = pool.clone();
            let cid = order.correlation_id;
            tokio::spawn(async move {
                let _ = db::mark_failed(&pool, cid).await;
            });
            ProcessingError::Transient(format!("DB error inserting order: {e}"))
        })?;

    tracing::info!(
        correlation_id = %order.correlation_id,
        db_order_id = %order_id,
        "Order persisted"
    );

    Ok(())
}

/// Business rule validation. Invalid orders are permanently rejected.
fn validate(order: &OrderMessage) -> Result<(), ProcessingError> {
    if order.customer_name.trim().is_empty() {
        return Err(ProcessingError::Permanent(
            "customer_name is empty".into(),
        ));
    }
    if order.product.trim().is_empty() {
        return Err(ProcessingError::Permanent("product is empty".into()));
    }
    if order.quantity <= 0 {
        return Err(ProcessingError::Permanent(
            "quantity must be > 0".into(),
        ));
    }
    if order.total_amount <= rust_decimal::Decimal::ZERO {
        return Err(ProcessingError::Permanent(
            "total_amount must be > 0".into(),
        ));
    }
    Ok(())
}
