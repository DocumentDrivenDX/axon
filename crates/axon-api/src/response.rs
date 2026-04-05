use serde::{Deserialize, Serialize};

use axon_audit::entry::AuditEntry;
use axon_core::types::{Entity, Link};

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
