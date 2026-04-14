use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Bound;
use std::time::{SystemTime, UNIX_EPOCH};

use axon_core::error::AxonError;
use axon_core::id::{
    CollectionId, EntityId, Namespace, QualifiedCollectionId, DEFAULT_DATABASE, DEFAULT_SCHEMA,
};
use axon_core::types::{Entity, Link};
use axon_schema::schema::{CollectionSchema, CollectionView};

use crate::adapter::{
    extract_compound_key, extract_index_value, resolve_field_path, CompoundKey, IndexValue,
    StorageAdapter,
};

type CollectionMap = HashMap<EntityId, Entity>;
type CatalogKey = QualifiedCollectionId;

/// A schema version entry: (schema, created_at_ns).
type SchemaVersionEntry = (CollectionSchema, u64);
/// A collection view version entry: (view, updated_at_ns).
type CollectionViewEntry = (CollectionView, u64);

/// Key for the EAV index: (qualified collection, field_name, indexed_value).
type IndexKey = (CatalogKey, String, IndexValue);

/// Key for compound indexes: (qualified collection, index_position, compound_key).
type CompoundIndexKey = (CatalogKey, usize, CompoundKey);

/// Bidirectional mapping between collection names and numeric IDs (ADR-010).
#[derive(Debug, Clone, Default)]
struct NumericIdCache {
    name_to_id: HashMap<CatalogKey, u64>,
    id_to_name: HashMap<u64, CatalogKey>,
    next_id: u64,
}

impl NumericIdCache {
    fn assign(&mut self, collection: &CatalogKey) -> u64 {
        if let Some(&id) = self.name_to_id.get(collection) {
            return id;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.name_to_id.insert(collection.clone(), id);
        self.id_to_name.insert(id, collection.clone());
        id
    }

    fn remove(&mut self, collection: &CatalogKey) {
        if let Some(id) = self.name_to_id.remove(collection) {
            self.id_to_name.remove(&id);
        }
    }
}

/// Key for the dedicated link store: (source_col, source_id, link_type, target_col, target_id).
type LinkKey = (CollectionId, EntityId, String, CollectionId, EntityId);

/// Combined snapshot of mutable state captured at transaction start.
#[derive(Debug, Clone)]
struct TxSnapshot {
    data: HashMap<CatalogKey, CollectionMap>,
    schema_versions: HashMap<CatalogKey, BTreeMap<u32, SchemaVersionEntry>>,
    collection_views: HashMap<CatalogKey, CollectionViewEntry>,
    collections: HashSet<CatalogKey>,
    databases: BTreeSet<String>,
    namespaces: BTreeMap<String, BTreeSet<String>>,
    /// Index snapshot for rollback.
    indexes: BTreeMap<IndexKey, BTreeSet<EntityId>>,
    /// Compound index snapshot for rollback.
    compound_indexes: BTreeMap<CompoundIndexKey, BTreeSet<EntityId>>,
    /// Numeric ID cache snapshot for rollback.
    numeric_ids: NumericIdCache,
    /// Dedicated link store snapshot.
    links: BTreeMap<LinkKey, Link>,
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
#[derive(Debug)]
pub struct MemoryStorageAdapter {
    data: HashMap<CatalogKey, CollectionMap>,
    /// Schema version history keyed by (collection → version → schema).
    schema_versions: HashMap<CatalogKey, BTreeMap<u32, SchemaVersionEntry>>,
    /// Latest collection view keyed by collection.
    collection_views: HashMap<CatalogKey, CollectionViewEntry>,
    /// Explicitly registered collections.
    collections: HashSet<CatalogKey>,
    /// Known database names.
    databases: BTreeSet<String>,
    /// Database -> schema names.
    namespaces: BTreeMap<String, BTreeSet<String>>,
    /// Snapshot saved at `begin_tx`; `Some` means a transaction is active.
    tx_snapshot: Option<TxSnapshot>,
    /// EAV secondary index: (collection, field, value) → set of entity IDs.
    indexes: BTreeMap<IndexKey, BTreeSet<EntityId>>,
    /// Compound indexes: (collection, index_position, compound_key) → set of entity IDs.
    compound_indexes: BTreeMap<CompoundIndexKey, BTreeSet<EntityId>>,
    /// Bidirectional name-to-numeric-ID cache (ADR-010).
    numeric_ids: NumericIdCache,
    /// Dedicated link store (ADR-010): replaces __axon_links__ pseudo-collection
    /// for new code paths. Keyed by (source_col, source_id, link_type, target_col, target_id).
    links: BTreeMap<LinkKey, Link>,
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn unregistered_collection_error(collection: &CollectionId) -> AxonError {
    AxonError::InvalidArgument(format!(
        "collection '{}' is not registered",
        collection.as_str()
    ))
}

impl Default for MemoryStorageAdapter {
    fn default() -> Self {
        let mut databases = BTreeSet::new();
        databases.insert(DEFAULT_DATABASE.to_string());

        let mut namespaces = BTreeMap::new();
        namespaces.insert(
            DEFAULT_DATABASE.to_string(),
            BTreeSet::from([DEFAULT_SCHEMA.to_string()]),
        );

        Self {
            data: HashMap::new(),
            schema_versions: HashMap::new(),
            collection_views: HashMap::new(),
            collections: HashSet::new(),
            databases,
            namespaces,
            tx_snapshot: None,
            indexes: BTreeMap::new(),
            compound_indexes: BTreeMap::new(),
            numeric_ids: NumericIdCache::default(),
            links: BTreeMap::new(),
        }
    }
}

impl MemoryStorageAdapter {
    fn catalog_key(namespace: &Namespace, collection: &CollectionId) -> CatalogKey {
        CatalogKey::from_parts(namespace, collection)
    }

    fn registered_catalog_keys(&self, collection: &CollectionId) -> Vec<CatalogKey> {
        let mut keys: Vec<_> = self
            .collections
            .iter()
            .filter(|key| key.collection == *collection)
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn resolve_catalog_key(&self, collection: &CollectionId) -> Result<CatalogKey, AxonError> {
        let (namespace, bare_collection) = Namespace::parse(collection.as_str());
        if bare_collection != collection.as_str() {
            return Ok(Self::catalog_key(
                &namespace,
                &CollectionId::new(bare_collection),
            ));
        }

        let keys = self.registered_catalog_keys(collection);
        match keys.as_slice() {
            [] => Ok(Self::catalog_key(&Namespace::default_ns(), collection)),
            [key] => Ok(key.clone()),
            _ => {
                let default_key = Self::catalog_key(&Namespace::default_ns(), collection);
                if keys.contains(&default_key) {
                    Ok(default_key)
                } else {
                    Err(AxonError::InvalidArgument(format!(
                        "collection '{}' exists in multiple namespaces; qualify the namespace",
                        collection.as_str()
                    )))
                }
            }
        }
    }

    fn remove_catalog_key(&mut self, key: &CatalogKey) {
        self.collections.remove(key);
        self.schema_versions.remove(key);
        self.collection_views.remove(key);
        self.numeric_ids.remove(key);
        self.indexes
            .retain(|(collection, _, _), _| collection != key);
        self.compound_indexes
            .retain(|(collection, _, _), _| collection != key);
    }

    fn purge_collection_entities(&mut self, collection: &CatalogKey) {
        self.data.remove(collection);
    }

    fn purge_doomed_collection_entities(&mut self, doomed: &[CatalogKey]) {
        for collection in doomed {
            self.purge_collection_entities(collection);
        }
    }

    fn namespace_collection_keys(&self, namespace: &Namespace) -> Vec<CatalogKey> {
        self.collections
            .iter()
            .filter(|key| key.namespace == *namespace)
            .cloned()
            .collect()
    }

    fn database_collection_keys(&self, database: &str) -> Vec<CatalogKey> {
        self.collections
            .iter()
            .filter(|key| key.namespace.database == database)
            .cloned()
            .collect()
    }
}

impl StorageAdapter for MemoryStorageAdapter {
    fn resolve_collection_key(
        &self,
        collection: &CollectionId,
    ) -> Result<QualifiedCollectionId, AxonError> {
        self.resolve_catalog_key(collection)
    }

    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        Ok(self.data.get(&key).and_then(|col| col.get(id)).cloned())
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let mut stored = entity;
        stored.collection = key.collection.clone();
        self.data
            .entry(key)
            .or_default()
            .insert(stored.id.clone(), stored);
        Ok(())
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        if let Some(col) = self.data.get_mut(&key) {
            col.remove(id);
        }
        Ok(())
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        Ok(self.data.get(&key).map_or(0, |col| col.len()))
    }

    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        let Some(col) = self.data.get(&key) else {
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
        let key = self.resolve_catalog_key(&entity.collection)?;
        let current = self
            .data
            .get(&key)
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
            collection: key.collection.clone(),
            version: expected_version + 1,
            ..entity
        };

