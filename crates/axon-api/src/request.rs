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
