use std::ops::Bound;

use axon_audit::entry::AuditEntry;
use axon_core::error::AxonError;
use axon_core::id::CollectionId;
use axon_core::id::EntityId;
use axon_core::types::{Entity, Link};
use axon_schema::schema::CollectionSchema;

/// A typed index value extracted from entity data.
///
/// Values are stored in EAV index tables, one variant per [`IndexType`].
/// The `Ord` implementation provides the sort order used for range scans.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndexValue {
    /// UTF-8 string value.
    String(String),
    /// Signed 64-bit integer.
    Integer(i64),
    /// IEEE 754 float, stored as ordered bits for BTreeMap compatibility.
    Float(OrderedFloat),
    /// Boolean value (false < true).
    Boolean(bool),
}

impl std::fmt::Display for IndexValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexValue::String(s) => write!(f, "\"{s}\""),
            IndexValue::Integer(n) => write!(f, "{n}"),
            IndexValue::Float(OrderedFloat(bits)) => {
                write!(f, "{}", f64::from_bits(*bits))
            }
            IndexValue::Boolean(b) => write!(f, "{b}"),
        }
    }
}

/// Wrapper for f64 that implements `Eq` and `Ord` via total ordering on bits.
///
/// NaN values are sorted after all other values. This enables use as a
/// BTreeMap key.
#[derive(Debug, Clone, Copy)]
pub struct OrderedFloat(pub u64);

impl OrderedFloat {
    pub fn new(val: f64) -> Self {
        Self(val.to_bits())
    }

    pub fn value(&self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for OrderedFloat {}

impl std::hash::Hash for OrderedFloat {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a = f64::from_bits(self.0);
        let b = f64::from_bits(other.0);
        a.total_cmp(&b)
    }
}

/// Extract an [`IndexValue`] from a JSON value according to the declared index type.
///
/// Returns `None` if the value is null, missing, or has a type mismatch
/// (per FEAT-013: mismatched types are not indexed, not errors).
pub fn extract_index_value(
    json_val: &serde_json::Value,
    index_type: &axon_schema::schema::IndexType,
) -> Option<IndexValue> {
    use axon_schema::schema::IndexType;
    match index_type {
        IndexType::String => json_val.as_str().map(|s| IndexValue::String(s.to_string())),
        IndexType::Integer => json_val.as_i64().map(IndexValue::Integer),
        IndexType::Float => json_val.as_f64().map(|f| IndexValue::Float(OrderedFloat::new(f))),
        IndexType::Boolean => json_val.as_bool().map(IndexValue::Boolean),
        IndexType::Datetime => {
            // Datetimes are stored as strings for ordering purposes.
            json_val
                .as_str()
                .map(|s| IndexValue::String(s.to_string()))
        }
    }
}

/// A compound index key: ordered list of field values for multi-field indexes.
///
/// Implements `Ord` lexicographically, enabling prefix matching via
/// BTreeMap range scans.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompoundKey(pub Vec<IndexValue>);

/// Extract a compound key from entity data given a compound index definition.
///
/// Returns `None` if any field in the compound key is null/missing/type-mismatch,
/// since compound index entries require all fields to be present.
pub fn extract_compound_key(
    data: &serde_json::Value,
    fields: &[axon_schema::schema::CompoundIndexField],
) -> Option<CompoundKey> {
    let mut values = Vec::with_capacity(fields.len());
    for f in fields {
        let json_val = resolve_field_path(data, &f.field)?;
        let val = extract_index_value(json_val, &f.index_type)?;
        values.push(val);
    }
    Some(CompoundKey(values))
}

/// Navigate a dotted field path in a JSON value.
///
/// E.g., `"address.city"` resolves `{"address": {"city": "NY"}}` to `"NY"`.
pub fn resolve_field_path<'a>(
    data: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    let mut current = data;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

/// Abstraction over Axon's backing storage.
///
/// Implementations wrap specific databases (SQLite, PostgreSQL, FoundationDB).
/// All operations are executed within the context of the adapter's internal
/// transaction semantics.
pub trait StorageAdapter: Send + Sync {
    /// Retrieves an entity by collection and ID.
    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError>;

