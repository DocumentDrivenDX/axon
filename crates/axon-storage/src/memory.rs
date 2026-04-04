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

    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        let Some(col) = self.data.get(collection) else {
            return Ok(vec![]);
        };

        let mut entities: Vec<&Entity> = col
            .values()
            .filter(|e| {
                start.map_or(true, |s| e.id.as_str() >= s.as_str())
                    && end.map_or(true, |en| e.id.as_str() <= en.as_str())
            })
            .collect();

        entities.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

        if let Some(n) = limit {
            entities.truncate(n);
        }

        Ok(entities.into_iter().cloned().collect())
    }

    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        let current_version = self
            .data
            .get(&entity.collection)
            .and_then(|col| col.get(&entity.id))
            .map(|e| e.version);

        match current_version {
            Some(v) if v == expected_version => {}
            Some(actual) => {
                return Err(AxonError::ConflictingVersion {
                    expected: expected_version,
                    actual,
                });
            }
            None => {
                return Err(AxonError::ConflictingVersion {
                    expected: expected_version,
                    actual: 0,
                });
            }
        }

        let updated = Entity {
            version: expected_version + 1,
            ..entity
        };

        self.data
            .entry(updated.collection.clone())
            .or_default()
            .insert(updated.id.clone(), updated.clone());

        Ok(updated)
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

    #[test]
    fn range_scan_returns_sorted_entities() {
        let mut store = MemoryStorageAdapter::default();
        store.put(entity("t-003")).unwrap();
        store.put(entity("t-001")).unwrap();
        store.put(entity("t-002")).unwrap();
        let results = store.range_scan(&tasks(), None, None, None).unwrap();
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-001", "t-002", "t-003"]);
    }

    #[test]
    fn range_scan_respects_start_end_and_limit() {
        let mut store = MemoryStorageAdapter::default();
        for i in 1..=5 {
            store.put(entity(&format!("t-00{i}"))).unwrap();
        }
        let start = EntityId::new("t-002");
        let end = EntityId::new("t-004");
        let results = store
            .range_scan(&tasks(), Some(&start), Some(&end), Some(2))
            .unwrap();
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-002", "t-003"]);
    }

    #[test]
    fn compare_and_swap_increments_version() {
        let mut store = MemoryStorageAdapter::default();
        store.put(entity("t-001")).unwrap();
        let updated = store
            .compare_and_swap(entity("t-001"), 1)
            .unwrap();
        assert_eq!(updated.version, 2);
        let stored = store.get(&tasks(), &EntityId::new("t-001")).unwrap().unwrap();
        assert_eq!(stored.version, 2);
    }

    #[test]
    fn compare_and_swap_rejects_wrong_version() {
        let mut store = MemoryStorageAdapter::default();
        store.put(entity("t-001")).unwrap();
        let err = store.compare_and_swap(entity("t-001"), 99).unwrap_err();
        assert!(
            matches!(err, AxonError::ConflictingVersion { expected: 99, actual: 1 }),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn compare_and_swap_rejects_missing_entity() {
        let mut store = MemoryStorageAdapter::default();
        let err = store.compare_and_swap(entity("ghost"), 1).unwrap_err();
        assert!(
            matches!(err, AxonError::ConflictingVersion { expected: 1, actual: 0 }),
            "unexpected error: {err}"
        );
    }
}
