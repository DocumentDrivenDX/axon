use thiserror::Error;

use crate::types::Entity;

use serde_json::{json, Value};

/// Browser-facing shape for schema validation failures.
pub fn schema_validation_detail(detail: &str) -> Value {
    json!({
        "message": detail,
        "field_errors": schema_validation_field_errors(detail),
    })
}

/// Best-effort field-level errors reconstructed from Axon's validation detail.
///
/// JSON Schema failures are produced as `/<json-pointer>: <message>`.
/// Other schema validation failures, such as rule or template validation,
/// remain available through `message` with an empty `field_errors` array.
pub fn schema_validation_field_errors(detail: &str) -> Vec<Value> {
    detail
        .split("; ")
        .filter_map(|part| {
            let (field_path, rest) = part.split_once(": ")?;
            if !field_path.is_empty() && !field_path.starts_with('/') {
                return None;
            }
            let field_path = if field_path.is_empty() {
                "/"
            } else {
                field_path
            };
            let (message, fix) = rest
                .split_once(" Fix: ")
                .map_or((rest, None), |(message, fix)| (message, Some(fix)));
            let mut error = json!({
                "field_path": field_path,
                "message": message,
                "severity": "error",
            });
            if let Some(fix) = fix {
                error["fix"] = json!(fix);
            }
            Some(error)
        })
        .collect()
}

/// Structured policy denial detail returned to API clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDenial {
    pub reason: String,
    pub collection: String,
    pub entity_id: Option<String>,
    pub field_path: Option<String>,
    pub policy: Option<String>,
    pub operation_index: Option<usize>,
    pub missing_index: Option<String>,
    pub cost_limit: Option<usize>,
    pub candidate_count: Option<usize>,
}

impl PolicyDenial {
    pub fn new(reason: impl Into<String>, collection: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            collection: collection.into(),
            entity_id: None,
            field_path: None,
            policy: None,
            operation_index: None,
            missing_index: None,
            cost_limit: None,
            candidate_count: None,
        }
    }

    pub fn detail(&self) -> Value {
        let mut detail = json!({
            "reason": &self.reason,
            "collection": &self.collection,
        });
        if let Some(entity_id) = &self.entity_id {
            detail["entity_id"] = json!(entity_id);
        }
        if let Some(field_path) = &self.field_path {
            detail["field_path"] = json!(field_path);
        }
        if let Some(policy) = &self.policy {
            detail["policy"] = json!(policy);
        }
        if let Some(operation_index) = self.operation_index {
            detail["operation_index"] = json!(operation_index);
        }
        if let Some(missing_index) = &self.missing_index {
            detail["missing_index"] = json!(missing_index);
        }
        if let Some(cost_limit) = self.cost_limit {
            detail["cost_limit"] = json!(cost_limit);
        }
        if let Some(candidate_count) = self.candidate_count {
            detail["candidate_count"] = json!(candidate_count);
        }
        detail
    }

    pub fn is_policy_filter_unindexed(&self) -> bool {
        self.reason == "policy_filter_unindexed"
    }
}

impl std::fmt::Display for PolicyDenial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "policy denied: reason={} collection={}",
            self.reason, self.collection
        )?;
        if let Some(entity_id) = &self.entity_id {
            write!(f, " entity_id={entity_id}")?;
        }
        if let Some(field_path) = &self.field_path {
            write!(f, " field_path={field_path}")?;
        }
        if let Some(policy) = &self.policy {
            write!(f, " policy={policy}")?;
        }
        if let Some(operation_index) = self.operation_index {
            write!(f, " operation_index={operation_index}")?;
        }
        if let Some(missing_index) = &self.missing_index {
            write!(f, " missing_index={missing_index}")?;
        }
        if let Some(cost_limit) = self.cost_limit {
            write!(f, " cost_limit={cost_limit}")?;
        }
        if let Some(candidate_count) = self.candidate_count {
            write!(f, " candidate_count={candidate_count}")?;
        }
        Ok(())
    }
}

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
        current_entity: Option<Box<Entity>>,
    },

    #[error("already exists: {0}")]
    AlreadyExists(String),

    /// Unique index constraint violation.
    ///
    /// A write would produce a duplicate value on a unique index.
    #[error("unique index violation on field `{field}`: value {value} already exists")]
    UniqueViolation {
        /// The indexed field path that was violated.
        field: String,
        /// String representation of the duplicate value.
        value: String,
    },

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    /// Named lifecycle not found on the collection schema.
    #[error("lifecycle not found: {lifecycle_name}")]
    LifecycleNotFound {
        /// The lifecycle name that was not found.
        lifecycle_name: String,
    },

    /// Entity is not in a state that allows the requested transition.
    #[error(
        "invalid transition in lifecycle `{lifecycle_name}`: \
         cannot transition from `{current_state}` to `{target_state}`"
    )]
    InvalidTransition {
        /// The lifecycle name.
        lifecycle_name: String,
        /// The entity's current state.
        current_state: String,
        /// The requested target state.
        target_state: String,
        /// States that are reachable from `current_state`.
        valid_transitions: Vec<String>,
    },

    /// Entity is missing a value at the lifecycle field.
    ///
    /// Raised on update when the entity payload has no value at the field
    /// named by a `LifecycleDef`. Create operations auto-populate with the
    /// lifecycle's `initial` state instead of raising this error.
    #[error("lifecycle field `{field}` is missing from entity data")]
    LifecycleFieldMissing {
        /// The lifecycle field path that is missing.
        field: String,
    },

    /// Entity has an invalid value at the lifecycle field.
    ///
    /// The value is either not a string or is a string that is not a known
    /// state for the lifecycle (not the `initial` state and not reachable
    /// from any transition).
    #[error("lifecycle field `{field}` has invalid value {actual}")]
    LifecycleStateInvalid {
        /// The lifecycle field path that holds the invalid value.
        field: String,
        /// The offending value as-seen in the entity data.
        actual: serde_json::Value,
    },

    /// Access to the resource or operation is denied.
    ///
    /// Returned by control-plane authorization helpers when the caller does
    /// not hold the required deployment-admin or tenant-admin role.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// Data-layer policy denied the operation or rejected an unsafe read plan.
    #[error("forbidden: {0}")]
    PolicyDenied(Box<PolicyDenial>),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Per-actor mutation rate limit was exceeded (FEAT-022 / ADR-016).
    ///
    /// `retry_after_ms` is the number of milliseconds the caller should wait
    /// before retrying. The HTTP layer maps this to `429 Too Many Requests`
    /// with a `Retry-After` header; the gRPC layer uses `RESOURCE_EXHAUSTED`.
    #[error("rate limit exceeded for actor `{actor}`: retry after {retry_after_ms}ms")]
    RateLimitExceeded {
        /// The actor whose bucket was exhausted.
        actor: String,
        /// How many milliseconds until enough tokens refill for one mutation.
        retry_after_ms: u64,
    },

    /// The caller's scope filter rejected this mutation (FEAT-022 / ADR-016).
    ///
    /// The actor's `entity_filter` did not match the target entity's data.
    /// Includes the filter that was applied so callers can see the boundary
    /// they crossed.
    #[error(
        "scope violation for actor `{actor}` on entity `{entity_id}`: \
         filter `{filter_field}` requires value `{filter_value}`"
    )]
    ScopeViolation {
        /// The actor whose scope was violated.
        actor: String,
        /// The target entity ID that the actor was denied access to.
        entity_id: String,
        /// The filter field that was checked.
        filter_field: String,
        /// The required value for the filter field.
        filter_value: serde_json::Value,
    },
}
