use thiserror::Error;

use crate::types::Entity;

/// Top-level error type for Axon operations.
#[derive(Debug, Error)]
pub enum AxonError {
    #[error("entity not found: {0}")]
    NotFound(String),

    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    /// Optimistic concurrency conflict.
    ///
    /// `current_entity` holds the entity state at the time of the conflict so
    /// callers can inspect it, merge their changes, and retry with the correct
    /// version (FEAT-004, FEAT-008).
    #[error("optimistic concurrency conflict: expected version {expected}, got {actual}")]
    ConflictingVersion {
        expected: u64,
        actual: u64,
        /// The entity's current state at the time of the conflict.
        /// `None` when the entity does not exist (create-on-existing conflicts)
        /// or when the entity state is not available at the layer that detected
        /// the conflict.
        current_entity: Option<Entity>,
    },

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
