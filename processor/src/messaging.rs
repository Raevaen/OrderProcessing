//! RabbitMQ messaging layer.
//! This file shows how Rust models AMQP metadata using typed wrappers (`FieldTable`
//! and `AMQPValue`) instead of raw strings. The message body stays as bytes until
//! it is decoded to UTF-8 and JSON, which is a safe and explicit boundary between
//! transport data and application data.

use lapin::{
    options::*,
    types::{AMQPValue, FieldTable},
    BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind,
};

use crate::config::Config;

/// Connect to RabbitMQ and return the connection handle.
pub async fn create_connection(url: &str) -> Result<Connection, lapin::Error> {
    Connection::connect(url, ConnectionProperties::default()).await
}

/// Declare the complete topology: main exchange + queue, retry exchange + queue,
/// dead-letter exchange + queue. All declarations are idempotent.
pub async fn declare_topology(channel: &Channel, config: &Config) -> Result<(), lapin::Error> {
    // ---------- Main exchange ----------
    channel
        .exchange_declare(
            "orders.exchange",
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    // ---------- Dead-letter exchange + queue ----------
    channel
        .exchange_declare(
            &config.dlx_exchange,
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_declare(
            &config.dlq_queue,
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    channel
        .queue_bind(
            &config.dlq_queue,
            &config.dlx_exchange,
            "order.created",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    // ---------- Retry exchange + queue ----------
    // Messages published here expire after a per-message TTL and are
    // dead-lettered back to the main exchange for retry.
    channel
        .exchange_declare(
            &config.retry_exchange,
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;

    let mut retry_args = FieldTable::default();
    retry_args.insert(
        "x-dead-letter-exchange".into(),
        AMQPValue::LongString("orders.exchange".into()),
    );
    retry_args.insert(
        "x-dead-letter-routing-key".into(),
        AMQPValue::LongString("order.created".into()),
    );

    channel
        .queue_declare(
            "orders.created.retry",
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            retry_args,
        )
        .await?;

    channel
        .queue_bind(
            "orders.created.retry",
            &config.retry_exchange,
            "order.created",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    // ---------- Main queue (with DLX) ----------
    let mut main_args = FieldTable::default();
    main_args.insert(
        "x-dead-letter-exchange".into(),
        AMQPValue::LongString(config.dlx_exchange.clone().into()),
    );
    main_args.insert(
        "x-dead-letter-routing-key".into(),
        AMQPValue::LongString("order.created".into()),
    );

    channel
        .queue_declare(
            &config.main_queue,
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            main_args,
        )
        .await?;

    channel
        .queue_bind(
            &config.main_queue,
            "orders.exchange",
            "order.created",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    tracing::info!("RabbitMQ topology declared");
    Ok(())
}

/// Extract the retry counter from the message headers.
/// Returns 0 if the header is absent.
pub fn get_retry_count(headers: &Option<FieldTable>) -> u32 {
    let Some(table) = headers.as_ref() else {
        return 0;
    };

    let mut retries = None;
    for (k, v) in table {
        if k.as_str() == "x-retry-count" {
            retries = match v {
                AMQPValue::LongInt(n) => Some(*n as u32),
                AMQPValue::LongUInt(n) => Some(*n),
                _ => None,
            };
            break;
        }
    }
    retries.unwrap_or(0)
}

/// Publish a message to the retry exchange with an expiration TTL so it
/// returns to the main queue after the backoff period.
pub async fn publish_to_retry(
    channel: &Channel,
    retry_exchange: &str,
    body: &[u8],
    retry_count: u32,
    backoff_ms: u64,
) -> Result<(), lapin::Error> {
    let mut headers = FieldTable::default();
    headers.insert(
        "x-retry-count".into(),
        AMQPValue::LongInt(retry_count as i32),
    );

    let props = BasicProperties::default()
        .with_headers(headers)
        .with_expiration(backoff_ms.to_string().into())
        .with_delivery_mode(2); // persistent

    channel
        .basic_publish(
            retry_exchange,
            "order.created",
            BasicPublishOptions::default(),
            body,
            props,
        )
        .await?;

    Ok(())
}

/// Acknowledge a message.
pub async fn ack(channel: &Channel, delivery_tag: u64) -> Result<(), lapin::Error> {
    channel
        .basic_ack(delivery_tag, BasicAckOptions::default())
        .await
}

/// Negative-acknowledge (no requeue) — sends the message to the DLX.
pub async fn nack_dead_letter(
    channel: &Channel,
    delivery_tag: u64,
) -> Result<(), lapin::Error> {
    channel
        .basic_nack(delivery_tag, BasicNackOptions::default())
        .await
}