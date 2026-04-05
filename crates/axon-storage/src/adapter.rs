use axon_audit::entry::AuditEntry;
use axon_core::error::AxonError;
use axon_core::id::CollectionId;
use axon_core::id::EntityId;
use axon_core::types::Entity;
use axon_schema::schema::CollectionSchema;

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
}
