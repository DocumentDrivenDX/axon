use std::collections::HashMap;

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;

use crate::adapter::StorageAdapter;

type CollectionMap = HashMap<EntityId, Entity>;

/// In-memory storage adapter for testing and development.
///
/// All data is lost when the adapter is dropped.
#[derive(Debug, Default)]
pub struct MemoryStorageAdapter {
    data: HashMap<CollectionId, CollectionMap>,
}

impl StorageAdapter for MemoryStorageAdapter {
    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        Ok(self
            .data
            .get(collection)
            .and_then(|col| col.get(id))
            .cloned())
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        self.data
            .entry(entity.collection.clone())
            .or_default()
            .insert(entity.id.clone(), entity);
        Ok(())
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        if let Some(col) = self.data.get_mut(collection) {
            col.remove(id);
        }
        Ok(())
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        Ok(self.data.get(collection).map_or(0, |col| col.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::{CollectionId, EntityId};
    use serde_json::json;

    fn tasks() -> CollectionId {
        CollectionId::new("tasks")
    }

    fn entity(id: &str) -> Entity {
        Entity::new(tasks(), EntityId::new(id), json!({"title": id}))
    }

    #[test]
    fn get_missing_returns_none() {
        let store = MemoryStorageAdapter::default();
        assert!(store
            .get(&tasks(), &EntityId::new("missing"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn put_then_get_roundtrip() {
        let mut store = MemoryStorageAdapter::default();
        store.put(entity("t-001")).unwrap();
        let found = store.get(&tasks(), &EntityId::new("t-001")).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().data["title"], "t-001");
    }

    #[test]
    fn count_reflects_puts_and_deletes() {
        let mut store = MemoryStorageAdapter::default();
        assert_eq!(store.count(&tasks()).unwrap(), 0);
        store.put(entity("t-001")).unwrap();
        store.put(entity("t-002")).unwrap();
        assert_eq!(store.count(&tasks()).unwrap(), 2);
        store.delete(&tasks(), &EntityId::new("t-001")).unwrap();
        assert_eq!(store.count(&tasks()).unwrap(), 1);
    }
}