        self.data
            .entry(key)
            .or_default()
            .insert(updated.id.clone(), updated.clone());

        Ok(updated)
    }

    fn create_if_absent(
        &mut self,
        entity: Entity,
        expected_absent_version: u64,
    ) -> Result<Entity, AxonError> {
        let key = self.resolve_catalog_key(&entity.collection)?;
        let current = self
            .data
            .get(&key)
            .and_then(|col| col.get(&entity.id))
            .cloned();

        if let Some(current) = current {
            return Err(AxonError::ConflictingVersion {
                expected: expected_absent_version,
                actual: current.version,
                current_entity: Some(Box::new(current)),
            });
        }

        let inserted = Entity {
            collection: key.collection.clone(),
            ..entity
        };
        self.data
            .entry(key)
            .or_default()
            .insert(inserted.id.clone(), inserted.clone());
        Ok(inserted)
    }

    fn begin_tx(&mut self) -> Result<(), AxonError> {
        if self.tx_snapshot.is_some() {
            return Err(AxonError::Storage("transaction already active".into()));
        }
        self.tx_snapshot = Some(TxSnapshot {
            data: self.data.clone(),
            schema_versions: self.schema_versions.clone(),
            collection_views: self.collection_views.clone(),
            collections: self.collections.clone(),
            databases: self.databases.clone(),
            namespaces: self.namespaces.clone(),
            indexes: self.indexes.clone(),
            compound_indexes: self.compound_indexes.clone(),
            numeric_ids: self.numeric_ids.clone(),
            links: self.links.clone(),
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
            self.collection_views = snapshot.collection_views;
            self.collections = snapshot.collections;
            self.databases = snapshot.databases;
            self.namespaces = snapshot.namespaces;
            self.indexes = snapshot.indexes;
            self.compound_indexes = snapshot.compound_indexes;
            self.numeric_ids = snapshot.numeric_ids;
            self.links = snapshot.links;
        }
        Ok(())
    }

    fn create_database(&mut self, name: &str) -> Result<(), AxonError> {
        if !self.databases.insert(name.to_string()) {
            return Err(AxonError::AlreadyExists(format!("database '{name}'")));
        }
        self.namespaces.insert(
            name.to_string(),
            BTreeSet::from([DEFAULT_SCHEMA.to_string()]),
        );
        Ok(())
    }

    fn list_databases(&self) -> Result<Vec<String>, AxonError> {
        Ok(self.databases.iter().cloned().collect())
    }

    fn drop_database(&mut self, name: &str) -> Result<(), AxonError> {
        if !self.databases.remove(name) {
            return Err(AxonError::NotFound(format!("database '{name}'")));
        }

        self.namespaces.remove(name);
        let doomed = self.database_collection_keys(name);
        self.purge_links_for_collections(&doomed)?;
        self.purge_doomed_collection_entities(&doomed);
        for key in doomed {
            self.remove_catalog_key(&key);
        }
        Ok(())
    }

    fn create_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        let schemas = self
            .namespaces
            .get_mut(&namespace.database)
            .ok_or_else(|| AxonError::NotFound(format!("database '{}'", namespace.database)))?;

        if !schemas.insert(namespace.schema.clone()) {
            return Err(AxonError::AlreadyExists(format!("namespace '{namespace}'")));
        }
        Ok(())
    }

    fn list_namespaces(&self, database: &str) -> Result<Vec<String>, AxonError> {
        match self.namespaces.get(database) {
            Some(schemas) => Ok(schemas.iter().cloned().collect()),
            None => Err(AxonError::NotFound(format!("database '{database}'"))),
        }
    }

    fn drop_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        let schemas = self
            .namespaces
            .get_mut(&namespace.database)
            .ok_or_else(|| AxonError::NotFound(format!("database '{}'", namespace.database)))?;

        if !schemas.remove(&namespace.schema) {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let doomed = self.namespace_collection_keys(namespace);
        self.purge_links_for_collections(&doomed)?;
        self.purge_doomed_collection_entities(&doomed);
        for key in doomed {
            self.remove_catalog_key(&key);
        }
        Ok(())
    }

    fn list_namespace_collections(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CollectionId>, AxonError> {
        let schemas = self
            .namespaces
            .get(&namespace.database)
            .ok_or_else(|| AxonError::NotFound(format!("database '{}'", namespace.database)))?;
        if !schemas.contains(&namespace.schema) {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let mut collections: Vec<CollectionId> = self
            .collections
            .iter()
            .filter(|key| key.namespace == *namespace)
            .map(|key| key.collection.clone())
            .collect();
        collections.sort();
        Ok(collections)
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(&schema.collection)?;
        let versions = self.schema_versions.entry(key).or_default();
        let next_version = versions.keys().last().map_or(1, |v| v + 1);
        let mut versioned = schema.clone();
        versioned.collection = schema
            .collection
            .as_str()
            .split('.')
            .next_back()
            .map(CollectionId::new)
            .unwrap_or_else(|| schema.collection.clone());
        versioned.version = next_version;
        versions.insert(next_version, (versioned, now_ns()));
        Ok(())
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        Ok(self
            .schema_versions
            .get(&key)
            .and_then(|versions| versions.values().last())
            .map(|(schema, _)| schema.clone()))
    }

    fn get_schema_version(
        &self,
        collection: &CollectionId,
        version: u32,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        Ok(self
            .schema_versions
            .get(&key)
            .and_then(|versions| versions.get(&version))
            .map(|(schema, _)| schema.clone()))
    }

    fn list_schema_versions(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<(u32, u64)>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        Ok(self
            .schema_versions
            .get(&key)
            .map(|versions| versions.iter().map(|(v, (_, ts))| (*v, *ts)).collect())
            .unwrap_or_default())
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.schema_versions.remove(&key);
        Ok(())
    }

    fn put_collection_view(&mut self, view: &CollectionView) -> Result<CollectionView, AxonError> {
        let key = self.resolve_catalog_key(&view.collection)?;
        if !self.collections.contains(&key) {
            return Err(unregistered_collection_error(&view.collection));
        }

        let next_version = self
            .collection_views
            .get(&key)
            .map_or(1, |(existing, _)| existing.version + 1);
        let mut versioned = view.clone();
        let updated_at_ns = now_ns();
        versioned.version = next_version;
        versioned.updated_at_ns = Some(updated_at_ns);
        versioned.collection = key.collection.clone();
        self.collection_views
            .insert(key, (versioned.clone(), updated_at_ns));
        Ok(versioned)
    }

    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        Ok(self
            .collection_views
            .get(&key)
            .map(|(view, _)| view.clone()))
    }

    fn delete_collection_view(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.collection_views.remove(&key);
        Ok(())
    }

    fn register_collection_in_namespace(
        &mut self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<(), AxonError> {
        let schemas = self
            .namespaces
            .get(&namespace.database)
            .ok_or_else(|| AxonError::NotFound(format!("database '{}'", namespace.database)))?;
        if !schemas.contains(&namespace.schema) {
            return Err(AxonError::NotFound(format!("namespace '{namespace}'")));
        }

        let key = Self::catalog_key(namespace, collection);
        self.collections.insert(key.clone());
        // Auto-assign a numeric ID (ADR-010).
        self.numeric_ids.assign(&key);
        Ok(())
    }

    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.remove_catalog_key(&key);
        Ok(())
    }

    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        let mut names: Vec<CollectionId> = self
            .collections
            .iter()
            .map(|key| key.collection.clone())
            .collect();
        names.sort();
        Ok(names)
    }

