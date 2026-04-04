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
