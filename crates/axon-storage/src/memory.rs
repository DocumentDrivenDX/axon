use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Bound;
use std::time::{SystemTime, UNIX_EPOCH};

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::CollectionSchema;

use crate::adapter::{
    extract_compound_key, extract_index_value, resolve_field_path, CompoundKey, IndexValue,
    StorageAdapter,
};

type CollectionMap = HashMap<EntityId, Entity>;

/// A schema version entry: (schema, created_at_ns).
type SchemaVersionEntry = (CollectionSchema, u64);

/// Key for the EAV index: (collection, field_name, indexed_value).
type IndexKey = (CollectionId, String, IndexValue);

/// Key for compound indexes: (collection, index_position, compound_key).
type CompoundIndexKey = (CollectionId, usize, CompoundKey);

/// Combined snapshot of mutable state captured at transaction start.
#[derive(Debug, Clone)]
struct TxSnapshot {
    data: HashMap<CollectionId, CollectionMap>,
    schema_versions: HashMap<CollectionId, BTreeMap<u32, SchemaVersionEntry>>,
    collections: HashSet<CollectionId>,
    /// Index snapshot for rollback.
    indexes: BTreeMap<IndexKey, BTreeSet<EntityId>>,
    /// Compound index snapshot for rollback.
    compound_indexes: BTreeMap<CompoundIndexKey, BTreeSet<EntityId>>,
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
    /// Schema version history keyed by (collection → version → schema).
    schema_versions: HashMap<CollectionId, BTreeMap<u32, SchemaVersionEntry>>,
    /// Explicitly registered collections.
    collections: HashSet<CollectionId>,
    /// Snapshot saved at `begin_tx`; `Some` means a transaction is active.
    tx_snapshot: Option<TxSnapshot>,
    /// EAV secondary index: (collection, field, value) → set of entity IDs.
    indexes: BTreeMap<IndexKey, BTreeSet<EntityId>>,
    /// Compound indexes: (collection, index_position, compound_key) → set of entity IDs.
    compound_indexes: BTreeMap<CompoundIndexKey, BTreeSet<EntityId>>,
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
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
            schema_versions: self.schema_versions.clone(),
            collections: self.collections.clone(),
            indexes: self.indexes.clone(),
            compound_indexes: self.compound_indexes.clone(),
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
            self.schema_versions = snapshot.schema_versions;
            self.collections = snapshot.collections;
            self.indexes = snapshot.indexes;
            self.compound_indexes = snapshot.compound_indexes;
        }
        Ok(())
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let versions = self
            .schema_versions
            .entry(schema.collection.clone())
            .or_default();
        let next_version = versions.keys().last().map_or(1, |v| v + 1);
        let mut versioned = schema.clone();
        versioned.version = next_version;
        versions.insert(next_version, (versioned, now_ns()));
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        Ok(self
            .schema_versions
            .get(collection)
            .and_then(|versions| versions.values().last())
            .map(|(schema, _)| schema.clone()))
    }

    fn get_schema_version(
        &self,
        collection: &CollectionId,
        version: u32,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        Ok(self
            .schema_versions
            .get(collection)
            .and_then(|versions| versions.get(&version))
            .map(|(schema, _)| schema.clone()))
    }

    fn list_schema_versions(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<(u32, u64)>, AxonError> {
        Ok(self
            .schema_versions
            .get(collection)
            .map(|versions| {
                versions
                    .iter()
                    .map(|(v, (_, ts))| (*v, *ts))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.schema_versions.remove(collection);
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

    // ── Secondary index operations (FEAT-013) ───────────────────────────

    fn update_indexes(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        old_data: Option<&serde_json::Value>,
        new_data: &serde_json::Value,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        // Remove old entries.
        if let Some(old) = old_data {
            for idx in indexes {
                if let Some(val) = resolve_field_path(old, &idx.field)
                    .and_then(|v| extract_index_value(v, &idx.index_type))
                {
                    let key = (collection.clone(), idx.field.clone(), val);
                    if let Some(set) = self.indexes.get_mut(&key) {
                        set.remove(entity_id);
                        if set.is_empty() {
                            self.indexes.remove(&key);
                        }
                    }
                }
            }
        }

        // Insert new entries (and check unique constraints).
        for idx in indexes {
            if let Some(val) = resolve_field_path(new_data, &idx.field)
                .and_then(|v| extract_index_value(v, &idx.index_type))
            {
                if idx.unique {
                    let key = (collection.clone(), idx.field.clone(), val.clone());
                    if let Some(existing) = self.indexes.get(&key) {
                        if existing.iter().any(|eid| eid != entity_id) {
                            return Err(AxonError::UniqueViolation {
                                field: idx.field.clone(),
                                value: val.to_string(),
                            });
                        }
                    }
                }
                let key = (collection.clone(), idx.field.clone(), val);
                self.indexes
                    .entry(key)
                    .or_default()
                    .insert(entity_id.clone());
            }
        }
        Ok(())
    }

    fn remove_index_entries(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        data: &serde_json::Value,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        for idx in indexes {
            if let Some(val) = resolve_field_path(data, &idx.field)
                .and_then(|v| extract_index_value(v, &idx.index_type))
            {
                let key = (collection.clone(), idx.field.clone(), val);
                if let Some(set) = self.indexes.get_mut(&key) {
                    set.remove(entity_id);
                    if set.is_empty() {
                        self.indexes.remove(&key);
                    }
                }
            }
        }
        Ok(())
    }

    fn index_lookup(
        &self,
        collection: &CollectionId,
        field: &str,
        value: &IndexValue,
    ) -> Result<Vec<EntityId>, AxonError> {
        let key = (collection.clone(), field.to_string(), value.clone());
        Ok(self
            .indexes
            .get(&key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default())
    }

    fn index_range(
        &self,
        collection: &CollectionId,
        field: &str,
        lower: Bound<&IndexValue>,
        upper: Bound<&IndexValue>,
    ) -> Result<Vec<EntityId>, AxonError> {
        // Build range bounds for the BTreeMap key.
        let lower_key = match lower {
            Bound::Included(v) => {
                Bound::Included((collection.clone(), field.to_string(), v.clone()))
            }
            Bound::Excluded(v) => {
                Bound::Excluded((collection.clone(), field.to_string(), v.clone()))
            }
            Bound::Unbounded => {
                // Start from (collection, field, min-possible-value).
                // We use Included with a synthetic minimum key.
                Bound::Included((
                    collection.clone(),
                    field.to_string(),
                    IndexValue::Boolean(false),
                ))
            }
        };
        let upper_key = match upper {
            Bound::Included(v) => {
                Bound::Included((collection.clone(), field.to_string(), v.clone()))
            }
            Bound::Excluded(v) => {
                Bound::Excluded((collection.clone(), field.to_string(), v.clone()))
            }
            Bound::Unbounded => {
                // We need a key that is strictly after all values for this (collection, field).
                // Since field is a String, we append a char that sorts after all values.
                let mut upper_field = field.to_string();
                upper_field.push('\x7f');
                Bound::Excluded((
                    collection.clone(),
                    upper_field,
                    IndexValue::Boolean(false),
                ))
            }
        };

        let mut result = Vec::new();
        for ((_col, f, _val), ids) in self.indexes.range((lower_key, upper_key)) {
            if f != field {
                continue;
            }
            result.extend(ids.iter().cloned());
        }
        Ok(result)
    }

    fn index_unique_conflict(
        &self,
        collection: &CollectionId,
        field: &str,
        value: &IndexValue,
        exclude_entity: &EntityId,
    ) -> Result<bool, AxonError> {
        let key = (collection.clone(), field.to_string(), value.clone());
        Ok(self
            .indexes
            .get(&key)
            .map(|set| set.iter().any(|eid| eid != exclude_entity))
            .unwrap_or(false))
    }

    fn drop_indexes(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.indexes
            .retain(|(col, _, _), _| col != collection);
        self.compound_indexes
            .retain(|(col, _, _), _| col != collection);
        Ok(())
    }

    // ── Compound index operations (US-033) ──────────────────────────────

    fn update_compound_indexes(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        old_data: Option<&serde_json::Value>,
        new_data: &serde_json::Value,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        // Remove old entries.
        if let Some(old) = old_data {
            for (idx_pos, idx) in indexes.iter().enumerate() {
                if let Some(key) = extract_compound_key(old, &idx.fields) {
                    let ckey = (collection.clone(), idx_pos, key);
                    if let Some(set) = self.compound_indexes.get_mut(&ckey) {
                        set.remove(entity_id);
                        if set.is_empty() {
                            self.compound_indexes.remove(&ckey);
                        }
                    }
                }
            }
        }

        // Insert new entries.
        for (idx_pos, idx) in indexes.iter().enumerate() {
            if let Some(key) = extract_compound_key(new_data, &idx.fields) {
                if idx.unique {
                    let ckey = (collection.clone(), idx_pos, key.clone());
                    if let Some(existing) = self.compound_indexes.get(&ckey) {
                        if existing.iter().any(|eid| eid != entity_id) {
                            let field_names: Vec<&str> =
                                idx.fields.iter().map(|f| f.field.as_str()).collect();
                            return Err(AxonError::UniqueViolation {
                                field: field_names.join(", "),
                                value: format!("{key:?}"),
                            });
                        }
                    }
                }
                let ckey = (collection.clone(), idx_pos, key);
                self.compound_indexes
                    .entry(ckey)
                    .or_default()
                    .insert(entity_id.clone());
            }
        }
        Ok(())
    }

    fn remove_compound_index_entries(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        data: &serde_json::Value,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        for (idx_pos, idx) in indexes.iter().enumerate() {
            if let Some(key) = extract_compound_key(data, &idx.fields) {
                let ckey = (collection.clone(), idx_pos, key);
                if let Some(set) = self.compound_indexes.get_mut(&ckey) {
                    set.remove(entity_id);
                    if set.is_empty() {
                        self.compound_indexes.remove(&ckey);
                    }
                }
            }
        }
        Ok(())
    }

    fn compound_index_lookup(
        &self,
        collection: &CollectionId,
        index_idx: usize,
        key: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        let ckey = (collection.clone(), index_idx, key.clone());
        Ok(self
            .compound_indexes
            .get(&ckey)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default())
    }

    fn compound_index_prefix(
        &self,
        collection: &CollectionId,
        index_idx: usize,
        prefix: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        let mut result = Vec::new();
        // Range from prefix..(prefix with all possible suffixes).
        let start = (collection.clone(), index_idx, prefix.clone());
        for ((col, idx, key), ids) in self.compound_indexes.range(start..) {
            if col != collection || *idx != index_idx {
                break;
            }
            // Check if this key starts with the prefix.
            if key.0.len() < prefix.0.len() {
                break;
            }
            if key.0[..prefix.0.len()] != prefix.0[..] {
                break;
            }
            result.extend(ids.iter().cloned());
        }
        Ok(result)
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
            version: 99, // ignored — auto-increment assigns v1
            entity_schema: None,
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };

        store.put_schema(&schema).unwrap();
        let retrieved = store.get_schema(&col).unwrap().unwrap();
        assert_eq!(retrieved.version, 1); // auto-incremented
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            })
            .unwrap();
        store
            .put_schema(&CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 2,
                entity_schema: None,
                link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
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
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            })
            .unwrap();
        assert!(store.get_schema(&col).unwrap().is_some());

        store.delete_schema(&col).unwrap();
        assert!(store.get_schema(&col).unwrap().is_none());
    }

    // ── Secondary index tests (FEAT-013, US-031) ────────────────────────

    mod index_tests {
        use super::*;
        use crate::adapter::{extract_index_value, IndexValue};
        use axon_schema::schema::{IndexDef, IndexType};

        fn status_index() -> IndexDef {
            IndexDef {
                field: "status".into(),
                index_type: IndexType::String,
                unique: false,
            }
        }

        fn priority_index() -> IndexDef {
            IndexDef {
                field: "priority".into(),
                index_type: IndexType::Integer,
                unique: false,
            }
        }

        fn unique_email_index() -> IndexDef {
            IndexDef {
                field: "email".into(),
                index_type: IndexType::String,
                unique: true,
            }
        }

        fn task_with_status(id: &str, status: &str) -> Entity {
            Entity::new(
                tasks(),
                EntityId::new(id),
                json!({"title": id, "status": status}),
            )
        }

        fn task_with_priority(id: &str, priority: i64) -> Entity {
            Entity::new(
                tasks(),
                EntityId::new(id),
                json!({"title": id, "priority": priority}),
            )
        }

        #[test]
        fn update_indexes_populates_equality_lookup() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let eid = EntityId::new("t-001");
            let data = json!({"status": "pending"});
            let indexes = vec![status_index()];

            store
                .update_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .unwrap();
            assert_eq!(results, vec![EntityId::new("t-001")]);
        }

        #[test]
        fn update_indexes_removes_old_entries() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let eid = EntityId::new("t-001");
            let old_data = json!({"status": "pending"});
            let new_data = json!({"status": "done"});
            let indexes = vec![status_index()];

            store
                .update_indexes(&col, &eid, None, &old_data, &indexes)
                .unwrap();
            store
                .update_indexes(&col, &eid, Some(&old_data), &new_data, &indexes)
                .unwrap();

            // Old value should be gone.
            let old_results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .unwrap();
            assert!(old_results.is_empty());

            // New value should be present.
            let new_results = store
                .index_lookup(&col, "status", &IndexValue::String("done".into()))
                .unwrap();
            assert_eq!(new_results, vec![EntityId::new("t-001")]);
        }

        #[test]
        fn remove_index_entries_cleans_up() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let eid = EntityId::new("t-001");
            let data = json!({"status": "pending"});
            let indexes = vec![status_index()];

            store
                .update_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();
            store
                .remove_index_entries(&col, &eid, &data, &indexes)
                .unwrap();

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .unwrap();
            assert!(results.is_empty());
        }

        #[test]
        fn index_range_returns_matching_entities() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![priority_index()];

            for i in 1..=5 {
                let eid = EntityId::new(format!("t-{i:03}"));
                let data = json!({"priority": i});
                store
                    .update_indexes(&col, &eid, None, &data, &indexes)
                    .unwrap();
            }

            // Range: priority > 2 (i.e., 3, 4, 5)
            let results = store
                .index_range(
                    &col,
                    "priority",
                    std::ops::Bound::Excluded(&IndexValue::Integer(2)),
                    std::ops::Bound::Unbounded,
                )
                .unwrap();
            assert_eq!(results.len(), 3);
        }

        #[test]
        fn unique_index_rejects_duplicate() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![unique_email_index()];

            let eid1 = EntityId::new("u-001");
            let data1 = json!({"email": "alice@example.com"});
            store
                .update_indexes(&col, &eid1, None, &data1, &indexes)
                .unwrap();

            let eid2 = EntityId::new("u-002");
            let data2 = json!({"email": "alice@example.com"});
            let err = store
                .update_indexes(&col, &eid2, None, &data2, &indexes)
                .unwrap_err();
            assert!(
                matches!(err, AxonError::UniqueViolation { .. }),
                "expected UniqueViolation, got: {err}"
            );
        }

        #[test]
        fn unique_index_allows_same_entity_update() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![unique_email_index()];

            let eid = EntityId::new("u-001");
            let data = json!({"email": "alice@example.com"});
            store
                .update_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();

            // Updating same entity with same value should succeed.
            let new_data = json!({"email": "alice@example.com", "name": "Alice"});
            store
                .update_indexes(&col, &eid, Some(&data), &new_data, &indexes)
                .unwrap();
        }

        #[test]
        fn null_values_are_not_indexed() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let eid = EntityId::new("t-001");
            let data = json!({"title": "no status"});
            let indexes = vec![status_index()];

            store
                .update_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();

            // No entries should exist for missing fields.
            let results = store
                .index_lookup(&col, "status", &IndexValue::String("".into()))
                .unwrap();
            assert!(results.is_empty());
        }

        #[test]
        fn drop_indexes_removes_all_entries() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![status_index()];

            let eid = EntityId::new("t-001");
            let data = json!({"status": "pending"});
            store
                .update_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();

            store.drop_indexes(&col).unwrap();

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .unwrap();
            assert!(results.is_empty());
        }

        #[test]
        fn index_unique_conflict_check() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![unique_email_index()];

            let eid1 = EntityId::new("u-001");
            let data1 = json!({"email": "alice@example.com"});
            store
                .update_indexes(&col, &eid1, None, &data1, &indexes)
                .unwrap();

            let conflict = store
                .index_unique_conflict(
                    &col,
                    "email",
                    &IndexValue::String("alice@example.com".into()),
                    &EntityId::new("u-002"),
                )
                .unwrap();
            assert!(conflict, "should detect conflict for different entity");

            let no_conflict = store
                .index_unique_conflict(
                    &col,
                    "email",
                    &IndexValue::String("alice@example.com".into()),
                    &eid1,
                )
                .unwrap();
            assert!(
                !no_conflict,
                "should not conflict when excluding the owning entity"
            );
        }

        #[test]
        fn abort_tx_rolls_back_index_changes() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![status_index()];

            let eid = EntityId::new("t-001");
            let data = json!({"status": "pending"});
            store
                .update_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();

            store.begin_tx().unwrap();
            let new_data = json!({"status": "done"});
            store
                .update_indexes(&col, &eid, Some(&data), &new_data, &indexes)
                .unwrap();
            store.abort_tx().unwrap();

            // Index should still have the old value.
            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .unwrap();
            assert_eq!(results, vec![EntityId::new("t-001")]);

            let done_results = store
                .index_lookup(&col, "status", &IndexValue::String("done".into()))
                .unwrap();
            assert!(done_results.is_empty());
        }

        #[test]
        fn nested_field_path_indexing() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let idx = IndexDef {
                field: "address.city".into(),
                index_type: IndexType::String,
                unique: false,
            };

            let eid = EntityId::new("t-001");
            let data = json!({"address": {"city": "NYC"}});
            store
                .update_indexes(&col, &eid, None, &data, &[idx])
                .unwrap();

            let results = store
                .index_lookup(&col, "address.city", &IndexValue::String("NYC".into()))
                .unwrap();
            assert_eq!(results, vec![EntityId::new("t-001")]);
        }

        #[test]
        fn extract_index_value_type_mismatch_returns_none() {
            // String value for integer index — should not be indexed.
            let val = json!("not a number");
            assert!(extract_index_value(&val, &IndexType::Integer).is_none());

            // Integer value for string index — should not be indexed.
            let val = json!(42);
            assert!(extract_index_value(&val, &IndexType::String).is_none());
        }

        #[test]
        fn multiple_entities_same_non_unique_value() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![status_index()];

            for i in 1..=3 {
                let eid = EntityId::new(format!("t-{i:03}"));
                let data = json!({"status": "pending"});
                store
                    .update_indexes(&col, &eid, None, &data, &indexes)
                    .unwrap();
            }

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .unwrap();
            assert_eq!(results.len(), 3);
        }
    }

    mod compound_index_tests {
        use super::*;
        use crate::adapter::CompoundKey;
        use axon_schema::schema::{CompoundIndexDef, CompoundIndexField, IndexType};

        fn status_priority_index() -> CompoundIndexDef {
            CompoundIndexDef {
                fields: vec![
                    CompoundIndexField {
                        field: "status".into(),
                        index_type: IndexType::String,
                    },
                    CompoundIndexField {
                        field: "priority".into(),
                        index_type: IndexType::Integer,
                    },
                ],
                unique: false,
            }
        }

        #[test]
        fn compound_index_lookup_exact_match() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![status_priority_index()];

            let eid = EntityId::new("t-001");
            let data = json!({"status": "pending", "priority": 1});
            store
                .update_compound_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();

            let key = CompoundKey(vec![
                IndexValue::String("pending".into()),
                IndexValue::Integer(1),
            ]);
            let results = store.compound_index_lookup(&col, 0, &key).unwrap();
            assert_eq!(results, vec![EntityId::new("t-001")]);
        }

        #[test]
        fn compound_index_prefix_match() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![status_priority_index()];

            for (id, status, priority) in &[
                ("t-001", "pending", 1),
                ("t-002", "pending", 2),
                ("t-003", "done", 1),
            ] {
                let eid = EntityId::new(*id);
                let data = json!({"status": status, "priority": priority});
                store
                    .update_compound_indexes(&col, &eid, None, &data, &indexes)
                    .unwrap();
            }

            // Prefix match on status=pending only.
            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            let results = store.compound_index_prefix(&col, 0, &prefix).unwrap();
            assert_eq!(results.len(), 2, "should match t-001 and t-002");
        }

        #[test]
        fn compound_index_removes_old_entries_on_update() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![status_priority_index()];

            let eid = EntityId::new("t-001");
            let old_data = json!({"status": "pending", "priority": 1});
            let new_data = json!({"status": "done", "priority": 1});

            store
                .update_compound_indexes(&col, &eid, None, &old_data, &indexes)
                .unwrap();
            store
                .update_compound_indexes(&col, &eid, Some(&old_data), &new_data, &indexes)
                .unwrap();

            // Old entry should be gone.
            let old_key = CompoundKey(vec![
                IndexValue::String("pending".into()),
                IndexValue::Integer(1),
            ]);
            let old_results = store.compound_index_lookup(&col, 0, &old_key).unwrap();
            assert!(old_results.is_empty());

            // New entry should exist.
            let new_key = CompoundKey(vec![
                IndexValue::String("done".into()),
                IndexValue::Integer(1),
            ]);
            let new_results = store.compound_index_lookup(&col, 0, &new_key).unwrap();
            assert_eq!(new_results, vec![EntityId::new("t-001")]);
        }

        #[test]
        fn compound_unique_index_rejects_duplicate() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![CompoundIndexDef {
                fields: vec![
                    CompoundIndexField {
                        field: "status".into(),
                        index_type: IndexType::String,
                    },
                    CompoundIndexField {
                        field: "priority".into(),
                        index_type: IndexType::Integer,
                    },
                ],
                unique: true,
            }];

            let eid1 = EntityId::new("t-001");
            let data1 = json!({"status": "pending", "priority": 1});
            store
                .update_compound_indexes(&col, &eid1, None, &data1, &indexes)
                .unwrap();

            let eid2 = EntityId::new("t-002");
            let data2 = json!({"status": "pending", "priority": 1});
            let err = store
                .update_compound_indexes(&col, &eid2, None, &data2, &indexes)
                .unwrap_err();
            assert!(matches!(err, AxonError::UniqueViolation { .. }));
        }

        #[test]
        fn compound_index_missing_field_not_indexed() {
            let mut store = MemoryStorageAdapter::default();
            let col = tasks();
            let indexes = vec![status_priority_index()];

            // Entity missing priority field — should not be indexed.
            let eid = EntityId::new("t-001");
            let data = json!({"status": "pending"});
            store
                .update_compound_indexes(&col, &eid, None, &data, &indexes)
                .unwrap();

            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            let results = store.compound_index_prefix(&col, 0, &prefix).unwrap();
            assert!(results.is_empty());
        }
    }
}

// L4 conformance test suite for MemoryStorageAdapter.
crate::storage_conformance_tests!(memory_conformance, MemoryStorageAdapter::default());
