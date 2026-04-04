use serde::{Deserialize, Serialize};

use axon_core::id::CollectionId;

/// Defines the structure and constraints for entities in a collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionSchema {
    /// The collection this schema governs.
    pub collection: CollectionId,
    /// Human-readable description.
    pub description: Option<String>,
    /// Schema version; incremented on each migration.
    pub version: u32,
}

impl CollectionSchema {
    pub fn new(collection: CollectionId) -> Self {
        Self {
            collection,
            description: None,
            version: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_new_starts_at_version_one() {
        let schema = CollectionSchema::new(CollectionId::new("tasks"));
        assert_eq!(schema.version, 1);
        assert!(schema.description.is_none());
    }
}
