use std::collections::BTreeSet;
use std::ops::Bound;

use axon_audit::entry::AuditEntry;
use axon_core::auth::{TenantId, TenantMember, TenantRole, User, UserId};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, Namespace, QualifiedCollectionId};
use axon_core::types::{Entity, Link};
use axon_schema::schema::{CollectionSchema, CollectionView};
use uuid::Uuid;

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
        IndexType::Float => json_val
            .as_f64()
            .map(|f| IndexValue::Float(OrderedFloat::new(f))),
        IndexType::Boolean => json_val.as_bool().map(IndexValue::Boolean),
        IndexType::Datetime => {
            // Datetimes are stored as strings for ordering purposes.
            json_val.as_str().map(|s| IndexValue::String(s.to_string()))
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

    /// Atomically inserts `entity` only if no live entity currently exists.
    ///
    /// This is used for rollback/recovery flows that recreate a deleted entity
    /// after the caller has already validated the deleted version via audit
    /// history. `expected_absent_version` is echoed back in
    /// [`AxonError::ConflictingVersion`] when another writer recreated the
    /// entity first.
    fn create_if_absent(
        &mut self,
        entity: Entity,
        expected_absent_version: u64,
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

    // ── Database and namespace catalogs (FEAT-014) ─────────────────────────

    /// Create a database and its default schema.
    fn create_database(&mut self, _name: &str) -> Result<(), AxonError> {
        Ok(())
    }

    /// Return all database names in ascending order.
    fn list_databases(&self) -> Result<Vec<String>, AxonError> {
        Ok(vec![])
    }

    /// Drop a database catalog entry after its collections have been removed.
    fn drop_database(&mut self, _name: &str) -> Result<(), AxonError> {
        Ok(())
    }

    /// Create a schema namespace within an existing database.
    fn create_namespace(&mut self, _namespace: &Namespace) -> Result<(), AxonError> {
        Ok(())
    }

    /// Return all schema names within a database in ascending order.
    fn list_namespaces(&self, _database: &str) -> Result<Vec<String>, AxonError> {
        Ok(vec![])
    }

    /// Drop a schema namespace after its collections have been removed.
    fn drop_namespace(&mut self, _namespace: &Namespace) -> Result<(), AxonError> {
        Ok(())
    }

    /// Return collections registered within a namespace in ascending order.
    fn list_namespace_collections(
        &self,
        _namespace: &Namespace,
    ) -> Result<Vec<CollectionId>, AxonError> {
        Ok(vec![])
    }

    /// Resolve a collection identifier to its fully qualified catalog key.
    ///
    /// Concrete adapters should override this when bare collection names need
    /// catalog-aware disambiguation across namespaces or databases.
    fn resolve_collection_key(
        &self,
        collection: &CollectionId,
    ) -> Result<QualifiedCollectionId, AxonError> {
        let (namespace, bare_collection) = Namespace::parse(collection.as_str());
        Ok(QualifiedCollectionId::from_parts(
            &namespace,
            &CollectionId::new(bare_collection),
        ))
    }

    /// Remove any links whose source or target belongs to the provided
    /// collections.
    ///
    /// The default implementation scans the forward link pseudo-collection and
    /// deletes matching links through [`StorageAdapter::delete_link`], allowing
    /// concrete adapters to clean both pseudo-collection rows and any dedicated
    /// link storage they maintain.
    fn purge_links_for_collections(
        &mut self,
        collections: &[QualifiedCollectionId],
    ) -> Result<(), AxonError> {
        if collections.is_empty() {
            return Ok(());
        }

        let doomed: BTreeSet<_> = collections.iter().cloned().collect();
        let links = self.range_scan(&Link::links_collection(), None, None, None)?;
        for entity in links {
            let Some(link) = Link::from_entity(&entity) else {
                continue;
            };

            let source = self.resolve_collection_key(&link.source_collection)?;
            let target = self.resolve_collection_key(&link.target_collection)?;
            if doomed.contains(&source) || doomed.contains(&target) {
                self.delete_link(
                    &link.source_collection,
                    &link.source_id,
                    &link.link_type,
                    &link.target_collection,
                    &link.target_id,
                )?;
            }
        }

        Ok(())
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

    // ── Collection presentation persistence ─────────────────────────────────

    /// Persist a [`CollectionView`], replacing any previously stored view for
    /// the same collection and incrementing its independent version counter.
    ///
    /// Implementations must reject views whose target collection has not been
    /// registered yet.
    ///
    /// Returns the stored view with the adapter-assigned `version` and
    /// `updated_at_ns`.
    fn put_collection_view(&mut self, view: &CollectionView) -> Result<CollectionView, AxonError> {
        Ok(view.clone())
    }

    /// Retrieve the latest [`CollectionView`] for a collection, if one exists.
    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        let _ = collection;
        Ok(None)
    }

    /// Delete the collection view for a collection. Returns `Ok(())` whether or
    /// not a view existed.
    fn delete_collection_view(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        let _ = collection;
        Ok(())
    }

    // ── Collection registry ──────────────────────────────────────────────────

    /// Record that a named collection has been explicitly created.
    ///
    /// Implementations must persist this so the collection survives process
    /// restart. The default implementation is a no-op.
    fn register_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.register_collection_in_namespace(collection, &Namespace::default_ns())
    }

    /// Record that a named collection belongs to a specific namespace.
    ///
    /// Implementations that do not yet distinguish namespaces may ignore the
    /// namespace and treat all collections as belonging to `default.default`.
    fn register_collection_in_namespace(
        &mut self,
        collection: &CollectionId,
        _namespace: &Namespace,
    ) -> Result<(), AxonError> {
        let _ = collection;
        Ok(())
    }

    /// Return whether a collection is registered within a specific namespace.
    fn collection_registered_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        Ok(self
            .list_namespace_collections(namespace)?
            .contains(collection))
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
    /// Returns entity IDs in ascending order. Returns an empty vec if no
    /// entities match. Implementations that do not maintain in-memory indexes
    /// should return `Err` so the caller falls through to a full scan.
    fn index_lookup(
        &self,
        _collection: &CollectionId,
        _field: &str,
        _value: &IndexValue,
    ) -> Result<Vec<EntityId>, AxonError> {
        Err(AxonError::Storage(
            "index_lookup not supported by this adapter".into(),
        ))
    }

    /// Range scan on an index, returning entity IDs whose indexed value
    /// falls within the given bounds.
    ///
    /// Returns entity IDs sorted by indexed value, then by entity ID.
    /// Implementations that do not maintain in-memory indexes should return
    /// `Err` so the caller falls through to a full scan.
    fn index_range(
        &self,
        _collection: &CollectionId,
        _field: &str,
        _lower: Bound<&IndexValue>,
        _upper: Bound<&IndexValue>,
    ) -> Result<Vec<EntityId>, AxonError> {
        Err(AxonError::Storage(
            "index_range not supported by this adapter".into(),
        ))
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
        Err(AxonError::Storage(
            "compound_index_lookup not supported by this adapter".into(),
        ))
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
        Err(AxonError::Storage(
            "compound_index_prefix not supported by this adapter".into(),
        ))
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
        let entities =
            self.range_scan(&Link::links_collection(), Some(&start), Some(&end), None)?;
        Ok(entities.iter().filter_map(Link::from_entity).collect())
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
        let rev_entries = self.range_scan(
            &Link::links_rev_collection(),
            Some(&start),
            Some(&end),
            None,
        )?;

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

    // Gate results now live on the Entity blob itself (FEAT-019); there is
    // no longer a dedicated side-table on the storage adapter.

    // ── Auth / tenancy queries (ADR-018) ─────────────────────────────────────

    /// Returns `true` if the given JWT ID has been revoked.
    ///
    /// Checks the `credential_revocations` table. Returns `false` for
    /// adapters that have not been migrated with [`apply_auth_migrations`].
    fn is_jti_revoked(&self, jti: Uuid) -> Result<bool, AxonError> {
        let _ = jti;
        Ok(false)
    }

    /// Look up a user by ID.
    ///
    /// Returns `Ok(None)` when the user does not exist or when the auth
    /// schema has not been applied.
    fn get_user(&self, user_id: UserId) -> Result<Option<User>, AxonError> {
        let _ = user_id;
        Ok(None)
    }

    /// Look up a tenant membership record.
    ///
    /// Returns `Ok(None)` when the user is not a member of the tenant or
    /// when the auth schema has not been applied.
    fn get_tenant_member(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<Option<TenantMember>, AxonError> {
        let _ = (tenant_id, user_id);
        Ok(None)
    }

    /// Upsert a `(provider, external_id)` → `User` mapping atomically.
    ///
    /// On first call for a given `(provider, external_id)` pair this creates a
    /// new `users` row and a new `user_identities` row pointing at it. On
    /// subsequent calls it returns the existing user unchanged. The method MUST
    /// be concurrency-safe: N parallel calls with the same `(provider,
    /// external_id)` must converge on exactly one `users` row and one
    /// `user_identities` row.
    ///
    /// ADR-018 §6 specifies the SQL pattern for SQL backends:
    ///
    /// ```sql
    /// INSERT INTO users (id, display_name, email, created_at_ms)
    /// VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING;
    /// INSERT INTO user_identities (provider, external_id, user_id, created_at_ms)
    /// VALUES (?, ?, ?, ?) ON CONFLICT (provider, external_id) DO NOTHING;
    /// SELECT user_id FROM user_identities WHERE provider = ? AND external_id = ?;
    /// ```
    ///
    /// Returns the user that now owns this identity (whether freshly created
    /// or pre-existing). Returns `Err` only on real storage errors, not on
    /// 'already exists' races.
    ///
    /// The default implementation returns [`AxonError::InvalidOperation`] so
    /// that adapters which have not been migrated with auth schema don't
    /// silently succeed.
    fn upsert_user_identity(
        &self,
        provider: &str,
        external_id: &str,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<User, AxonError> {
        let _ = (provider, external_id, display_name, email);
        Err(AxonError::InvalidOperation(
            "upsert_user_identity not supported by this adapter".into(),
        ))
    }

    /// Add or update a user's tenant membership. Idempotent on `(tenant_id,
    /// user_id)`: if the row exists, the role is updated to the given value
    /// (this lets the same call act as both 'grant' and 'change role').
    ///
    /// ADR-018 §3 specifies tenant_users is keyed on `(tenant_id, user_id)`.
    /// The SQL pattern is:
    /// ```sql
    /// INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
    /// VALUES (?, ?, ?, ?)
    /// ON CONFLICT (tenant_id, user_id) DO UPDATE SET role = excluded.role;
    /// ```
    ///
    /// The default implementation returns `Err(AxonError::Internal)` so that
    /// unmigrated adapters are explicit about the gap.
    fn upsert_tenant_member(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
        role: TenantRole,
    ) -> Result<TenantMember, AxonError> {
        let _ = (tenant_id, user_id, role);
        Err(AxonError::InvalidOperation(
            "upsert_tenant_member not supported by this adapter".into(),
        ))
    }

    /// Remove a user's tenant membership. Returns `Ok(true)` if a row was
    /// deleted, `Ok(false)` if no such membership existed.
    ///
    /// The default implementation returns `Ok(false)` so that unmigrated
    /// adapters are explicit about the gap without erroring.
    fn remove_tenant_member(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<bool, AxonError> {
        let _ = (tenant_id, user_id);
        Ok(false)
    }
}

/// Forward all `StorageAdapter` calls through a `Box<dyn StorageAdapter>`.
///
/// This blanket impl allows `Box<dyn StorageAdapter + Send + Sync>` to be used
/// as a `TenantHandler` storage, enabling the HTTP gateway to serve both
/// `SqliteStorageAdapter` and `PostgresStorageAdapter` through the same type.
///
/// Only the seven required methods (no default) are forwarded explicitly.
/// All defaulted methods are also forwarded so that concrete overrides
/// (e.g. `PostgresStorageAdapter`'s dedicated SQL implementations) are
/// dispatched correctly through the vtable rather than silently using the
/// trait's own defaults.
impl StorageAdapter for Box<dyn StorageAdapter + Send + Sync> {
    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        (**self).get(collection, id)
    }
    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        (**self).put(entity)
    }
    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        (**self).delete(collection, id)
    }
    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        (**self).count(collection)
    }
    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        (**self).range_scan(collection, start, end, limit)
    }
    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        (**self).compare_and_swap(entity, expected_version)
    }
    fn create_if_absent(
        &mut self,
        entity: Entity,
        expected_absent_version: u64,
    ) -> Result<Entity, AxonError> {
        (**self).create_if_absent(entity, expected_absent_version)
    }
    fn begin_tx(&mut self) -> Result<(), AxonError> {
        (**self).begin_tx()
    }
    fn commit_tx(&mut self) -> Result<(), AxonError> {
        (**self).commit_tx()
    }
    fn abort_tx(&mut self) -> Result<(), AxonError> {
        (**self).abort_tx()
    }
    fn append_audit_entry(&mut self, entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        (**self).append_audit_entry(entry)
    }
    fn create_database(&mut self, name: &str) -> Result<(), AxonError> {
        (**self).create_database(name)
    }
    fn list_databases(&self) -> Result<Vec<String>, AxonError> {
        (**self).list_databases()
    }
    fn drop_database(&mut self, name: &str) -> Result<(), AxonError> {
        (**self).drop_database(name)
    }
    fn create_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        (**self).create_namespace(namespace)
    }
    fn list_namespaces(&self, database: &str) -> Result<Vec<String>, AxonError> {
        (**self).list_namespaces(database)
    }
    fn drop_namespace(&mut self, namespace: &Namespace) -> Result<(), AxonError> {
        (**self).drop_namespace(namespace)
    }
    fn list_namespace_collections(
        &self,
        namespace: &Namespace,
    ) -> Result<Vec<CollectionId>, AxonError> {
        (**self).list_namespace_collections(namespace)
    }
    fn resolve_collection_key(
        &self,
        collection: &CollectionId,
    ) -> Result<QualifiedCollectionId, AxonError> {
        (**self).resolve_collection_key(collection)
    }
    fn purge_links_for_collections(
        &mut self,
        collections: &[QualifiedCollectionId],
    ) -> Result<(), AxonError> {
        (**self).purge_links_for_collections(collections)
    }
    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        (**self).put_schema(schema)
    }
    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        (**self).get_schema(collection)
    }
    fn get_schema_version(
        &self,
        collection: &CollectionId,
        version: u32,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        (**self).get_schema_version(collection, version)
    }
    fn list_schema_versions(
        &self,
        collection: &CollectionId,
    ) -> Result<Vec<(u32, u64)>, AxonError> {
        (**self).list_schema_versions(collection)
    }
    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        (**self).delete_schema(collection)
    }
    fn put_collection_view(&mut self, view: &CollectionView) -> Result<CollectionView, AxonError> {
        (**self).put_collection_view(view)
    }
    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        (**self).get_collection_view(collection)
    }
    fn delete_collection_view(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        (**self).delete_collection_view(collection)
    }
    fn register_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        (**self).register_collection(collection)
    }
    fn register_collection_in_namespace(
        &mut self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<(), AxonError> {
        (**self).register_collection_in_namespace(collection, namespace)
    }
    fn collection_registered_in_namespace(
        &self,
        collection: &CollectionId,
        namespace: &Namespace,
    ) -> Result<bool, AxonError> {
        (**self).collection_registered_in_namespace(collection, namespace)
    }
    fn unregister_collection(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        (**self).unregister_collection(collection)
    }
    fn list_collections(&self) -> Result<Vec<CollectionId>, AxonError> {
        (**self).list_collections()
    }
    fn collection_numeric_id(&self, collection: &CollectionId) -> Result<Option<u64>, AxonError> {
        (**self).collection_numeric_id(collection)
    }
    fn collection_by_numeric_id(&self, numeric_id: u64) -> Result<Option<CollectionId>, AxonError> {
        (**self).collection_by_numeric_id(numeric_id)
    }
    fn update_indexes(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        old_data: Option<&serde_json::Value>,
        new_data: &serde_json::Value,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        (**self).update_indexes(collection, entity_id, old_data, new_data, indexes)
    }
    fn remove_index_entries(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        data: &serde_json::Value,
        indexes: &[axon_schema::schema::IndexDef],
    ) -> Result<(), AxonError> {
        (**self).remove_index_entries(collection, entity_id, data, indexes)
    }
    fn index_lookup(
        &self,
        collection: &CollectionId,
        field: &str,
        value: &IndexValue,
    ) -> Result<Vec<EntityId>, AxonError> {
        (**self).index_lookup(collection, field, value)
    }
    fn index_range(
        &self,
        collection: &CollectionId,
        field: &str,
        lower: Bound<&IndexValue>,
        upper: Bound<&IndexValue>,
    ) -> Result<Vec<EntityId>, AxonError> {
        (**self).index_range(collection, field, lower, upper)
    }
    fn index_unique_conflict(
        &self,
        collection: &CollectionId,
        field: &str,
        value: &IndexValue,
        exclude_entity: &EntityId,
    ) -> Result<bool, AxonError> {
        (**self).index_unique_conflict(collection, field, value, exclude_entity)
    }
    fn drop_indexes(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        (**self).drop_indexes(collection)
    }
    fn update_compound_indexes(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        old_data: Option<&serde_json::Value>,
        new_data: &serde_json::Value,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        (**self).update_compound_indexes(collection, entity_id, old_data, new_data, indexes)
    }
    fn remove_compound_index_entries(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        data: &serde_json::Value,
        indexes: &[axon_schema::schema::CompoundIndexDef],
    ) -> Result<(), AxonError> {
        (**self).remove_compound_index_entries(collection, entity_id, data, indexes)
    }
    fn compound_index_lookup(
        &self,
        collection: &CollectionId,
        index_idx: usize,
        key: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        (**self).compound_index_lookup(collection, index_idx, key)
    }
    fn compound_index_prefix(
        &self,
        collection: &CollectionId,
        index_idx: usize,
        prefix: &CompoundKey,
    ) -> Result<Vec<EntityId>, AxonError> {
        (**self).compound_index_prefix(collection, index_idx, prefix)
    }
    fn put_link(&mut self, link: &axon_core::types::Link) -> Result<(), AxonError> {
        (**self).put_link(link)
    }
    fn delete_link(
        &mut self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_collection: &CollectionId,
        target_id: &EntityId,
    ) -> Result<(), AxonError> {
        (**self).delete_link(
            source_collection,
            source_id,
            link_type,
            target_collection,
            target_id,
        )
    }
    fn get_link(
        &self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: &str,
        target_collection: &CollectionId,
        target_id: &EntityId,
    ) -> Result<Option<axon_core::types::Link>, AxonError> {
        (**self).get_link(
            source_collection,
            source_id,
            link_type,
            target_collection,
            target_id,
        )
    }
    fn list_outbound_links(
        &self,
        source_collection: &CollectionId,
        source_id: &EntityId,
        link_type: Option<&str>,
    ) -> Result<Vec<axon_core::types::Link>, AxonError> {
        (**self).list_outbound_links(source_collection, source_id, link_type)
    }
    fn list_inbound_links(
        &self,
        target_collection: &CollectionId,
        target_id: &EntityId,
        link_type: Option<&str>,
    ) -> Result<Vec<axon_core::types::Link>, AxonError> {
        (**self).list_inbound_links(target_collection, target_id, link_type)
    }

    fn is_jti_revoked(&self, jti: Uuid) -> Result<bool, AxonError> {
        (**self).is_jti_revoked(jti)
    }

    fn get_user(&self, user_id: UserId) -> Result<Option<User>, AxonError> {
        (**self).get_user(user_id)
    }

    fn get_tenant_member(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<Option<TenantMember>, AxonError> {
        (**self).get_tenant_member(tenant_id, user_id)
    }

    fn upsert_user_identity(
        &self,
        provider: &str,
        external_id: &str,
        display_name: &str,
        email: Option<&str>,
    ) -> Result<User, AxonError> {
        (**self).upsert_user_identity(provider, external_id, display_name, email)
    }

    fn upsert_tenant_member(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
        role: TenantRole,
    ) -> Result<TenantMember, AxonError> {
        (**self).upsert_tenant_member(tenant_id, user_id, role)
    }

    fn remove_tenant_member(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<bool, AxonError> {
        (**self).remove_tenant_member(tenant_id, user_id)
    }
}