    /// Stores an entity, overwriting any existing entity with the same ID.
    /// Use this for initial inserts. For versioned updates, use [`compare_and_swap`].
    fn put(&mut self, entity: Entity) -> Result<(), AxonError>;

    /// Deletes an entity. Returns `Ok(())` whether or not the entity existed.
    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError>;

    /// Returns the number of entities in the given collection.
    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError>;

    /// Returns entities in a collection ordered by entity ID.
    ///
    /// - `start`: inclusive lower bound (no lower bound if `None`)
    /// - `end`: inclusive upper bound (no upper bound if `None`)
    /// - `limit`: maximum number of results (unlimited if `None`)
    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError>;

    /// Atomically updates an entity using optimistic concurrency control (OCC).
    ///
    /// Writes `entity` only if the current stored version equals `expected_version`.
    /// On success, the stored version is incremented to `expected_version + 1` and
    /// the updated entity is returned.
    ///
    /// Returns [`AxonError::ConflictingVersion`] if the version does not match or
    /// the entity does not exist.
    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError>;

    /// Begin a storage-level transaction.
    ///
    /// After this call, all writes are buffered or protected until [`commit_tx`]
    /// or [`abort_tx`] is called. Callers must call exactly one of those to
    /// end the transaction.
    ///
    /// The default implementation is a no-op (suitable for adapters whose
    /// mutation methods are already atomic or whose concurrency model does
    /// not require explicit transactions).
    fn begin_tx(&mut self) -> Result<(), AxonError> {
        Ok(())
    }

    /// Commit the current transaction, making all buffered writes durable.
    ///
    /// Returns an error if no transaction is active.
    /// The default implementation is a no-op.
    fn commit_tx(&mut self) -> Result<(), AxonError> {
        Ok(())
    }

    /// Abort the current transaction, discarding all buffered writes.
    ///
    /// Idempotent: calling `abort_tx` when no transaction is active is not an error.
    /// The default implementation is a no-op.
    fn abort_tx(&mut self) -> Result<(), AxonError> {
        Ok(())
    }

    // ── Audit log (co-located writes) ────────────────────────────────────────

    /// Append an audit entry within the current storage transaction.
    ///
    /// This is called **inside** `begin_tx` / `commit_tx` so that the audit
    /// write is part of the same atomic database transaction as entity
    /// mutations. If the storage transaction is later rolled back, the audit
    /// entry is rolled back too.
    ///
    /// The implementation must assign `entry.id` (from the backing store's
    /// sequence) and `entry.timestamp_ns` (current wall-clock time) if they
    /// are still at their zero sentinel values, then persist the entry.
    ///
    /// The default implementation is a no-op that returns the entry unchanged.
    /// Adapters that support co-located audit writes (e.g. SQLite) should
    /// override this method.
    fn append_audit_entry(&mut self, entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        Ok(entry)
    }

    // ── Schema persistence ───────────────────────────────────────────────────

    /// Persist a [`CollectionSchema`], replacing any previously stored schema for
    /// the same collection.
    ///
    /// The default implementation is a no-op that returns `Ok(())`.
    /// Concrete adapters should override this to provide durable schema storage.
    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        let _ = schema;
        Ok(())
    }

