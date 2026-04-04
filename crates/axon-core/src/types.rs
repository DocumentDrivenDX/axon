use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{CollectionId, EntityId};

/// The name of the internal collection used to store links.
pub const LINKS_COLLECTION: &str = "__axon_links__";

/// A typed directional edge between two entities.
///
/// Links are stored as entities in the [`LINKS_COLLECTION`] pseudo-collection
/// so that the existing `StorageAdapter` trait can persist them without
/// requiring a separate storage interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    /// Collection of the source entity.
    pub source_collection: CollectionId,
    /// ID of the source entity.
    pub source_id: EntityId,
    /// Collection of the target entity.
    pub target_collection: CollectionId,
    /// ID of the target entity.
    pub target_id: EntityId,
    /// Semantic label for this edge (e.g., `"belongs-to"`, `"depends-on"`).
    pub link_type: String,
    /// Optional metadata attached to this edge.
    #[serde(default)]
    pub metadata: Value,
}

impl Link {
    /// Computes the canonical storage ID for a link entity.
    ///
    /// Format: `<source_col>/<source_id>/<link_type>/<target_col>/<target_id>`
    pub fn storage_id(
        source_col: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_col: &CollectionId,
        target_id: &EntityId,
    ) -> EntityId {
        EntityId::new(format!(
            "{}/{}/{}/{}/{}",
            source_col, source_id, link_type, target_col, target_id,
        ))
    }

    /// Returns the internal collection that holds all links.
    pub fn links_collection() -> CollectionId {
        CollectionId::new(LINKS_COLLECTION)
    }

    /// Serializes this link into a storage [`Entity`].
    pub fn to_entity(&self) -> Entity {
        let storage_id = Self::storage_id(
            &self.source_collection,
            &self.source_id,
            &self.link_type,
            &self.target_collection,
            &self.target_id,
        );
        Entity::new(
            Self::links_collection(),
            storage_id,
            serde_json::to_value(self).expect("Link is always serializable"),
        )
    }

    /// Deserializes a [`Link`] from a storage entity.
    pub fn from_entity(entity: &Entity) -> Option<Self> {
        serde_json::from_value(entity.data.clone()).ok()
    }
}

/// A versioned entity stored in a collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// The collection this entity belongs to.
    pub collection: CollectionId,
    /// Unique identifier within the collection.
    pub id: EntityId,
    /// Monotonically increasing version; starts at 1.
    pub version: u64,
    /// The entity data as an arbitrary JSON object.
    pub data: Value,
}

impl Entity {
    pub fn new(collection: CollectionId, id: EntityId, data: Value) -> Self {
        Self {
            collection,
            id,
            version: 1,
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entity_new_starts_at_version_one() {
        let entity = Entity::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            json!({"title": "hello"}),
        );
        assert_eq!(entity.version, 1);
        assert_eq!(entity.data["title"], "hello");
    }
}
