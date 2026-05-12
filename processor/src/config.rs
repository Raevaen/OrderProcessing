use std::env;

/// Runtime configuration read from environment variables (12-factor style).
/// Falls back to defaults suitable for the docker-compose setup.
#[derive(Debug, Clone)]
pub struct Config {
    pub rabbitmq_url: String,
    pub database_url: String,
    pub main_queue: String,
    pub retry_exchange: String,
    pub dlx_exchange: String,
    pub dlq_queue: String,
    pub max_retries: u32,
    pub base_backoff_ms: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            rabbitmq_url: env::var("RABBITMQ_URL")
                .unwrap_or_else(|_| "amqp://guest:guest@rabbitmq:5672/%2f".into()),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://postgres:postgres@postgres:5432/orderprocessing".into()
            }),
            main_queue: env::var("MAIN_QUEUE")
                .unwrap_or_else(|_| "orders.created.queue".into()),
            retry_exchange: env::var("RETRY_EXCHANGE")
                .unwrap_or_else(|_| "orders.retry".into()),
            dlx_exchange: env::var("DLX_EXCHANGE")
                .unwrap_or_else(|_| "orders.dlx".into()),
            dlq_queue: env::var("DLQ_QUEUE")
                .unwrap_or_else(|_| "orders.created.dlq".into()),
            max_retries: env::var("MAX_RETRIES")
                .unwrap_or_else(|_| "5".into())
                .parse()
                .unwrap_or(5),
            base_backoff_ms: env::var("BASE_BACKOFF_MS")
                .unwrap_or_else(|_| "1000".into())
                .parse()
                .unwrap_or(1000),
        }
    }
}
