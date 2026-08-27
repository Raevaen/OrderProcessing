//! Application entry point.
//! The project uses Tokio for async work, `Arc` to share the channel/pool/config
//! across concurrent tasks, and `tokio::select!` to cancel gracefully on Ctrl+C.
//! Rust's ownership rules keep the shared objects valid while multiple tasks read
//! from the same runtime state.

use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::TryStreamExt;
use lapin::options::BasicConsumeOptions;
use tokio::signal;

mod config;
mod db;
mod error;
mod handler;
mod messaging;
mod models;
mod processor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (local dev).
    let _ = dotenvy::dotenv();

    // Initialize structured logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .json()
        .init();

    let config = config::Config::from_env();

    tracing::info!(
        rabbitmq_url = %config.rabbitmq_url,
        database_url = %config.database_url,
        "Starting order processor"
    );

    // --- Database pool ---
    let pool = db::create_pool(&config.database_url).await?;
    tracing::info!("Database pool created");

    // --- RabbitMQ connection ---
    let conn = messaging::create_connection(&config.rabbitmq_url).await?;
    tracing::info!("Connected to RabbitMQ");

    let channel = conn.create_channel().await?;
    messaging::declare_topology(&channel, &config).await?;

    // --- Consumer stream ---
    let consumer = channel
        .basic_consume(
            &config.main_queue,
            "order-processor",
            BasicConsumeOptions {
                no_ack: false,
                ..Default::default()
            },
            Default::default(),
        )
        .await?;

    tracing::info!("Consuming from queue '{}'", config.main_queue);

    let stream = consumer.into_stream();
    tokio::pin!(stream);

    let channel = Arc::new(channel);
    let pool = Arc::new(pool);
    let config = Arc::new(config);

    // Graceful shutdown signal.
    let shutdown = async {
        let _ = signal::ctrl_c().await;
        tracing::info!("Shutdown signal received");
    };

    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            delivery = stream.next() => {
                match delivery {
                    Some(Ok(delivery)) => {
                        let ch = Arc::clone(&channel);
                        let db_pool = Arc::clone(&pool);
                        let cfg = Arc::clone(&config);

                        tokio::spawn(async move {
                            handler::handle_delivery(delivery, &ch, &db_pool, &cfg).await;
                        });
                    }
                    Some(Err(e)) => {
                        tracing::error!(error = %e, "Consumer stream error");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    None => {
                        tracing::warn!("Consumer stream ended, exiting");
                        break;
                    }
                }
            }
            _ = &mut shutdown => {
                tracing::info!("Shutting down consumer");
                break;
            }
        }
    }

    tracing::info!("Order processor stopped");
    Ok(())
}