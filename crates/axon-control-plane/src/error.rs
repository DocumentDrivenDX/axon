//! Control plane error hierarchy.

use thiserror::Error;

/// Errors returned by control plane operations.
#[derive(Debug, Error)]
pub enum ControlPlaneError {
    /// Referenced tenant does not exist.
    #[error("tenant not found: {0}")]
    TenantNotFound(String),

    /// A tenant with the same id already exists.
    #[error("tenant already exists: {0}")]
    TenantAlreadyExists(String),

    /// Input failed validation (empty name, invalid backing store, etc.).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Operation is not valid in the tenant's current lifecycle state.
    ///
    /// For example, attempting to deprovision a tenant that has not yet been
    /// provisioned, or registering a BYOC instance for a tenant that is in the
    /// `Terminated` state.
    #[error("invalid tenant state: {tenant_id} is {current:?}, cannot {operation}")]
    InvalidState {
        tenant_id: String,
        current: crate::model::TenantStatus,
        operation: String,
    },

    /// Backend store failure (e.g. Postgres connection error in a future
    /// adapter). The in-memory store never returns this variant.
    #[error("control plane store error: {0}")]
    Store(String),
}
