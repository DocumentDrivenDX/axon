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
    fn put(&mut self, entity: Entity) -> Result<(), AxonError>;

    /// Deletes an entity. Returns `Ok(())` whether or not the entity existed.
    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError>;

    /// Returns the number of entities in the given collection.
    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError>;
}