    fn collection_numeric_id(&self, collection: &CollectionId) -> Result<Option<u64>, AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        Ok(self.numeric_ids.name_to_id.get(&key).copied())
    }

    fn collection_by_numeric_id(&self, numeric_id: u64) -> Result<Option<CollectionId>, AxonError> {
        Ok(self
            .numeric_ids
            .id_to_name
            .get(&numeric_id)
            .map(|key| key.collection.clone()))
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
        let collection_key = self.resolve_catalog_key(collection)?;
        // Remove old entries.
        if let Some(old) = old_data {
            for idx in indexes {
                if let Some(val) = resolve_field_path(old, &idx.field)
                    .and_then(|v| extract_index_value(v, &idx.index_type))
                {
                    let key = (collection_key.clone(), idx.field.clone(), val);
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
                    let key = (collection_key.clone(), idx.field.clone(), val.clone());
                    if let Some(existing) = self.indexes.get(&key) {
                        if existing.iter().any(|eid| eid != entity_id) {
                            return Err(AxonError::UniqueViolation {
                                field: idx.field.clone(),
                                value: val.to_string(),
                            });
                        }
                    }
                }
                let key = (collection_key.clone(), idx.field.clone(), val);
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
        let collection_key = self.resolve_catalog_key(collection)?;
        for idx in indexes {
            if let Some(val) = resolve_field_path(data, &idx.field)
                .and_then(|v| extract_index_value(v, &idx.index_type))
            {
                let key = (collection_key.clone(), idx.field.clone(), val);
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
        let collection_key = self.resolve_catalog_key(collection)?;
        let key = (collection_key, field.to_string(), value.clone());
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
        let collection_key = self.resolve_catalog_key(collection)?;
        // Build range bounds for the BTreeMap key.
        let lower_key = match lower {
            Bound::Included(v) => {
                Bound::Included((collection_key.clone(), field.to_string(), v.clone()))
            }
            Bound::Excluded(v) => {
                Bound::Excluded((collection_key.clone(), field.to_string(), v.clone()))
            }
            Bound::Unbounded => {
                // Start from (collection, field, min-possible-value).
                // We use Included with a synthetic minimum key.
                Bound::Included((
                    collection_key.clone(),
                    field.to_string(),
                    IndexValue::Boolean(false),
                ))
            }
        };
        let upper_key = match upper {
            Bound::Included(v) => {
                Bound::Included((collection_key.clone(), field.to_string(), v.clone()))
            }
            Bound::Excluded(v) => {
                Bound::Excluded((collection_key.clone(), field.to_string(), v.clone()))
            }
            Bound::Unbounded => {
                // We need a key that is strictly after all values for this (collection, field).
                // Since field is a String, we append a char that sorts after all values.
                let mut upper_field = field.to_string();
                upper_field.push('\x7f');
                Bound::Excluded((collection_key, upper_field, IndexValue::Boolean(false)))
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
        let collection_key = self.resolve_catalog_key(collection)?;
        let key = (collection_key, field.to_string(), value.clone());
        Ok(self
            .indexes
            .get(&key)
            .map(|set| set.iter().any(|eid| eid != exclude_entity))
            .unwrap_or(false))
    }

    fn drop_indexes(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let key = self.resolve_catalog_key(collection)?;
        self.indexes.retain(|(col, _, _), _| col != &key);
        self.compound_indexes.retain(|(col, _, _), _| col != &key);
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
        let collection_key = self.resolve_catalog_key(collection)?;
        // Remove old entries.
        if let Some(old) = old_data {
            for (idx_pos, idx) in indexes.iter().enumerate() {
                if let Some(key) = extract_compound_key(old, &idx.fields) {
                    let ckey = (collection_key.clone(), idx_pos, key);
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
                    let ckey = (collection_key.clone(), idx_pos, key.clone());
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
                let ckey = (collection_key.clone(), idx_pos, key);
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
        let collection_key = self.resolve_catalog_key(collection)?;
        for (idx_pos, idx) in indexes.iter().enumerate() {
            if let Some(key) = extract_compound_key(data, &idx.fields) {
                let ckey = (collection_key.clone(), idx_pos, key);
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
        let collection_key = self.resolve_catalog_key(collection)?;
        let ckey = (collection_key, index_idx, key.clone());
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
        let collection_key = self.resolve_catalog_key(collection)?;
        let mut result = Vec::new();
        // Range from prefix..(prefix with all possible suffixes).
        let start = (collection_key.clone(), index_idx, prefix.clone());
        for ((col, idx, key), ids) in self.compound_indexes.range(start..) {
            if col != &collection_key || *idx != index_idx {
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

    // ── Dedicated link store (ADR-010) ──────────────────────────────────

    fn put_link(&mut self, link: &Link) -> Result<(), AxonError> {
        let key = (
            link.source_collection.clone(),
            link.source_id.clone(),
            link.link_type.clone(),
            link.target_collection.clone(),
            link.target_id.clone(),
        );
        self.links.insert(key, link.clone());
        // Also write to pseudo-collections for backward compatibility.
        self.put(link.to_entity())?;
        self.put(link.to_rev_entity())
    }

    fn delete_link(
        &mut self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_collection: &CollectionId,
        target_id: &EntityId,
    ) -> Result<(), AxonError> {
        let key = (
            source_collection.clone(),
            source_id.clone(),
            link_type.to_string(),
            target_collection.clone(),
            target_id.clone(),
        );
        self.links.remove(&key);
        // Also clean pseudo-collections for backward compatibility.
        let fwd_id = Link::storage_id(
            source_collection,
            source_id,
            link_type,
            target_collection,
            target_id,
        );
        let rev_id = Link::rev_storage_id(
            target_collection,
            target_id,
            source_collection,
            source_id,
            link_type,
        );
        self.delete(&Link::links_collection(), &fwd_id)?;
        self.delete(&Link::links_rev_collection(), &rev_id)
    }

    fn get_link(
        &self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_collection: &CollectionId,
        target_id: &EntityId,
    ) -> Result<Option<Link>, AxonError> {
        let key = (
            source_collection.clone(),
            source_id.clone(),
            link_type.to_string(),
            target_collection.clone(),
            target_id.clone(),
        );
        Ok(self.links.get(&key).cloned())
    }

    fn list_outbound_links(
        &self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: Option<&str>,
    ) -> Result<Vec<Link>, AxonError> {
        Ok(self
            .links
            .iter()
            .filter(|((sc, si, lt, _, _), _)| {
                sc == source_collection && si == source_id && link_type.map_or(true, |f| lt == f)
            })
            .map(|(_, link)| link.clone())
            .collect())
    }

    fn list_inbound_links(
        &self,
        target_collection: &CollectionId,
        target_id: &EntityId,
        link_type: Option<&str>,
    ) -> Result<Vec<Link>, AxonError> {
        Ok(self
            .links
            .iter()
            .filter(|((_, _, lt, tc, ti), _)| {
                tc == target_collection && ti == target_id && link_type.map_or(true, |f| lt == f)
            })
            .map(|(_, link)| link.clone())
            .collect())
    }

    // Gate results now live on the Entity blob itself (FEAT-019); no
    // dedicated side-table lives on the storage adapter anymore.
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::{CollectionId, EntityId, Namespace};
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
            .expect("test operation should succeed")
            .is_none());
    }

    #[test]
    fn put_then_get_roundtrip() {
        let mut store = MemoryStorageAdapter::default();
        store
            .put(entity("t-001"))
            .expect("test operation should succeed");
        let found = store
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed");
        assert!(found.is_some());
        assert_eq!(
            found.expect("test operation should succeed").data["title"],
            "t-001"
        );
    }

    #[test]
    fn two_part_collection_names_resolve_to_default_database_schema() {
        let mut store = MemoryStorageAdapter::default();
        let billing = Namespace::new("default", "billing");
        let invoices = CollectionId::new("invoices");
        let schema_qualified = CollectionId::new("billing.invoices");
        let fully_qualified = CollectionId::new("default.billing.invoices");
        let entity_id = EntityId::new("inv-001");

        store
            .create_namespace(&billing)
            .expect("billing namespace create should succeed");
        store
            .register_collection_in_namespace(&invoices, &billing)
            .expect("billing collection register should succeed");
        store
            .put(Entity::new(
                schema_qualified.clone(),
                entity_id.clone(),
                json!({"scope": "billing"}),
            ))
            .expect("two-part entity put should succeed");

        assert_eq!(
            store
                .get(&schema_qualified, &entity_id)
                .expect("two-part get should succeed")
                .expect("two-part entity should exist")
                .data["scope"],
            json!("billing")
        );
        assert_eq!(
            store
                .get(&fully_qualified, &entity_id)
                .expect("fully qualified get should succeed")
                .expect("fully qualified entity should exist")
                .data["scope"],
            json!("billing")
        );
        assert_eq!(
            store
                .count(&fully_qualified)
                .expect("fully qualified count should succeed"),
            1
        );
    }

    #[test]
    fn count_reflects_puts_and_deletes() {
        let mut store = MemoryStorageAdapter::default();
        assert_eq!(
            store
                .count(&tasks())
                .expect("test operation should succeed"),
            0
        );
        store
            .put(entity("t-001"))
            .expect("test operation should succeed");
        store
            .put(entity("t-002"))
            .expect("test operation should succeed");
        assert_eq!(
            store
                .count(&tasks())
                .expect("test operation should succeed"),
            2
        );
        store
            .delete(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed");
        assert_eq!(
            store
                .count(&tasks())
                .expect("test operation should succeed"),
            1
        );
    }

    #[test]
    fn range_scan_returns_sorted_entities() {
        let mut store = MemoryStorageAdapter::default();
        store
            .put(entity("t-003"))
            .expect("test operation should succeed");
        store
            .put(entity("t-001"))
            .expect("test operation should succeed");
        store
            .put(entity("t-002"))
            .expect("test operation should succeed");
        let results = store
            .range_scan(&tasks(), None, None, None)
            .expect("test operation should succeed");
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-001", "t-002", "t-003"]);
    }

    #[test]
    fn range_scan_respects_start_end_and_limit() {
        let mut store = MemoryStorageAdapter::default();
        for i in 1..=5 {
            store
                .put(entity(&format!("t-00{i}")))
                .expect("test operation should succeed");
        }
        let start = EntityId::new("t-002");
        let end = EntityId::new("t-004");
        let results = store
            .range_scan(&tasks(), Some(&start), Some(&end), Some(2))
            .expect("test operation should succeed");
        let ids: Vec<_> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t-002", "t-003"]);
    }

    #[test]
    fn compare_and_swap_increments_version() {
        let mut store = MemoryStorageAdapter::default();
        store
            .put(entity("t-001"))
            .expect("test operation should succeed");
        let updated = store
            .compare_and_swap(entity("t-001"), 1)
            .expect("test operation should succeed");
        assert_eq!(updated.version, 2);
        let stored = store
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .expect("test operation should succeed");
        assert_eq!(stored.version, 2);
    }

    #[test]
    fn compare_and_swap_rejects_wrong_version() {
        let mut store = MemoryStorageAdapter::default();
        store
            .put(entity("t-001"))
            .expect("test operation should succeed");
        let err = store
            .compare_and_swap(entity("t-001"), 99)
            .expect_err("test operation should fail");
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
        let err = store
            .compare_and_swap(entity("ghost"), 1)
            .expect_err("test operation should fail");
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
        store.begin_tx().expect("test operation should succeed");
        store
            .put(entity("t-001"))
            .expect("test operation should succeed");
        store.commit_tx().expect("test operation should succeed");
        assert!(store
            .get(&tasks(), &EntityId::new("t-001"))
            .expect("test operation should succeed")
            .is_some());
    }

    #[test]
    fn abort_tx_rolls_back_writes() {
        let mut store = MemoryStorageAdapter::default();
        store
            .put(entity("t-existing"))
            .expect("test operation should succeed");

        store.begin_tx().expect("test operation should succeed");
        store
            .put(entity("t-new"))
            .expect("test operation should succeed");
        store
            .delete(&tasks(), &EntityId::new("t-existing"))
            .expect("test operation should succeed");
        store.abort_tx().expect("test operation should succeed");

        // t-new must be gone, t-existing must be restored.
        assert!(store
            .get(&tasks(), &EntityId::new("t-new"))
            .expect("test operation should succeed")
            .is_none());
        assert!(store
            .get(&tasks(), &EntityId::new("t-existing"))
            .expect("test operation should succeed")
            .is_some());
    }

    #[test]
    fn begin_tx_rejects_nested_begin() {
        let mut store = MemoryStorageAdapter::default();
        store.begin_tx().expect("test operation should succeed");
        assert!(store.begin_tx().is_err());
        // Clean up.
        store.abort_tx().expect("test operation should succeed");
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
        store.abort_tx().expect("test operation should succeed");
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
        lifecycles: Default::default(),
        };

        store
            .put_schema(&schema)
            .expect("test operation should succeed");
        let retrieved = store
            .get_schema(&col)
            .expect("test operation should succeed")
            .expect("test operation should succeed");
        assert_eq!(retrieved.version, 1); // auto-incremented
        assert_eq!(retrieved.description.as_deref(), Some("my schema"));
    }

    #[test]
    fn get_schema_missing_returns_none() {
        let store = MemoryStorageAdapter::default();
        assert!(store
            .get_schema(&tasks())
            .expect("test operation should succeed")
            .is_none());
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
            lifecycles: Default::default(),
            })
            .expect("test operation should succeed");
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
            lifecycles: Default::default(),
            })
            .expect("test operation should succeed");

        assert_eq!(
            store
                .get_schema(&col)
                .expect("test operation should succeed")
                .expect("test operation should succeed")
                .version,
            2
        );
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
        lifecycles: Default::default(),
        };

        // Persist a schema before the transaction.
        store
            .put_schema(&original)
            .expect("test operation should succeed");

        store.begin_tx().expect("test operation should succeed");
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
            lifecycles: Default::default(),
            })
            .expect("test operation should succeed");
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
            lifecycles: Default::default(),
            })
            .expect("test operation should succeed");
        store.abort_tx().expect("test operation should succeed");

        // Schema for `tasks` must be restored to v1.
        let retrieved = store
            .get_schema(&col)
            .expect("test operation should succeed")
            .expect("test operation should succeed");
        assert_eq!(retrieved.version, 1, "schema should be rolled back to v1");
        assert_eq!(retrieved.description.as_deref(), Some("v1"));

        // Schema added inside the transaction must not persist.
        assert!(
            store
                .get_schema(&other)
                .expect("test operation should succeed")
                .is_none(),
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
            lifecycles: Default::default(),
            })
            .expect("test operation should succeed");
        assert!(store
            .get_schema(&col)
            .expect("test operation should succeed")
            .is_some());

        store
            .delete_schema(&col)
            .expect("test operation should succeed");
        assert!(store
            .get_schema(&col)
            .expect("test operation should succeed")
            .is_none());
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

        #[expect(dead_code, reason = "helper is kept for nearby index tests")]
        fn task_with_status(id: &str, status: &str) -> Entity {
            Entity::new(
                tasks(),
                EntityId::new(id),
                json!({"title": id, "status": status}),
            )
        }

        #[expect(dead_code, reason = "helper is kept for nearby index tests")]
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
                .expect("test operation should succeed");

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");
            store
                .update_indexes(&col, &eid, Some(&old_data), &new_data, &indexes)
                .expect("test operation should succeed");

            // Old value should be gone.
            let old_results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("test operation should succeed");
            assert!(old_results.is_empty());

            // New value should be present.
            let new_results = store
                .index_lookup(&col, "status", &IndexValue::String("done".into()))
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");
            store
                .remove_index_entries(&col, &eid, &data, &indexes)
                .expect("test operation should succeed");

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("test operation should succeed");
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
                    .expect("test operation should succeed");
            }

            // Range: priority > 2 (i.e., 3, 4, 5)
            let results = store
                .index_range(
                    &col,
                    "priority",
                    std::ops::Bound::Excluded(&IndexValue::Integer(2)),
                    std::ops::Bound::Unbounded,
                )
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            let eid2 = EntityId::new("u-002");
            let data2 = json!({"email": "alice@example.com"});
            let err = store
                .update_indexes(&col, &eid2, None, &data2, &indexes)
                .expect_err("test operation should fail");
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
                .expect("test operation should succeed");

            // Updating same entity with same value should succeed.
            let new_data = json!({"email": "alice@example.com", "name": "Alice"});
            store
                .update_indexes(&col, &eid, Some(&data), &new_data, &indexes)
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            // No entries should exist for missing fields.
            let results = store
                .index_lookup(&col, "status", &IndexValue::String(String::new()))
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            store
                .drop_indexes(&col)
                .expect("test operation should succeed");

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            let conflict = store
                .index_unique_conflict(
                    &col,
                    "email",
                    &IndexValue::String("alice@example.com".into()),
                    &EntityId::new("u-002"),
                )
                .expect("test operation should succeed");
            assert!(conflict, "should detect conflict for different entity");

            let no_conflict = store
                .index_unique_conflict(
                    &col,
                    "email",
                    &IndexValue::String("alice@example.com".into()),
                    &eid1,
                )
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            store.begin_tx().expect("test operation should succeed");
            let new_data = json!({"status": "done"});
            store
                .update_indexes(&col, &eid, Some(&data), &new_data, &indexes)
                .expect("test operation should succeed");
            store.abort_tx().expect("test operation should succeed");

            // Index should still have the old value.
            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("test operation should succeed");
            assert_eq!(results, vec![EntityId::new("t-001")]);

            let done_results = store
                .index_lookup(&col, "status", &IndexValue::String("done".into()))
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            let results = store
                .index_lookup(&col, "address.city", &IndexValue::String("NYC".into()))
                .expect("test operation should succeed");
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
                    .expect("test operation should succeed");
            }

            let results = store
                .index_lookup(&col, "status", &IndexValue::String("pending".into()))
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            let key = CompoundKey(vec![
                IndexValue::String("pending".into()),
                IndexValue::Integer(1),
            ]);
            let results = store
                .compound_index_lookup(&col, 0, &key)
                .expect("test operation should succeed");
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
                    .expect("test operation should succeed");
            }

