//! CDC cursor persistence — CONTRACT-006 §Cursor semantics.
//!
//! Provides the [`CdcCursorStore`] trait and [`MemoryCursorStore`] for tracking
//! the last emitted `audit_id` per `(sink_name, collection)` pair so that CDC
//! producers can resume from a stable offset after restart.

use std::collections::HashMap;

/// Persists the last emitted `audit_id` per CDC sink and collection.
///
/// CONTRACT-006 §Cursor semantics — `_cdc_cursors` table. Each `(sink_name,
/// collection)` pair has an independent cursor so producers that tail multiple
/// collections can advance their progress independently.
///
/// The collection key is the [`axon_core::id::CollectionId`] string (e.g.
/// `"tasks"`, `"prod.default.invoices"`). Use the empty string `""` as the
/// collection key when the cursor covers all collections (no collection filter).
pub trait CdcCursorStore: Send {
    /// Returns the last emitted `audit_id` for `(sink_name, collection)`, or
    /// `None` if no cursor has been stored yet (i.e. start from the beginning).
    fn get(&self, sink_name: &str, collection: &str) -> Option<u64>;

    /// Advances the cursor for `(sink_name, collection)` to `audit_id`.
    ///
    /// Must only be called after the event at `audit_id` has been successfully
    /// emitted to the sink. Implementations SHOULD be durable so that a
    /// producer restart does not re-emit events before the stored cursor.
    fn set(&mut self, sink_name: &str, collection: &str, audit_id: u64);
}

/// In-memory cursor store for testing and single-process embedded mode.
///
/// Not durable across process restarts. Use a persistent backend (e.g. a
/// storage adapter) in production.
#[derive(Debug, Default)]
pub struct MemoryCursorStore {
    cursors: HashMap<(String, String), u64>,
}

impl MemoryCursorStore {
    /// Returns the number of distinct `(sink, collection)` cursors stored.
    pub fn len(&self) -> usize {
        self.cursors.len()
    }

    /// Returns `true` if no cursors have been stored.
    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty()
    }
}

impl CdcCursorStore for MemoryCursorStore {
    fn get(&self, sink_name: &str, collection: &str) -> Option<u64> {
        self.cursors
            .get(&(sink_name.to_string(), collection.to_string()))
            .copied()
    }

    fn set(&mut self, sink_name: &str, collection: &str, audit_id: u64) {
        self.cursors
            .insert((sink_name.to_string(), collection.to_string()), audit_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_cursor_store_empty_returns_none() {
        let store = MemoryCursorStore::default();
        assert!(store.get("kafka", "tasks").is_none());
    }

    #[test]
    fn memory_cursor_store_set_then_get_round_trip() {
        let mut store = MemoryCursorStore::default();
        store.set("kafka", "tasks", 42);
        assert_eq!(store.get("kafka", "tasks"), Some(42));
    }

    #[test]
    fn memory_cursor_store_set_overwrites_previous() {
        let mut store = MemoryCursorStore::default();
        store.set("kafka", "tasks", 10);
        store.set("kafka", "tasks", 20);
        assert_eq!(store.get("kafka", "tasks"), Some(20));
    }

    #[test]
    fn memory_cursor_store_keys_are_independent() {
        let mut store = MemoryCursorStore::default();
        store.set("kafka", "tasks", 100);
        store.set("kafka", "users", 200);
        store.set("file", "tasks", 50);

        assert_eq!(store.get("kafka", "tasks"), Some(100));
        assert_eq!(store.get("kafka", "users"), Some(200));
        assert_eq!(store.get("file", "tasks"), Some(50));
        assert!(store.get("file", "users").is_none());
    }

    #[test]
    fn memory_cursor_store_global_cursor_uses_empty_collection() {
        let mut store = MemoryCursorStore::default();
        store.set("sse", "", 99);
        assert_eq!(store.get("sse", ""), Some(99));
        assert!(store.get("sse", "tasks").is_none());
    }

    #[test]
    fn memory_cursor_store_len_tracks_distinct_keys() {
        let mut store = MemoryCursorStore::default();
        assert!(store.is_empty());
        store.set("kafka", "tasks", 1);
        assert_eq!(store.len(), 1);
        store.set("kafka", "tasks", 2);
        assert_eq!(store.len(), 1, "overwrite does not add a new key");
        store.set("kafka", "users", 3);
        assert_eq!(store.len(), 2);
    }
}
