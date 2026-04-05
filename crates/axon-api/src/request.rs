use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::id::{CollectionId, EntityId};

/// Request to create a new entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    pub data: Value,
    /// Optional actor identity for the audit log.
    pub actor: Option<String>,
}

/// Request to read an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
}

/// Request to update an existing entity using optimistic concurrency control.
///
/// The write succeeds only if the stored version equals `expected_version`.
/// On conflict, [`axon_core::error::AxonError::ConflictingVersion`] is returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    /// Replacement data for the entity.
    pub data: Value,
    /// The version the caller believes is current. Must match the stored version.
    pub expected_version: u64,
    pub actor: Option<String>,
}

/// Request to delete an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEntityRequest {
    pub collection: CollectionId,
    pub id: EntityId,
    pub actor: Option<String>,
}

/// Request to create a typed link between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLinkRequest {
    pub source_collection: CollectionId,
    pub source_id: EntityId,
    pub target_collection: CollectionId,
    pub target_id: EntityId,
    /// Semantic label for the edge (e.g., `"belongs-to"`, `"depends-on"`).
    pub link_type: String,
    /// Optional metadata stored on the link.
    #[serde(default)]
    pub metadata: Value,
    pub actor: Option<String>,
}

/// Request to traverse links from a starting entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseRequest {
    /// Starting entity.
    pub collection: CollectionId,
    pub id: EntityId,
    /// Filter traversal to this link type. If `None`, follow all link types.
    pub link_type: Option<String>,
    /// Maximum hop depth (default: 3, capped at 10).
    pub max_depth: Option<usize>,
}

// ── Audit requests ───────────────────────────────────────────────────────────

/// Request to query the audit log with optional filters and cursor-based pagination.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryAuditRequest {
    /// Restrict to entries for this collection.
    pub collection: Option<CollectionId>,
    /// Restrict to entries for this entity.
    pub entity_id: Option<EntityId>,
    /// Restrict to entries produced by this actor.
    pub actor: Option<String>,
    /// Restrict to entries of this operation type (e.g. `"entity_create"`).
    pub operation: Option<String>,
    /// Inclusive start of the time range (nanoseconds since Unix epoch).
    pub since_ns: Option<u64>,
    /// Inclusive end of the time range (nanoseconds since Unix epoch).
    pub until_ns: Option<u64>,
    /// Pagination cursor: last entry ID seen. Omit to start from the beginning.
    pub after_id: Option<u64>,
    /// Maximum number of entries to return per page.
    pub limit: Option<usize>,
}

/// Request to revert an entity to the `before` state recorded in an audit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertEntityRequest {
    /// The audit entry whose `before` state should be restored.
    pub audit_entry_id: u64,
    /// Actor performing the revert.
    pub actor: Option<String>,
    /// When `true`, bypass schema validation for the restored state.
    /// Use only when the schema has changed since the audit entry was recorded.
    #[serde(default)]
    pub force: bool,
}

// ── Collection lifecycle requests ────────────────────────────────────────────

/// Request to explicitly create a named collection and record the event in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollectionRequest {
    pub name: CollectionId,
    pub actor: Option<String>,
}

/// Request to drop a collection and all its entities, recording the event in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropCollectionRequest {
    pub name: CollectionId,
    pub actor: Option<String>,
}