            // Prefix match on status=pending only.
            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            let results = store
                .compound_index_prefix(&col, 0, &prefix)
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");
            store
                .update_compound_indexes(&col, &eid, Some(&old_data), &new_data, &indexes)
                .expect("test operation should succeed");

            // Old entry should be gone.
            let old_key = CompoundKey(vec![
                IndexValue::String("pending".into()),
                IndexValue::Integer(1),
            ]);
            let old_results = store
                .compound_index_lookup(&col, 0, &old_key)
                .expect("test operation should succeed");
            assert!(old_results.is_empty());

            // New entry should exist.
            let new_key = CompoundKey(vec![
                IndexValue::String("done".into()),
                IndexValue::Integer(1),
            ]);
            let new_results = store
                .compound_index_lookup(&col, 0, &new_key)
                .expect("test operation should succeed");
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
                .expect("test operation should succeed");

            let eid2 = EntityId::new("t-002");
            let data2 = json!({"status": "pending", "priority": 1});
            let err = store
                .update_compound_indexes(&col, &eid2, None, &data2, &indexes)
                .expect_err("test operation should fail");
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
                .expect("test operation should succeed");

            let prefix = CompoundKey(vec![IndexValue::String("pending".into())]);
            let results = store
                .compound_index_prefix(&col, 0, &prefix)
                .expect("test operation should succeed");
            assert!(results.is_empty());
        }
    }

    // ── Numeric collection ID tests (ADR-010) ──────────────────────────

    mod numeric_collection_ids {
        use super::*;

        #[test]
        fn register_assigns_numeric_id() {
            let mut store = MemoryStorageAdapter::default();
            store
                .register_collection(&tasks())
                .expect("test operation should succeed");

            let nid = store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed");
            assert!(
                nid.is_some(),
                "registered collection should have numeric id"
            );
            assert!(
                nid.expect("test operation should succeed") > 0,
                "numeric id should be positive"
            );
        }

        #[test]
        fn numeric_id_is_stable_on_re_register() {
            let mut store = MemoryStorageAdapter::default();
            store
                .register_collection(&tasks())
                .expect("test operation should succeed");
            let first = store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .expect("test operation should succeed");

            // Re-register should not change the ID.
            store
                .register_collection(&tasks())
                .expect("test operation should succeed");
            let second = store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .expect("test operation should succeed");
            assert_eq!(first, second);
        }

        #[test]
        fn different_collections_get_different_ids() {
            let mut store = MemoryStorageAdapter::default();
            store
                .register_collection(&tasks())
                .expect("test operation should succeed");
            store
                .register_collection(&CollectionId::new("users"))
                .expect("test operation should succeed");

            let tasks_id = store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .expect("test operation should succeed");
            let users_id = store
                .collection_numeric_id(&CollectionId::new("users"))
                .expect("test operation should succeed")
                .expect("test operation should succeed");
            assert_ne!(tasks_id, users_id);
        }

        #[test]
        fn reverse_lookup_by_numeric_id() {
            let mut store = MemoryStorageAdapter::default();
            store
                .register_collection(&tasks())
                .expect("test operation should succeed");
            let nid = store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .expect("test operation should succeed");

            let resolved = store
                .collection_by_numeric_id(nid)
                .expect("test operation should succeed");
            assert_eq!(resolved.as_ref(), Some(&tasks()));
        }

        #[test]
        fn unregistered_collection_has_no_numeric_id() {
            let store = MemoryStorageAdapter::default();
            let nid = store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed");
            assert!(nid.is_none());
        }

        #[test]
        fn unregister_removes_numeric_id() {
            let mut store = MemoryStorageAdapter::default();
            store
                .register_collection(&tasks())
                .expect("test operation should succeed");
            let nid = store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .expect("test operation should succeed");

            store
                .unregister_collection(&tasks())
                .expect("test operation should succeed");
            assert!(store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .is_none());
            assert!(store
                .collection_by_numeric_id(nid)
                .expect("test operation should succeed")
                .is_none());
        }

        #[test]
        fn abort_tx_rolls_back_numeric_ids() {
            let mut store = MemoryStorageAdapter::default();
            store.begin_tx().expect("test operation should succeed");
            store
                .register_collection(&tasks())
                .expect("test operation should succeed");
            assert!(store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .is_some());

            store.abort_tx().expect("test operation should succeed");
            assert!(store
                .collection_numeric_id(&tasks())
                .expect("test operation should succeed")
                .is_none());
        }

        #[test]
        fn unknown_numeric_id_returns_none() {
            let store = MemoryStorageAdapter::default();
            assert!(store
                .collection_by_numeric_id(9999)
                .expect("test operation should succeed")
                .is_none());
        }
    }

    // ── Dedicated link store tests (ADR-010) ────────────────────────────

    mod dedicated_links {
        use super::*;
        use serde_json::json;

        fn make_link() -> Link {
            Link {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-001"),
                link_type: "assigned-to".into(),
                metadata: json!({}),
            }
        }

        #[test]
        fn put_and_get_link() {
            let mut store = MemoryStorageAdapter::default();
            let link = make_link();
            store
                .put_link(&link)
                .expect("test operation should succeed");

            let found = store
                .get_link(
                    &link.source_collection,
                    &link.source_id,
                    &link.link_type,
                    &link.target_collection,
                    &link.target_id,
                )
                .expect("test operation should succeed");
            assert_eq!(found, Some(link));
        }

        #[test]
        fn get_nonexistent_link_returns_none() {
            let store = MemoryStorageAdapter::default();
            let found = store
                .get_link(
                    &CollectionId::new("a"),
                    &EntityId::new("1"),
                    "x",
                    &CollectionId::new("b"),
                    &EntityId::new("2"),
                )
                .expect("test operation should succeed");
            assert!(found.is_none());
        }

        #[test]
        fn delete_link_removes_it() {
            let mut store = MemoryStorageAdapter::default();
            let link = make_link();
            store
                .put_link(&link)
                .expect("test operation should succeed");

            store
                .delete_link(
                    &link.source_collection,
                    &link.source_id,
                    &link.link_type,
                    &link.target_collection,
                    &link.target_id,
                )
                .expect("test operation should succeed");

            let found = store
                .get_link(
                    &link.source_collection,
                    &link.source_id,
                    &link.link_type,
                    &link.target_collection,
                    &link.target_id,
                )
                .expect("test operation should succeed");
            assert!(found.is_none());
        }

        #[test]
        fn list_outbound_links_all() {
            let mut store = MemoryStorageAdapter::default();
            let link1 = make_link();
            let link2 = Link {
                link_type: "created-by".into(),
                ..make_link()
            };
            store
                .put_link(&link1)
                .expect("test operation should succeed");
            store
                .put_link(&link2)
                .expect("test operation should succeed");

            let outbound = store
                .list_outbound_links(&CollectionId::new("tasks"), &EntityId::new("t-001"), None)
                .expect("test operation should succeed");
            assert_eq!(outbound.len(), 2);
        }

        #[test]
        fn list_outbound_links_filtered() {
            let mut store = MemoryStorageAdapter::default();
            let link1 = make_link();
            let link2 = Link {
                link_type: "created-by".into(),
                ..make_link()
            };
            store
                .put_link(&link1)
                .expect("test operation should succeed");
            store
                .put_link(&link2)
                .expect("test operation should succeed");

            let outbound = store
                .list_outbound_links(
                    &CollectionId::new("tasks"),
                    &EntityId::new("t-001"),
                    Some("assigned-to"),
                )
                .expect("test operation should succeed");
            assert_eq!(outbound.len(), 1);
            assert_eq!(outbound[0].link_type, "assigned-to");
        }

        #[test]
        fn list_inbound_links() {
            let mut store = MemoryStorageAdapter::default();
            let link = make_link();
            store
                .put_link(&link)
                .expect("test operation should succeed");

            let inbound = store
                .list_inbound_links(&CollectionId::new("users"), &EntityId::new("u-001"), None)
                .expect("test operation should succeed");
            assert_eq!(inbound.len(), 1);
            assert_eq!(inbound[0].source_id, EntityId::new("t-001"));
        }

        #[test]
        fn abort_tx_rolls_back_links() {
            let mut store = MemoryStorageAdapter::default();
            store.begin_tx().expect("test operation should succeed");
            store
                .put_link(&make_link())
                .expect("test operation should succeed");
            store.abort_tx().expect("test operation should succeed");

            let outbound = store
                .list_outbound_links(&CollectionId::new("tasks"), &EntityId::new("t-001"), None)
                .expect("test operation should succeed");
            assert!(outbound.is_empty());
        }

        #[test]
        fn namespace_catalogs_allow_same_name_without_cross_drop() {
            let mut store = MemoryStorageAdapter::default();
            let invoices = CollectionId::new("invoices");
            let billing = Namespace::new("prod", "billing");
            let engineering = Namespace::new("prod", "engineering");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&billing)
                .expect("billing namespace create should succeed");
            store
                .create_namespace(&engineering)
                .expect("engineering namespace create should succeed");

            store
                .register_collection_in_namespace(&invoices, &Namespace::default_ns())
                .expect("default collection register should succeed");
            store
                .register_collection_in_namespace(&invoices, &billing)
                .expect("billing collection register should succeed");
            store
                .register_collection_in_namespace(&invoices, &engineering)
                .expect("engineering collection register should succeed");

            assert_eq!(
                store
                    .list_namespace_collections(&billing)
                    .expect("billing list should succeed"),
                vec![invoices.clone()]
            );
            assert_eq!(
                store
                    .list_namespace_collections(&engineering)
                    .expect("engineering list should succeed"),
                vec![invoices.clone()]
            );

            store
                .drop_namespace(&billing)
                .expect("billing drop should succeed");
            assert_eq!(
                store
                    .list_namespace_collections(&engineering)
                    .expect("engineering list should survive billing drop"),
                vec![invoices.clone()]
            );
            assert_eq!(
                store
                    .list_namespace_collections(&Namespace::default_ns())
                    .expect("default list should survive billing drop"),
                vec![invoices.clone()]
            );

            store
                .drop_database("prod")
                .expect("prod drop should succeed");
            assert_eq!(
                store
                    .list_namespace_collections(&Namespace::default_ns())
                    .expect("default list should survive prod drop"),
                vec![invoices]
            );
        }

        #[test]
        fn drop_namespace_purges_entities_for_removed_collections() {
            let mut store = MemoryStorageAdapter::default();
            let billing = Namespace::new("prod", "billing");
            let engineering = Namespace::new("prod", "engineering");
            let invoices = CollectionId::new("invoices");
            let ledger = CollectionId::new("ledger");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&billing)
                .expect("billing namespace create should succeed");
            store
                .create_namespace(&engineering)
                .expect("engineering namespace create should succeed");
            store
                .register_collection_in_namespace(&invoices, &billing)
                .expect("billing collection register should succeed");
            store
                .register_collection_in_namespace(&ledger, &engineering)
                .expect("engineering collection register should succeed");
            store
                .put(Entity::new(
                    invoices.clone(),
                    EntityId::new("inv-001"),
                    json!({"title": "invoice"}),
                ))
                .expect("billing entity put should succeed");
            store
                .put(Entity::new(
                    ledger.clone(),
                    EntityId::new("led-001"),
                    json!({"title": "ledger"}),
                ))
                .expect("engineering entity put should succeed");

            store
                .drop_namespace(&billing)
                .expect("billing drop should succeed");

            assert!(
                store
                    .get(&invoices, &EntityId::new("inv-001"))
                    .expect("billing entity lookup should succeed")
                    .is_none(),
                "dropped namespace entities must be purged"
            );
            assert!(
                store
                    .get(&ledger, &EntityId::new("led-001"))
                    .expect("surviving entity lookup should succeed")
                    .is_some(),
                "entities in other namespaces must survive"
            );
        }

        #[test]
        fn drop_namespace_keeps_same_named_entities_in_surviving_namespaces() {
            let mut store = MemoryStorageAdapter::default();
            let billing = Namespace::new("prod", "billing");
            let engineering = Namespace::new("prod", "engineering");
            let invoices = CollectionId::new("invoices");
            let ledger = CollectionId::new("ledger");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&billing)
                .expect("billing namespace create should succeed");
            store
                .create_namespace(&engineering)
                .expect("engineering namespace create should succeed");
            store
                .register_collection_in_namespace(&invoices, &Namespace::default_ns())
                .expect("default collection register should succeed");
            store
                .register_collection_in_namespace(&invoices, &billing)
                .expect("billing collection register should succeed");
            store
                .register_collection_in_namespace(&invoices, &engineering)
                .expect("engineering collection register should succeed");
            store
                .register_collection_in_namespace(&ledger, &billing)
                .expect("billing ledger register should succeed");
            store
                .put(Entity::new(
                    invoices.clone(),
                    EntityId::new("inv-default-001"),
                    json!({"title": "default invoice"}),
                ))
                .expect("default entity put should succeed");
            store
                .put(Entity::new(
                    ledger.clone(),
                    EntityId::new("led-billing-001"),
                    json!({"title": "billing ledger"}),
                ))
                .expect("billing ledger put should succeed");

            store
                .drop_namespace(&billing)
                .expect("billing drop should succeed");

            assert!(
                store
                    .get(&invoices, &EntityId::new("inv-default-001"))
                    .expect("default entity lookup should succeed")
                    .is_some(),
                "same-named entities in surviving namespaces must be preserved"
            );
            assert!(
                store
                    .get(&ledger, &EntityId::new("led-billing-001"))
                    .expect("billing ledger lookup should succeed")
                    .is_none(),
                "entities in the dropped namespace must be purged"
            );
        }

        #[test]
        fn drop_namespace_purges_links_for_removed_collections() {
            let mut store = MemoryStorageAdapter::default();
            let billing = Namespace::new("prod", "billing");
            let engineering = Namespace::new("prod", "engineering");
            let invoices = CollectionId::new("prod.billing.invoices");
            let ledger = CollectionId::new("prod.engineering.ledger");
            let keep = CollectionId::new("keep");
            let archive = CollectionId::new("archive");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&billing)
                .expect("billing namespace create should succeed");
            store
                .create_namespace(&engineering)
                .expect("engineering namespace create should succeed");
            store
                .register_collection_in_namespace(&CollectionId::new("invoices"), &billing)
                .expect("billing collection register should succeed");
            store
                .register_collection_in_namespace(&CollectionId::new("ledger"), &engineering)
                .expect("engineering collection register should succeed");
            store
                .register_collection(&keep)
                .expect("default collection register should succeed");
            store
                .register_collection(&archive)
                .expect("archive collection register should succeed");
            for entity in [
                Entity::new(
                    invoices.clone(),
                    EntityId::new("inv-001"),
                    json!({"title": "invoice"}),
                ),
                Entity::new(
                    ledger.clone(),
                    EntityId::new("led-001"),
                    json!({"title": "ledger"}),
                ),
                Entity::new(
                    keep.clone(),
                    EntityId::new("keep-001"),
                    json!({"title": "keep"}),
                ),
                Entity::new(
                    archive.clone(),
                    EntityId::new("arc-001"),
                    json!({"title": "archive"}),
                ),
            ] {
                store.put(entity).expect("entity put should succeed");
            }

            for link in [
                Link {
                    source_collection: invoices.clone(),
                    source_id: EntityId::new("inv-001"),
                    target_collection: ledger.clone(),
                    target_id: EntityId::new("led-001"),
                    link_type: "relates-to".into(),
                    metadata: serde_json::Value::Null,
                },
                Link {
                    source_collection: keep.clone(),
                    source_id: EntityId::new("keep-001"),
                    target_collection: invoices.clone(),
                    target_id: EntityId::new("inv-001"),
                    link_type: "references".into(),
                    metadata: serde_json::Value::Null,
                },
                Link {
                    source_collection: keep.clone(),
                    source_id: EntityId::new("keep-001"),
                    target_collection: archive.clone(),
                    target_id: EntityId::new("arc-001"),
                    link_type: "references".into(),
                    metadata: serde_json::Value::Null,
                },
            ] {
                store.put_link(&link).expect("link put should succeed");
            }

            store
                .drop_namespace(&billing)
                .expect("billing drop should succeed");

            assert!(
                store
                    .list_inbound_links(&ledger, &EntityId::new("led-001"), None)
                    .expect("ledger inbound links should load")
                    .is_empty(),
                "links from removed collections must be purged"
            );
            let keep_links = store
                .list_outbound_links(&keep, &EntityId::new("keep-001"), None)
                .expect("keep outbound links should load");
            assert_eq!(keep_links.len(), 1);
            assert_eq!(keep_links[0].target_collection, archive);
        }

        #[test]
        fn drop_database_purges_entities_for_removed_collections() {
            let mut store = MemoryStorageAdapter::default();
            let analytics = Namespace::new("prod", "analytics");
            let orders = CollectionId::new("orders");
            let rollups = CollectionId::new("rollups");
            let keep = CollectionId::new("keep");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&analytics)
                .expect("analytics namespace create should succeed");
            store
                .register_collection_in_namespace(&orders, &Namespace::new("prod", "default"))
                .expect("prod default collection register should succeed");
            store
                .register_collection_in_namespace(&rollups, &analytics)
                .expect("analytics collection register should succeed");
            store
                .register_collection_in_namespace(&keep, &Namespace::default_ns())
                .expect("default collection register should succeed");
            store
                .put(Entity::new(
                    orders.clone(),
                    EntityId::new("ord-001"),
                    json!({"title": "order"}),
                ))
                .expect("prod default entity put should succeed");
            store
                .put(Entity::new(
                    rollups.clone(),
                    EntityId::new("sum-001"),
                    json!({"title": "rollup"}),
                ))
                .expect("analytics entity put should succeed");
            store
                .put(Entity::new(
                    keep.clone(),
                    EntityId::new("keep-001"),
                    json!({"title": "keep"}),
                ))
                .expect("default entity put should succeed");

            store
                .drop_database("prod")
                .expect("database drop should succeed");

            assert!(
                store
                    .get(&orders, &EntityId::new("ord-001"))
                    .expect("orders lookup should succeed")
                    .is_none(),
                "dropped database entities must be purged"
            );
            assert!(
                store
                    .get(&rollups, &EntityId::new("sum-001"))
                    .expect("rollups lookup should succeed")
                    .is_none(),
                "all namespace entities in the dropped database must be purged"
            );
            assert!(
                store
                    .get(&keep, &EntityId::new("keep-001"))
                    .expect("default lookup should succeed")
                    .is_some(),
                "entities in other databases must survive"
            );
        }

        #[test]
        fn drop_database_purges_links_for_removed_collections() {
            let mut store = MemoryStorageAdapter::default();
            let analytics = Namespace::new("prod", "analytics");
            let orders = CollectionId::new("prod.default.orders");
            let rollups = CollectionId::new("prod.analytics.rollups");
            let keep = CollectionId::new("keep");
            let archive = CollectionId::new("archive");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&analytics)
                .expect("analytics namespace create should succeed");
            store
                .register_collection_in_namespace(
                    &CollectionId::new("orders"),
                    &Namespace::new("prod", "default"),
                )
                .expect("prod default collection register should succeed");
            store
                .register_collection_in_namespace(&CollectionId::new("rollups"), &analytics)
                .expect("analytics collection register should succeed");
            store
                .register_collection(&keep)
                .expect("default collection register should succeed");
            store
                .register_collection(&archive)
                .expect("archive collection register should succeed");
            for entity in [
                Entity::new(
                    orders.clone(),
                    EntityId::new("ord-001"),
                    json!({"title": "order"}),
                ),
                Entity::new(
                    rollups.clone(),
                    EntityId::new("sum-001"),
                    json!({"title": "rollup"}),
                ),
                Entity::new(
                    keep.clone(),
                    EntityId::new("keep-001"),
                    json!({"title": "keep"}),
                ),
                Entity::new(
                    archive.clone(),
                    EntityId::new("arc-001"),
                    json!({"title": "archive"}),
                ),
            ] {
                store.put(entity).expect("entity put should succeed");
            }

            for link in [
                Link {
                    source_collection: keep.clone(),
                    source_id: EntityId::new("keep-001"),
                    target_collection: orders.clone(),
                    target_id: EntityId::new("ord-001"),
                    link_type: "references".into(),
                    metadata: serde_json::Value::Null,
                },
                Link {
                    source_collection: rollups.clone(),
                    source_id: EntityId::new("sum-001"),
                    target_collection: keep.clone(),
                    target_id: EntityId::new("keep-001"),
                    link_type: "feeds".into(),
                    metadata: serde_json::Value::Null,
                },
                Link {
                    source_collection: keep.clone(),
                    source_id: EntityId::new("keep-001"),
                    target_collection: archive.clone(),
                    target_id: EntityId::new("arc-001"),
                    link_type: "references".into(),
                    metadata: serde_json::Value::Null,
                },
            ] {
                store.put_link(&link).expect("link put should succeed");
            }

            store
                .drop_database("prod")
                .expect("database drop should succeed");

            assert!(
                store
                    .list_inbound_links(&keep, &EntityId::new("keep-001"), Some("feeds"))
                    .expect("keep inbound links should load")
                    .is_empty(),
                "inbound links from removed databases must be purged"
            );
            let keep_links = store
                .list_outbound_links(&keep, &EntityId::new("keep-001"), None)
                .expect("keep outbound links should load");
            assert_eq!(keep_links.len(), 1);
            assert_eq!(keep_links[0].target_collection, archive);
        }

        #[test]
        fn drop_database_keeps_same_named_entities_in_surviving_databases() {
            let mut store = MemoryStorageAdapter::default();
            let billing = Namespace::new("prod", "billing");
            let invoices = CollectionId::new("invoices");
            let orders = CollectionId::new("orders");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&billing)
                .expect("billing namespace create should succeed");
            store
                .register_collection_in_namespace(&invoices, &Namespace::default_ns())
                .expect("default collection register should succeed");
            store
                .register_collection_in_namespace(&invoices, &billing)
                .expect("billing collection register should succeed");
            store
                .register_collection_in_namespace(&orders, &Namespace::new("prod", "default"))
                .expect("prod orders register should succeed");
            store
                .put(Entity::new(
                    invoices.clone(),
                    EntityId::new("inv-default-001"),
                    json!({"title": "default invoice"}),
                ))
                .expect("default entity put should succeed");
            store
                .put(Entity::new(
                    orders.clone(),
                    EntityId::new("ord-prod-001"),
                    json!({"title": "prod order"}),
                ))
                .expect("prod orders put should succeed");

            store
                .drop_database("prod")
                .expect("prod drop should succeed");

            assert!(
                store
                    .get(&invoices, &EntityId::new("inv-default-001"))
                    .expect("default entity lookup should succeed")
                    .is_some(),
                "same-named entities in surviving databases must be preserved"
            );
            assert!(
                store
                    .get(&orders, &EntityId::new("ord-prod-001"))
                    .expect("dropped database entity lookup should succeed")
                    .is_none(),
                "entities in the dropped database must be purged"
            );
        }

        #[test]
        fn qualified_entity_identity_isolated_across_namespaces() {
            let mut store = MemoryStorageAdapter::default();
            let billing = Namespace::new("prod", "billing");
            let engineering = Namespace::new("prod", "engineering");
            let invoices = CollectionId::new("invoices");
            let billing_invoices = CollectionId::new("prod.billing.invoices");
            let engineering_invoices = CollectionId::new("prod.engineering.invoices");
            let entity_id = EntityId::new("inv-001");

            store
                .create_database("prod")
                .expect("database create should succeed");
            store
                .create_namespace(&billing)
                .expect("billing namespace create should succeed");
            store
                .create_namespace(&engineering)
                .expect("engineering namespace create should succeed");
            store
                .register_collection_in_namespace(&invoices, &billing)
                .expect("billing collection register should succeed");
            store
                .register_collection_in_namespace(&invoices, &engineering)
                .expect("engineering collection register should succeed");

            store
                .put(Entity::new(
                    billing_invoices.clone(),
                    entity_id.clone(),
                    json!({"scope": "billing"}),
                ))
                .expect("billing entity put should succeed");
            store
                .put(Entity::new(
                    engineering_invoices.clone(),
                    entity_id.clone(),
                    json!({"scope": "engineering"}),
                ))
                .expect("engineering entity put should succeed");

            assert_eq!(
                store
                    .get(&billing_invoices, &entity_id)
                    .expect("billing get should succeed")
                    .expect("billing entity should exist")
                    .data["scope"],
                json!("billing")
            );
            assert_eq!(
                store
                    .get(&engineering_invoices, &entity_id)
                    .expect("engineering get should succeed")
                    .expect("engineering entity should exist")
                    .data["scope"],
                json!("engineering")
            );
            assert_eq!(
                store
                    .count(&billing_invoices)
                    .expect("billing count should succeed"),
                1
            );
            assert_eq!(
                store
                    .count(&engineering_invoices)
                    .expect("engineering count should succeed"),
                1
            );

            let updated = store
                .compare_and_swap(
                    Entity::new(
                        billing_invoices.clone(),
                        entity_id.clone(),
                        json!({"scope": "billing-updated"}),
                    ),
                    1,
                )
                .expect("billing compare_and_swap should succeed");
            assert_eq!(updated.version, 2);
            assert_eq!(
                store
                    .range_scan(&billing_invoices, None, None, None)
                    .expect("billing range scan should succeed")
                    .len(),
                1
            );
            assert_eq!(
                store
                    .range_scan(&engineering_invoices, None, None, None)
                    .expect("engineering range scan should succeed")
                    .len(),
                1
            );
            assert_eq!(
                store
                    .get(&engineering_invoices, &entity_id)
                    .expect("engineering get after billing update should succeed")
                    .expect("engineering entity should still exist")
                    .version,
                1
            );

            store
                .delete(&billing_invoices, &entity_id)
                .expect("billing delete should succeed");
            assert!(
                store
                    .get(&billing_invoices, &entity_id)
                    .expect("billing get after delete should succeed")
                    .is_none(),
                "billing entity should be removed"
            );
            assert!(
                store
                    .get(&engineering_invoices, &entity_id)
                    .expect("engineering get after billing delete should succeed")
                    .is_some(),
                "engineering entity should survive"
            );
        }
    }
}

// L4 conformance test suite for MemoryStorageAdapter.
crate::storage_conformance_tests!(memory_conformance, MemoryStorageAdapter::default());
