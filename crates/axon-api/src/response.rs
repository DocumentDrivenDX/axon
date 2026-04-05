use serde::{Deserialize, Serialize};

use axon_audit::entry::AuditEntry;
use axon_core::types::{Entity, Link};
use axon_schema::schema::CollectionSchema;

/// Response containing a retrieved entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEntityResponse {
    pub entity: Entity,
}

/// Response after successfully creating an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntityResponse {
    pub entity: Entity,
}

/// Response after successfully updating an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEntityResponse {
    /// The entity at its new version.
    pub entity: Entity,
}

/// Response after successfully deleting an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEntityResponse {
    pub collection: String,
    pub id: String,
}

/// Response after successfully creating a link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLinkResponse {
    pub link: Link,
}

/// Response after successfully deleting a link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteLinkResponse {
    pub source_collection: String,
    pub source_id: String,
    pub target_collection: String,
    pub target_id: String,
    pub link_type: String,
}

/// Response from a link-traversal query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseResponse {
    /// Entities reached from the starting entity, in BFS order.
    /// Does not include the starting entity itself.
    pub entities: Vec<Entity>,
}

// ── Audit responses ──────────────────────────────────────────────────────────

/// Response from an audit log query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAuditResponse {
    pub entries: Vec<AuditEntry>,
    /// Cursor for the next page. `None` when no further results exist.
    pub next_cursor: Option<u64>,
}

/// Response after reverting an entity to a previous state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertEntityResponse {
    /// The entity at its restored state.
    pub entity: Entity,
    /// The new audit entry produced by the revert operation.
    pub audit_entry: AuditEntry,
}

// ── Entity query response ─────────────────────────────────────────────────────

/// Response from a filtered entity query (US-011 / FEAT-004).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEntitiesResponse {
    /// Matching entities. Empty when `count_only` was requested.
    pub entities: Vec<Entity>,
    /// Total number of entities that matched the filter (before pagination).
    /// Always populated regardless of `count_only`.
    pub total_count: usize,
    /// Cursor for the next page. `None` when the result set is exhausted.
    pub next_cursor: Option<String>,
}

// ── Collection lifecycle responses ───────────────────────────────────────────

/// Response after creating a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollectionResponse {
    pub name: String,
}

/// Response after dropping a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropCollectionResponse {
    pub name: String,
    /// Number of entities that were removed.
    pub entities_removed: usize,
}

// ── Schema responses ─────────────────────────────────────────────────────────

/// Response after storing or updating a collection schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutSchemaResponse {
    pub schema: CollectionSchema,
}

/// Response after retrieving a collection schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSchemaResponse {
    pub schema: CollectionSchema,
}