    /// Retrieve the [`CollectionSchema`] for a collection, if one exists.
    ///
    /// Returns `Ok(None)` when no schema has been stored for the collection.
    /// The default implementation always returns `Ok(None)`.
    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        let _ = collection;
        Ok(None)
    }

    /// Retrieve a specific version of a collection schema.
    ///
    /// Returns `Ok(None)` if the version does not exist.
    /// The default implementation always returns `Ok(None)`.
    fn get_schema_version(
        &self,
        collection: &CollectionId,
        version: u32,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        let _ = (collection, version);
        Ok(None)
    }

    /// List all schema versions for a collection, returning (version, created_at_ns) pairs
    /// in ascending version order.
    ///
    /// The default implementation returns an empty list.
    fn list_schema_versions(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<(u32, u64)>, AxonError> {
        let _ = collection;
        Ok(vec![])
    }

    /// Delete the schema for a collection. Returns `Ok(())` whether or not a
    /// schema existed.
    ///
    /// The default implementation is a no-op.
    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let _ = collection;
        Ok(())
    }

    // ── Collection registry ──────────────────────────────────────────────────

    /// Record that a named collection has been explicitly created.
    ///
    /// Implementations must persist this so the collection survives process
    /// restart. The default implementation is a no-op.
    fn register_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let _ = collection;
        Ok(())
    }

    /// Remove a collection from the registry.
    ///
    /// Returns `Ok(())` whether or not the collection was registered.
    /// The default implementation is a no-op.
    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let _ = collection;
        Ok(())
    }

    /// Return all explicitly-registered collection names, in ascending order.
    ///
    /// The default implementation returns an empty list (suitable for adapters
    /// that do not implement persistent collection registration).
    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        Ok(vec![])
    }

    // ── Numeric collection ID mapping (ADR-010) ─────────────────────────

    /// Resolve a collection's stable numeric ID.
    ///
    /// Returns the auto-assigned numeric ID for a registered collection.
    /// Returns `None` if the collection has not been registered.
    /// The default implementation always returns `None`.
    fn collection_numeric_id(&self, collection: &CollectionId) -> Result<Option<u64>, AxonError> {
        let _ = collection;
        Ok(None)
    }

    /// Resolve a numeric ID back to a collection name.
    ///
    /// Returns `None` if no collection has the given numeric ID.
    /// The default implementation always returns `None`.
    fn collection_by_numeric_id(&self, numeric_id: u64) -> Result<Option<CollectionId>, AxonError> {
        let _ = numeric_id;
        Ok(None)
    }

    // ── Secondary index operations (FEAT-013) ───────────────────────────

    /// Update index entries for an entity.
    ///
    /// Removes any existing index entries for this entity, then inserts new
    /// entries based on the entity's current data and the declared indexes.
    /// Called automatically on every `put` and `compare_and_swap`.
    ///
    /// `old_data` is the previous entity data (for removing stale entries).
    /// `None` if this is a new entity.
    ///
    /// The default implementation is a no-op.
    fn update_indexes(
        &mut self,
        _collection: &CollectionId,
        _entity_id: &EntityId,
        _old_data: Option<&serde_json::Value>,
        _new_data: &serde_json::Value,
        _indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        Ok(())
    }

    /// Remove all index entries for a deleted entity.
    ///
    /// The default implementation is a no-op.
    fn remove_index_entries(
        &mut self,
        _collection: &CollectionId,
        _entity_id: &EntityId,
        _data: &serde_json::Value,
        _indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        Ok(())
    }

    /// Look up entity IDs by exact index value (equality query).
    ///
    /// Returns entity IDs in ascending order. Returns an empty vec if the
    /// index does not exist or no entities match.
    fn index_lookup(
        &self,
        _collection: &CollectionId,
        _field: &str,
        _value: &IndexValue,
    ) -> Result<Vec<EntityId>, AxonError> {
        Ok(vec![])
    }

    /// Range scan on an index, returning entity IDs whose indexed value
    /// falls within the given bounds.
    ///
    /// Returns entity IDs sorted by indexed value, then by entity ID.
    fn index_range(
        &self,
        _collection: &CollectionId,
        _field: &str,
        _lower: Bound<&IndexValue>,
        _upper: Bound<&IndexValue>,
    ) -> Result<Vec<EntityId>, AxonError> {
        Ok(vec![])
    }

    /// Check if the given value already exists in a unique index for a
    /// different entity (i.e., would violate a unique constraint).
    ///
    /// Returns `true` if the value is already taken by another entity.
    fn index_unique_conflict(
        &self,
        _collection: &CollectionId,
        _field: &str,
        _value: &IndexValue,
        _exclude_entity: &EntityId,
    ) -> Result<bool, AxonError> {
        Ok(false)
    }

    /// Drop all index entries for a collection (e.g. on collection drop).
    ///
    /// The default implementation is a no-op.
    fn drop_indexes(&mut self, _collection: &CollectionId) -> Result<(), AxonError> {
        Ok(())
    }

    // ── Compound index operations (FEAT-013, US-033) ────────────────────

    /// Update compound index entries for an entity.
    fn update_compound_indexes(
        &mut self,
        _collection: &CollectionId,
        _entity_id: &EntityId,
        _old_data: Option<&serde_json::Value>,
        _new_data: &serde_json::Value,
        _indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        Ok(())
    }

    /// Remove compound index entries for a deleted entity.
    fn remove_compound_index_entries(
        &mut self,
        _collection: &CollectionId,
        _entity_id: &EntityId,
        _data: &serde_json::Value,
        _indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        Ok(())
    }

    /// Look up entity IDs by exact compound key.
    fn compound_index_lookup(
        &self,
        _collection: &CollectionId,
        _index_idx: usize,
        _key: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        Ok(vec![])
    }

    /// Prefix match on a compound index using a partial key.
    ///
    /// Returns entity IDs whose compound key starts with the given prefix.
    fn compound_index_prefix(
        &self,
        _collection: &CollectionId,
        _index_idx: usize,
        _prefix: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        Ok(vec![])
    }

    // ── Dedicated link storage (ADR-010) ────────────────────────────────

    /// Store a link in the dedicated links table.
    ///
    /// Replaces any existing link with the same (source, target, link_type) key.
    /// The default implementation falls back to the pseudo-collection approach.
    fn put_link(&mut self, link: &Link) -> Result<(), AxonError> {
        self.put(link.to_entity())?;
        self.put(link.to_rev_entity())
    }

    /// Delete a link from the dedicated links table.
    ///
    /// Returns `Ok(())` whether or not the link existed.
    fn delete_link(
        &mut self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_collection: &CollectionId,
        target_id: &EntityId,
    ) -> Result<(), AxonError> {
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

    /// Look up a specific link.
    ///
    /// Returns `None` if the link does not exist.
    fn get_link(
        &self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_collection: &CollectionId,
        target_id: &EntityId,
    ) -> Result<Option<Link>, AxonError> {
        let fwd_id = Link::storage_id(
            source_collection,
            source_id,
            link_type,
            target_collection,
            target_id,
        );
        let entity = self.get(&Link::links_collection(), &fwd_id)?;
        Ok(entity.and_then(|e| Link::from_entity(&e)))
    }

    /// List outbound links from a given entity.
    ///
    /// If `link_type` is `Some`, only returns links of that type.
    fn list_outbound_links(
        &self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: Option<&str>,
    ) -> Result<Vec<Link>, AxonError> {
        let prefix = match link_type {
            Some(lt) => format!("{source_collection}/{source_id}/{lt}/"),
            None => format!("{source_collection}/{source_id}/"),
        };
        let start = EntityId::new(&prefix);
        // Compute an exclusive upper bound by incrementing last byte.
        let mut end_str = prefix.clone();
        // Replace trailing '/' with '0' (ASCII after '/') as upper bound.
        end_str.pop();
        end_str.push('0');
        let end = EntityId::new(&end_str);
        let entities = self.range_scan(&Link::links_collection(), Some(&start), Some(&end), None)?;
        Ok(entities
            .iter()
            .filter_map(Link::from_entity)
            .collect())
    }

    /// List inbound links to a given entity.
    ///
    /// Uses the reverse index for efficient lookup.
    fn list_inbound_links(
        &self,
        target_collection: &CollectionId,
        target_id: &EntityId,
        link_type: Option<&str>,
    ) -> Result<Vec<Link>, AxonError> {
        // Scan the reverse index to find link keys, then resolve forward entries.
        let prefix = format!("{target_collection}/{target_id}/");
        let start = EntityId::new(&prefix);
        let mut end_str = prefix.clone();
        end_str.pop();
        end_str.push('0');
        let end = EntityId::new(&end_str);
        let rev_entries =
            self.range_scan(&Link::links_rev_collection(), Some(&start), Some(&end), None)?;

        let mut links = Vec::new();
        for rev_ent in &rev_entries {
            // Parse reverse ID: target_col/target_id/source_col/source_id/link_type
            let parts: Vec<&str> = rev_ent.id.as_str().splitn(5, '/').collect();
            if parts.len() < 5 {
                continue;
            }
            let (src_col, src_id, lt) = (parts[2], parts[3], parts[4]);
            if let Some(filter_lt) = link_type {
                if lt != filter_lt {
                    continue;
                }
            }
            if let Some(link) = self.get_link(
                &CollectionId::new(src_col),
                &EntityId::new(src_id),
                lt,
                target_collection,
                target_id,
            )? {
                links.push(link);
            }
        }
        Ok(links)
    }
}
