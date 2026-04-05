use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;

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
}
