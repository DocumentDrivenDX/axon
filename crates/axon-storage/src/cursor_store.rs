//! Durable, storage-backed CDC cursor persistence (CONTRACT-006 §Cursor
//! semantics; FEAT-032).
//!
//! [`StorageCursorStore`] persists CDC cursors as entities in a reserved system
//! collection so they survive process restarts — unlike
//! [`axon_audit::MemoryCursorStore`], which is in-memory only. A local read
//! replica (FEAT-032) or any CDC sink can therefore resume from the last
//! durable offset after a restart.

use axon_audit::cursor::CdcCursorStore;
use axon_core::id::{CollectionId, EntityId, SystemCollection};
use axon_core::types::Entity;
use serde_json::json;

use crate::adapter::StorageAdapter;

/// Reserved system collection that holds CDC cursor checkpoints.
pub const CDC_CURSORS_COLLECTION: &str = "_cdc_cursors";

/// A durable [`CdcCursorStore`] backed by a [`StorageAdapter`].
///
/// Each `(sink, collection)` cursor is stored as an entity in
/// [`CDC_CURSORS_COLLECTION`], keyed by a unit-separator-joined id, with the
/// `audit_id` in its `data`. Because cursors live in the same durable storage as
/// entity data, a producer restart resumes from the last persisted offset.
///
/// **Failure semantics.** The [`CdcCursorStore`] trait's `set` is infallible, so
/// if persistence fails [`set`](StorageCursorStore::set) logs a warning and
/// leaves the cursor unadvanced. The producer then re-emits from the last
/// durable offset, which CONTRACT-006's at-least-once delivery + dedup-by-
/// `audit_id` contract tolerates. A persistence failure never silently advances
/// the cursor past unpersisted progress.
pub struct StorageCursorStore<S: StorageAdapter> {
    storage: S,
}

impl<S: StorageAdapter> StorageCursorStore<S> {
    /// Wrap a storage adapter as a durable cursor store. Cursors persist into
    /// the same storage, so they survive a restart that reopens the adapter.
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    /// Consume the store and return the underlying storage adapter.
    pub fn into_inner(self) -> S {
        self.storage
    }

    /// Borrow the underlying storage adapter.
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Mutably borrow the underlying storage adapter.
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    fn collection() -> CollectionId {
        SystemCollection::cdc_cursors().collection_id()
    }

    fn cursor_id(sink_name: &str, collection: &str) -> EntityId {
        // U+001F (Unit Separator) is not valid in sink or collection names, so
        // the joined key is unambiguous.
        EntityId::new(format!("{sink_name}\u{1f}{collection}"))
    }
}

impl<S: StorageAdapter> CdcCursorStore for StorageCursorStore<S> {
    fn get(&self, sink_name: &str, collection: &str) -> Option<u64> {
        self.storage
            .get(&Self::collection(), &Self::cursor_id(sink_name, collection))
            .ok()
            .flatten()
            .and_then(|e| e.data.get("audit_id").and_then(serde_json::Value::as_u64))
    }

    fn set(&mut self, sink_name: &str, collection: &str, audit_id: u64) {
        let entity = Entity::new(
            Self::collection(),
            Self::cursor_id(sink_name, collection),
            json!({ "sink": sink_name, "collection": collection, "audit_id": audit_id }),
        );
        if let Err(e) = self.storage.put(entity) {
            tracing::warn!(
                sink = sink_name,
                collection,
                audit_id,
                "failed to persist CDC cursor; leaving it unadvanced \
                 (at-least-once + dedup recovers): {e}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryStorageAdapter;

    #[test]
    fn empty_store_returns_none() {
        let store = StorageCursorStore::new(MemoryStorageAdapter::default());
        assert!(store.get("kafka", "tasks").is_none());
    }

    #[test]
    fn set_then_get_round_trip() {
        let mut store = StorageCursorStore::new(MemoryStorageAdapter::default());
        store.set("kafka", "tasks", 42);
        assert_eq!(store.get("kafka", "tasks"), Some(42));
    }

    #[test]
    fn set_overwrites_previous() {
        let mut store = StorageCursorStore::new(MemoryStorageAdapter::default());
        store.set("kafka", "tasks", 10);
        store.set("kafka", "tasks", 20);
        assert_eq!(store.get("kafka", "tasks"), Some(20));
    }

    #[test]
    fn keys_are_independent_across_sink_and_collection() {
        let mut store = StorageCursorStore::new(MemoryStorageAdapter::default());
        store.set("kafka", "tasks", 100);
        store.set("kafka", "users", 200);
        store.set("file", "tasks", 50);
        store.set("sse", "", 99); // global cursor uses empty collection

        assert_eq!(store.get("kafka", "tasks"), Some(100));
        assert_eq!(store.get("kafka", "users"), Some(200));
        assert_eq!(store.get("file", "tasks"), Some(50));
        assert_eq!(store.get("sse", ""), Some(99));
        assert!(store.get("file", "users").is_none());
        assert!(store.get("sse", "tasks").is_none());
    }

    /// The durability contract: a cursor set, then the store dropped, is still
    /// readable when a fresh store is reconstructed over the same storage.
    #[test]
    fn cursor_survives_store_drop_and_reconstruction() {
        let mut store = StorageCursorStore::new(MemoryStorageAdapter::default());
        store.set("kafka", "tasks", 7);
        store.set("kafka", "users", 9);

        // Drop the store, recovering the underlying storage (the "durable
        // medium" — for a real DB this is the same database on reopen).
        let storage = store.into_inner();

        // A brand-new store over the same storage sees the persisted cursors.
        let reopened = StorageCursorStore::new(storage);
        assert_eq!(
            reopened.get("kafka", "tasks"),
            Some(7),
            "cursor must survive store drop + reconstruction"
        );
        assert_eq!(reopened.get("kafka", "users"), Some(9));
    }
}
