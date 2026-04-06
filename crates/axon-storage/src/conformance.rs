//! StorageAdapter conformance test suite (L4).
//!
//! Every StorageAdapter implementation must pass the identical test suite.
//! Tests are written against the trait, parameterized by backend.
//!
//! Usage:
//! ```ignore
//! storage_conformance_tests!(memory_backend, || MemoryStorageAdapter::default());
//! ```

/// Generates the full StorageAdapter conformance test suite for a given backend.
///
/// `$mod_name` becomes the test module name, and `$make_adapter` is an
/// expression that produces a fresh `impl StorageAdapter` instance.
#[macro_export]
macro_rules! storage_conformance_tests {
    ($mod_name:ident, $make_adapter:expr) => {
        #[cfg(test)]
        mod $mod_name {
            use super::*;
            use $crate::adapter::StorageAdapter;
            use axon_core::error::AxonError;
            use axon_core::id::{CollectionId, EntityId};
            use axon_core::types::Entity;
            use axon_schema::schema::CollectionSchema;
            use serde_json::json;

            fn tasks() -> CollectionId {
                CollectionId::new("tasks")
            }

            fn entity(id: &str, title: &str) -> Entity {
                Entity::new(tasks(), EntityId::new(id), json!({"title": title}))
            }

            fn store() -> impl StorageAdapter {
                $make_adapter
            }

            // ── Core entity operations ──────────────────────────────────

            #[test]
            fn get_missing_returns_none() {
                let s = store();
                let result = s.get(&tasks(), &EntityId::new("ghost")).unwrap();
                assert!(result.is_none());
            }

            #[test]
            fn put_then_get_roundtrip() {
                let mut s = store();
                let e = entity("t-001", "hello");
                s.put(e.clone()).unwrap();
                let got = s.get(&tasks(), &EntityId::new("t-001")).unwrap().unwrap();
                assert_eq!(got.data["title"], "hello");
                assert_eq!(got.version, 1);
            }

            #[test]
            fn count_reflects_puts_and_deletes() {
                let mut s = store();
                assert_eq!(s.count(&tasks()).unwrap(), 0);
                s.put(entity("a", "a")).unwrap();
                s.put(entity("b", "b")).unwrap();
                assert_eq!(s.count(&tasks()).unwrap(), 2);
                s.delete(&tasks(), &EntityId::new("a")).unwrap();
                assert_eq!(s.count(&tasks()).unwrap(), 1);
            }

            #[test]
            fn delete_missing_is_ok() {
                let mut s = store();
                s.delete(&tasks(), &EntityId::new("ghost")).unwrap();
            }

            #[test]
            fn range_scan_returns_sorted_entities() {
                let mut s = store();
                s.put(entity("c", "c")).unwrap();
                s.put(entity("a", "a")).unwrap();
                s.put(entity("b", "b")).unwrap();
                let result = s.range_scan(&tasks(), None, None, None).unwrap();
                let ids: Vec<_> = result.iter().map(|e| e.id.as_str()).collect();
                assert_eq!(ids, vec!["a", "b", "c"]);
            }

            #[test]
            fn range_scan_respects_start_end_and_limit() {
                let mut s = store();
                for id in ["a", "b", "c", "d", "e"] {
                    s.put(entity(id, id)).unwrap();
                }
                let result = s
                    .range_scan(
                        &tasks(),
                        Some(&EntityId::new("b")),
                        Some(&EntityId::new("d")),
                        Some(2),
                    )
                    .unwrap();
                let ids: Vec<_> = result.iter().map(|e| e.id.as_str()).collect();
                assert_eq!(ids, vec!["b", "c"]);
            }

            // ── Compare and swap (OCC) ──────────────────────────────────

            #[test]
            fn compare_and_swap_increments_version() {
                let mut s = store();
                s.put(entity("t-001", "v1")).unwrap();
                let updated = s
                    .compare_and_swap(entity("t-001", "v2"), 1)
                    .unwrap();
                assert_eq!(updated.version, 2);
                assert_eq!(updated.data["title"], "v2");
            }

            #[test]
            fn compare_and_swap_rejects_wrong_version() {
                let mut s = store();
                s.put(entity("t-001", "v1")).unwrap();
                let err = s.compare_and_swap(entity("t-001", "v2"), 99).unwrap_err();
                assert!(
                    matches!(err, AxonError::ConflictingVersion { expected: 99, actual: 1, .. }),
                    "expected ConflictingVersion, got: {err}"
                );
                // Entity should be unchanged.
                let got = s.get(&tasks(), &EntityId::new("t-001")).unwrap().unwrap();
                assert_eq!(got.data["title"], "v1");
            }

            #[test]
            fn compare_and_swap_rejects_missing_entity() {
                let mut s = store();
                let err = s.compare_and_swap(entity("ghost", "v1"), 1).unwrap_err();
                assert!(
                    matches!(err, AxonError::ConflictingVersion { .. }),
                    "expected ConflictingVersion, got: {err}"
                );
            }

            // ── Transaction control ─────────────────────────────────────

            #[test]
            fn begin_commit_tx_persists_writes() {
                let mut s = store();
                s.begin_tx().unwrap();
                s.put(entity("t-001", "hello")).unwrap();
                s.commit_tx().unwrap();
                let got = s.get(&tasks(), &EntityId::new("t-001")).unwrap();
                assert!(got.is_some());
            }

            #[test]
            fn abort_tx_rolls_back_writes() {
                let mut s = store();
                s.put(entity("t-001", "original")).unwrap();
                s.begin_tx().unwrap();
                s.put(Entity::new(tasks(), EntityId::new("t-001"), json!({"title": "modified"}))).unwrap();
                s.put(entity("t-002", "new")).unwrap();
                s.abort_tx().unwrap();
                let got = s.get(&tasks(), &EntityId::new("t-001")).unwrap().unwrap();
                assert_eq!(got.data["title"], "original", "abort should restore original");
                assert!(s.get(&tasks(), &EntityId::new("t-002")).unwrap().is_none(), "abort should remove new entity");
            }

            #[test]
            fn begin_tx_rejects_nested_begin() {
                let mut s = store();
                s.begin_tx().unwrap();
                let err = s.begin_tx().unwrap_err();
                assert!(
                    matches!(err, AxonError::Storage(_) | AxonError::InvalidOperation(_)),
                    "expected Storage or InvalidOperation, got: {err}"
                );
                s.abort_tx().unwrap();
            }

            #[test]
            fn commit_tx_requires_active_transaction() {
                let mut s = store();
                let err = s.commit_tx().unwrap_err();
                assert!(
                    matches!(err, AxonError::Storage(_) | AxonError::InvalidOperation(_)),
                    "expected Storage or InvalidOperation, got: {err}"
                );
            }

            #[test]
            fn abort_tx_without_active_tx_is_noop() {
                let mut s = store();
                // Should not error.
                s.abort_tx().unwrap();
            }

            // ── Schema persistence ──────────────────────────────────────

            #[test]
            fn put_get_schema_roundtrip() {
                let mut s = store();
                let col = CollectionId::new("widgets");
                let schema = CollectionSchema {
                    collection: col.clone(),
                    description: Some("test schema".into()),
                    version: 99, // ignored — auto-increment assigns v1
                    entity_schema: Some(json!({"type": "object"})),
                    link_types: Default::default(),
                };
                s.put_schema(&schema).unwrap();
                let got = s.get_schema(&col).unwrap().unwrap();
                assert_eq!(got.collection, col);
                assert_eq!(got.version, 1); // auto-incremented
                assert_eq!(got.description.as_deref(), Some("test schema"));
            }

            #[test]
            fn get_schema_missing_returns_none() {
                let s = store();
                assert!(s.get_schema(&CollectionId::new("ghost")).unwrap().is_none());
            }

            #[test]
            fn put_schema_overwrites_previous() {
                let mut s = store();
                let col = CollectionId::new("items");
                let v1 = CollectionSchema {
                    collection: col.clone(),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                };
                let v2 = CollectionSchema {
                    collection: col.clone(),
                    description: Some("v2".into()),
                    version: 2,
                    entity_schema: None,
                    link_types: Default::default(),
                };
                s.put_schema(&v1).unwrap();
                s.put_schema(&v2).unwrap();
                let got = s.get_schema(&col).unwrap().unwrap();
                assert_eq!(got.version, 2);
                assert_eq!(got.description.as_deref(), Some("v2"));
            }

            #[test]
            fn delete_schema_removes_it() {
                let mut s = store();
                let col = CollectionId::new("tmp");
                let schema = CollectionSchema {
                    collection: col.clone(),
                    description: None,
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                };
                s.put_schema(&schema).unwrap();
                assert!(s.get_schema(&col).unwrap().is_some());
                s.delete_schema(&col).unwrap();
                assert!(s.get_schema(&col).unwrap().is_none());
            }

            #[test]
            fn abort_tx_rolls_back_schema_changes() {
                let mut s = store();
                let col = CollectionId::new("items");
                let original = CollectionSchema {
                    collection: col.clone(),
                    description: Some("v1".into()),
                    version: 1,
                    entity_schema: None,
                    link_types: Default::default(),
                };
                s.put_schema(&original).unwrap();

                s.begin_tx().unwrap();
                s.put_schema(&CollectionSchema {
                    collection: col.clone(),
                    description: Some("v2".into()),
                    version: 2,
                    entity_schema: None,
                    link_types: Default::default(),
                })
                .unwrap();
                s.abort_tx().unwrap();

                let got = s.get_schema(&col).unwrap().unwrap();
                assert_eq!(got.version, 1, "abort should restore original schema");
            }

            // ── Collection registry ─────────────────────────────────────

            #[test]
            fn register_and_list_collections() {
                let mut s = store();
                s.register_collection(&CollectionId::new("alpha")).unwrap();
                s.register_collection(&CollectionId::new("beta")).unwrap();
                let list = s.list_collections().unwrap();
                let names: Vec<_> = list.iter().map(|c| c.as_str()).collect();
                assert!(names.contains(&"alpha"));
                assert!(names.contains(&"beta"));
            }

            #[test]
            fn unregister_collection_removes_it() {
                let mut s = store();
                let col = CollectionId::new("temp");
                s.register_collection(&col).unwrap();
                assert!(s.list_collections().unwrap().contains(&col));
                s.unregister_collection(&col).unwrap();
                assert!(!s.list_collections().unwrap().contains(&col));
            }
        }
    };
}
