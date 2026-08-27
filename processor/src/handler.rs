//! RabbitMQ delivery handler.
//! It converts raw bytes into structured data and then branches on the outcome with
//! `match`. This pattern is idiomatic Rust for state-machine style work: parse,
//! validate, retry, or dead-letter, with each branch explicitly visible to the
//! reader.

use std::str;

use lapin::{message::Delivery, Channel};
use sqlx::PgPool;

use crate::config::Config;
use crate::db;
use crate::messaging::{self, get_retry_count};
use crate::models::OrderMessage;
use crate::processor;

/// Dispatch a single RabbitMQ delivery: parse, process, and ack/nack/retry.
pub async fn handle_delivery(
    delivery: Delivery,
    channel: &Channel,
    pool: &PgPool,
    config: &Config,
) {
    let tag = delivery.delivery_tag;

    // Parse
    let payload = match str::from_utf8(&delivery.data) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, delivery_tag = tag, "Invalid UTF-8 payload, dead-lettering");
            let _ = messaging::nack_dead_letter(channel, tag).await;
            return;
        }
    };

    let order: OrderMessage = match serde_json::from_str(payload) {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(error = %e, delivery_tag = tag, "Invalid JSON payload, dead-lettering");
            let _ = messaging::nack_dead_letter(channel, tag).await;
            return;
        }
    };

    tracing::info!(
        correlation_id = %order.correlation_id,
        order_id = %order.order_id,
        "Dispatching order"
    );

    let retry_count = get_retry_count(&delivery.properties.headers());

    match processor::process_order(pool, &order).await {
        Ok(()) => {
            tracing::info!(correlation_id = %order.correlation_id, "Processed OK");
            let _ = messaging::ack(channel, tag).await;
            tracing::info!(
                correlation_id = %order.correlation_id,
                delivery_tag = tag,
                "ACK sent after successful processing"
            );
        }

        Err(e) if matches!(e, crate::error::ProcessingError::AlreadyProcessed(_)) => {
            tracing::info!(
                correlation_id = %order.correlation_id,
                order_id = %order.order_id,
                "Idempotent — already processed"
            );
            let _ = messaging::ack(channel, tag).await;
            tracing::info!(
                correlation_id = %order.correlation_id,
                delivery_tag = tag,
                "ACK sent for duplicate message"
            );
        }

        Err(e) if e.is_transient() => {
            if retry_count >= config.max_retries {
                tracing::error!(
                    correlation_id = %order.correlation_id,
                    retry_count,
                    max = config.max_retries,
                    "Max retries exceeded, dead-lettering"
                );
                let _ = db::record_attempt(
                    pool,
                    order.correlation_id,
                    retry_count as i32 + 1,
                    Some(&e.to_string()),
                )
                .await;
                let _ = db::mark_failed(pool, order.correlation_id).await;
                let _ = messaging::nack_dead_letter(channel, tag).await;
            } else {
                let backoff_ms = config.base_backoff_ms * 2u64.pow(retry_count);
                tracing::warn!(
                    correlation_id = %order.correlation_id,
                    retry_count,
                    next_retry = retry_count + 1,
                    backoff_ms,
                    error = %e,
                    "Transient error — scheduling retry"
                );

                let _ = db::record_attempt(
                    pool,
                    order.correlation_id,
                    retry_count as i32 + 1,
                    Some(&e.to_string()),
                )
                .await;

                if let Err(pub_err) = messaging::publish_to_retry(
                    channel,
                    &config.retry_exchange,
                    &delivery.data,
                    retry_count + 1,
                    backoff_ms,
                )
                .await
                {
                    tracing::error!(error = %pub_err, "Failed to publish to retry exchange");
                    let _ = messaging::nack_dead_letter(channel, tag).await;
                    return;
                }

                let _ = messaging::ack(channel, tag).await;
            }
        }

        Err(e) => {
            // Permanent failure — no retry, straight to DLQ.
            tracing::error!(
                correlation_id = %order.correlation_id,
                error = %e,
                "Permanent error — dead-lettering"
            );
            let _ = db::record_attempt(
                pool,
                order.correlation_id,
                retry_count as i32 + 1,
                Some(&e.to_string()),
            )
            .await;
            let _ = db::mark_failed(pool, order.correlation_id).await;
            let _ = messaging::nack_dead_letter(channel, tag).await;
        }
    }
}
