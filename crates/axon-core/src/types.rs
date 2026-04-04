use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{CollectionId, EntityId};

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
