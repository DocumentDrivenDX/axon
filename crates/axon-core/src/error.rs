use thiserror::Error;

/// Top-level error type for Axon operations.
#[derive(Debug, Error)]
pub enum AxonError {
    #[error("entity not found: {0}")]
    NotFound(String),

    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    #[error("optimistic concurrency conflict: expected version {expected}, got {actual}")]
    ConflictingVersion { expected: u64, actual: u64 },

    #[error("already exists: {0}")]
    AlreadyExists(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
