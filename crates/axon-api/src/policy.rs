use std::collections::HashMap;

use axon_core::id::CollectionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Request-local subject snapshot used for FEAT-029 policy evaluation.
///
/// The snapshot is resolved once at request start. Collection-backed
/// attributes are deliberately stored on this value instead of a handler-level
/// cache so subject data cannot leak between requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicySubjectSnapshot {
    /// Audit-facing actor name for this request.
    pub actor: String,
    /// Canonical and policy-declared subject bindings.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub bindings: HashMap<String, Value>,
    /// Request-scoped `subject.attributes.*` values resolved from identity
    /// attribute sources.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, Value>,
}

/// Schema/policy version snapshot used by one AxonHandler operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyRequestSnapshot {
    /// Collection the operation was evaluated against.
    pub collection: CollectionId,
    /// Schema namespace active at request start.
    pub namespace: String,
    /// Database context active at request start.
    pub database_id: String,
    /// Tenant identity from authenticated attribution, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    /// Collection schema version active at request start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    /// Policy version active at request start. In ADR-019 V1 this follows the
    /// collection schema version when `access_control` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version: Option<u32>,
    /// Resolved policy subject for this request.
    pub subject: PolicySubjectSnapshot,
}
