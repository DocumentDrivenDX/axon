use serde::{Deserialize, Serialize};

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
