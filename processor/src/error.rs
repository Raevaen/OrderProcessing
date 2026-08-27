//! Domain errors for the order pipeline.
//! The enum carries the failure intent directly. Rust is chosen here so retryable
//! and non-retryable failures are differentiated in the type system instead of
//! being hidden inside single string messages.

use thiserror::Error;
use uuid::Uuid;

/// Errors the processor can encounter during message handling.
/// Transient errors trigger retry; permanent errors route to the DLQ.
#[derive(Error, Debug)]
pub enum ProcessingError {
    /// Recoverable — will be retried with backoff.
    #[error("transient: {0}")]
    Transient(String),

    /// Unrecoverable — will be routed directly to the DLQ.
    #[error("permanent: {0}")]
    Permanent(String),

    /// The correlation_id has already been processed. Safe to ack.
    #[error("already processed (idempotent): correlation_id={0}")]
    AlreadyProcessed(Uuid),
}

impl ProcessingError {
    pub fn is_transient(&self) -> bool {
        matches!(self, ProcessingError::Transient(_))
    }
}
