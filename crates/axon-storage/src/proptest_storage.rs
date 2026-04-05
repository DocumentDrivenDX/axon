//! PROP-003 — OCC Linearizability property-based tests.
//!
//! Property: For any sequence of concurrent read-then-write operations on a
//! single entity, the final state is consistent with some serial ordering of
//! the successful writes.  No acknowledged write is lost.
//!
//! Because `MemoryStorageAdapter` requires `&mut self` for all writes, true
//! concurrency cannot occur within a single test.  Instead we verify the
//! serial OCC invariants:
//!
//! - Sequential writes with the correct expected version always succeed and
//!   produce monotonically increasing versions.
//! - A write that supplies a stale (wrong) expected version always fails with
//!   `ConflictingVersion` and leaves the stored entity untouched.

use proptest::prelude::*;
use serde_json::json;

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;

use crate::adapter::StorageAdapter;
use crate::memory::MemoryStorageAdapter;

proptest! {
    /// PROP-003a: sequential CAS operations with the correct version always
    /// succeed and the entity version increments monotonically.
    ///
    /// Seeds an entity at version 1 then repeatedly reads the current version
    /// and calls `compare_and_swap` with it.  Every write must succeed and the
    /// resulting version must equal `previous_version + 1`.
    #[test]
    fn occ_sequential_writes_produce_monotonic_versions(n_updates in 1_usize..20) {
        let mut store = MemoryStorageAdapter::default();
        let col = CollectionId::new("entities");
        let id  = EntityId::new("e-001");

        // Seed the entity at version 1.
        store.put(Entity::new(col.clone(), id.clone(), json!({"v": 0}))).unwrap();

        for i in 1..=n_updates {
            let current = store.get(&col, &id).unwrap().unwrap();
            prop_assert_eq!(current.version, i as u64,
                "before update {}: expected stored version {}", i, i);

            let candidate = Entity {
                collection: col.clone(),
                id:         id.clone(),
                version:    current.version,
                data:       json!({"v": i}),
            created_at_ns: None,
            updated_at_ns: None,
            created_by: None,
            updated_by: None,
            };
            let updated = store.compare_and_swap(candidate, current.version)
                .expect("CAS with correct version must always succeed");

            prop_assert_eq!(updated.version, (i as u64) + 1,
                "after update {}: version must be {}", i, i + 1);
        }

        let final_entity = store.get(&col, &id).unwrap().unwrap();
        prop_assert_eq!(final_entity.version, (n_updates as u64) + 1,
            "final version must be n_updates + 1");
        prop_assert_eq!(&final_entity.data["v"], &json!(n_updates),
            "final data must reflect the last write");
    }

    /// PROP-003b: a CAS attempt with a stale version always fails with
    /// `ConflictingVersion` and leaves the stored entity untouched.
    ///
    /// Inserts an entity at version 1 then attempts a write supplying a version
    /// that is strictly greater than 1.  The stored entity must be unchanged
    /// and the error must carry the correct `actual` version and the
    /// `current_entity` snapshot.
    #[test]
    fn occ_stale_version_always_fails(
        initial_tag   in 0_u64..1000,
        version_delta in 1_u64..50,
    ) {
        let mut store = MemoryStorageAdapter::default();
        let col = CollectionId::new("entities");
        let id  = EntityId::new("e-001");

        store.put(Entity {
            collection: col.clone(),
            id:         id.clone(),
            version:    1,
            data:       json!({"tag": initial_tag}),
        created_at_ns: None,
        updated_at_ns: None,
        created_by: None,
        updated_by: None,
        }).unwrap();

        let wrong_version = 1 + version_delta; // always > 1
        let result = store.compare_and_swap(
            Entity {
                collection: col.clone(),
                id:         id.clone(),
                version:    wrong_version,
                data:       json!({"tag": "should-not-persist"}),
            created_at_ns: None,
            updated_at_ns: None,
            created_by: None,
            updated_by: None,
            },
            wrong_version,
        );

        match result {
            Err(AxonError::ConflictingVersion { expected, actual, current_entity }) => {
                prop_assert_eq!(expected, wrong_version,
                    "error must echo the expected version we passed");
                prop_assert_eq!(actual, 1_u64,
                    "error must report the true stored version");
                let ce = current_entity.expect("current_entity must be Some");
                prop_assert_eq!(ce.version, 1_u64,
                    "current_entity must reflect stored version");
                prop_assert_eq!(&ce.data["tag"], &json!(initial_tag),
                    "current_entity data must match stored data");
            }
            other => prop_assert!(
                false,
                "expected ConflictingVersion, got {other:?}"
            ),
        }

        // Stored entity must be unchanged.
        let stored = store.get(&col, &id).unwrap().unwrap();
        prop_assert_eq!(stored.version, 1_u64, "stored version must be unchanged");
        prop_assert_eq!(&stored.data["tag"], &json!(initial_tag),
            "stored data must be unchanged after failed CAS");
    }
}
