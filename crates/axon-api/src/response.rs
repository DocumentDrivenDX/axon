use serde::{Deserialize, Serialize};

use axon_core::types::Entity;

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
