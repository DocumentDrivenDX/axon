use std::collections::{HashMap, HashSet};

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::CollectionSchema;

use crate::adapter::StorageAdapter;

type CollectionMap = HashMap<EntityId, Entity>;

/// Combined snapshot of mutable state captured at transaction start.
#[derive(Debug, Clone)]
struct TxSnapshot {
    data: HashMap<CollectionId, CollectionMap>,
    schemas: HashMap<CollectionId, CollectionSchema>,
    collections: HashSet<CollectionId>,
}

/// In-memory storage adapter for testing and development.
///
/// All data is lost when the adapter is dropped.
///
/// Transactions use a full-snapshot approach: [`begin_tx`] captures a clone of
/// the current state; [`abort_tx`] restores it; [`commit_tx`] discards the
/// snapshot. Because all mutations require `&mut self`, Rust's borrow checker
/// already provides exclusive access, so no additional synchronisation is
/// needed.
#[derive(Debug, Default)]
pub struct MemoryStorageAdapter {
    data: HashMap<CollectionId, CollectionMap>,
    /// Persisted schemas keyed by collection.
    schemas: HashMap<CollectionId, CollectionSchema>,
    /// Explicitly registered collections.
    collections: HashSet<CollectionId>,
    /// Snapshot saved at `begin_tx`; `Some` means a transaction is active.
    tx_snapshot: Option<TxSnapshot>,
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
        let current = self
            .data
            .get(&entity.collection)
            .and_then(|col| col.get(&entity.id))
            .cloned();

        match current.as_ref().map(|e| e.version) {
            Some(v) if v == expected_version => {}
            Some(actual) => {
                return Err(AxonError::ConflictingVersion {
                    expected: expected_version,
                    actual,
                    current_entity: current.map(Box::new),
                });
            }
            None => {
                return Err(AxonError::ConflictingVersion {
                    expected: expected_version,
                    actual: 0,
                    current_entity: None,
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

    fn begin_tx(&mut self) -> Result<(), AxonError> {
        if self.tx_snapshot.is_some() {
            return Err(AxonError::Storage("transaction already active".into()));
        }
        self.tx_snapshot = Some(TxSnapshot {
            data: self.data.clone(),
            schemas: self.schemas.clone(),
            collections: self.collections.clone(),
        });
        Ok(())
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        if self.tx_snapshot.is_none() {
            return Err(AxonError::Storage("no active transaction".into()));
        }
        self.tx_snapshot = None;
        Ok(())
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        if let Some(snapshot) = self.tx_snapshot.take() {
            self.data = snapshot.data;
            self.schemas = snapshot.schemas;
            self.collections = snapshot.collections;
        }
        Ok(())
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        self.schemas
            .insert(schema.collection.clone(), schema.clone());
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        Ok(self.schemas.get(collection).cloned())
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.schemas.remove(collection);
        Ok(())
    }

    fn register_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.collections.insert(collection.clone());
        Ok(())
    }

    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.collections.remove(collection);
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        let mut names: Vec<CollectionId> = self.collections.iter().cloned().collect();
        names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(names)
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
        let updated = store.compare_and_swap(entity("t-001"), 1).unwrap();
        assert_eq!(updated.version, 2);
        let stored = store
            .get(&tasks(), &EntityId::new("t-001"))
            .unwrap()
            .unwrap();
        assert_eq!(stored.version, 2);
    }

    #[test]
    fn compare_and_swap_rejects_wrong_version() {
        let mut store = MemoryStorageAdapter::default();
        store.put(entity("t-001")).unwrap();
        let err = store.compare_and_swap(entity("t-001"), 99).unwrap_err();
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 99,
                    actual: 1,
                    ..
                }
            ),
            "unexpected error: {err}"
        );
        // current_entity must contain the stored state so callers can merge
        if let AxonError::ConflictingVersion { current_entity, .. } = err {
            let ce =
                current_entity.expect("current_entity must be present on wrong-version conflict");
            assert_eq!(ce.version, 1);
        }
    }

    #[test]
    fn compare_and_swap_rejects_missing_entity() {
        let mut store = MemoryStorageAdapter::default();
        let err = store.compare_and_swap(entity("ghost"), 1).unwrap_err();
        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 1,
                    actual: 0,
                    ..
                }
            ),
            "unexpected error: {err}"
        );
        // No entity exists, so current_entity must be None.
        if let AxonError::ConflictingVersion { current_entity, .. } = err {
            assert!(
                current_entity.is_none(),
                "no entity should be present for missing-entity conflict"
            );
        }
    }

    #[test]
    fn begin_commit_tx_persists_writes() {
        let mut store = MemoryStorageAdapter::default();
        store.begin_tx().unwrap();
        store.put(entity("t-001")).unwrap();
        store.commit_tx().unwrap();
        assert!(store
            .get(&tasks(), &EntityId::new("t-001"))
            .unwrap()
            .is_some());
    }

    #[test]
    fn abort_tx_rolls_back_writes() {
        let mut store = MemoryStorageAdapter::default();
        store.put(entity("t-existing")).unwrap();

        store.begin_tx().unwrap();
        store.put(entity("t-new")).unwrap();
        store
            .delete(&tasks(), &EntityId::new("t-existing"))
            .unwrap();
        store.abort_tx().unwrap();

        // t-new must be gone, t-existing must be restored.
        assert!(store
            .get(&tasks(), &EntityId::new("t-new"))
            .unwrap()
            .is_none());
        assert!(store
            .get(&tasks(), &EntityId::new("t-existing"))
            .unwrap()
            .is_some());
    }

    #[test]
    fn begin_tx_rejects_nested_begin() {
        let mut store = MemoryStorageAdapter::default();
        store.begin_tx().unwrap();
        assert!(store.begin_tx().is_err());
        // Clean up.
        store.abort_tx().unwrap();
    }

    #[test]
    fn commit_tx_requires_active_transaction() {
        let mut store = MemoryStorageAdapter::default();
        assert!(store.commit_tx().is_err());
    }

    #[test]
    fn abort_tx_without_active_tx_is_noop() {
        let mut store = MemoryStorageAdapter::default();
        // Should not error.
        store.abort_tx().unwrap();
    }

    // ── Schema persistence ───────────────────────────────────────────────────

    #[test]
    fn put_get_schema_roundtrip() {
        use axon_schema::schema::CollectionSchema;
        let mut store = MemoryStorageAdapter::default();
        let col = tasks();
        let schema = CollectionSchema {
            collection: col.clone(),
            description: Some("my schema".into()),
            version: 3,
            entity_schema: None,
            link_types: Default::default(),
        };

        store.put_schema(&schema).unwrap();
        let retrieved = store.get_schema(&col).unwrap().unwrap();
        assert_eq!(retrieved.version, 3);
        assert_eq!(retrieved.description.as_deref(), Some("my schema"));
    }

    #[test]
    fn get_schema_missing_returns_none() {
        let store = MemoryStorageAdapter::default();
        assert!(store.get_schema(&tasks()).unwrap().is_none());
    }

    #[test]
    fn put_schema_overwrites_previous() {
        use axon_schema::schema::CollectionSchema;
        let mut store = MemoryStorageAdapter::default();
        let col = tasks();

        store
            .put_schema(&CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
            })
            .unwrap();
        store
            .put_schema(&CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 2,
                entity_schema: None,
                link_types: Default::default(),
            })
            .unwrap();

        assert_eq!(store.get_schema(&col).unwrap().unwrap().version, 2);
    }

    #[test]
    fn abort_tx_rolls_back_schema_changes() {
        use axon_schema::schema::CollectionSchema;
        let mut store = MemoryStorageAdapter::default();
        let col = tasks();
        let original = CollectionSchema {
            collection: col.clone(),
            description: Some("v1".into()),
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
        };

        // Persist a schema before the transaction.
        store.put_schema(&original).unwrap();

        store.begin_tx().unwrap();
        // Overwrite the schema inside the transaction.
        store
            .put_schema(&CollectionSchema {
                collection: col.clone(),
                description: Some("v2".into()),
                version: 2,
                entity_schema: None,
                link_types: Default::default(),
            })
            .unwrap();
        // Also add a schema for a second collection.
        let other = CollectionId::new("other");
        store
            .put_schema(&CollectionSchema {
                collection: other.clone(),
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
            })
            .unwrap();
        store.abort_tx().unwrap();

        // Schema for `tasks` must be restored to v1.
        let retrieved = store.get_schema(&col).unwrap().unwrap();
        assert_eq!(retrieved.version, 1, "schema should be rolled back to v1");
        assert_eq!(retrieved.description.as_deref(), Some("v1"));

        // Schema added inside the transaction must not persist.
        assert!(
            store.get_schema(&other).unwrap().is_none(),
            "schema added in aborted transaction must not persist"
        );
    }

    #[test]
    fn delete_schema_removes_it() {
        use axon_schema::schema::CollectionSchema;
        let mut store = MemoryStorageAdapter::default();
        let col = tasks();

        store
            .put_schema(&CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
            })
            .unwrap();
        assert!(store.get_schema(&col).unwrap().is_some());

        store.delete_schema(&col).unwrap();
        assert!(store.get_schema(&col).unwrap().is_none());
    }
}

// L4 conformance test suite for MemoryStorageAdapter.
crate::storage_conformance_tests!(memory_conformance, MemoryStorageAdapter::default());
