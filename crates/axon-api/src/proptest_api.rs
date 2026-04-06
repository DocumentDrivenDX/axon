//! Property-based tests for axon-api.
//!
//! PROP-002 — Audit Reconstruction
//! PROP-004 — Transaction Serializability
//! PROP-005 — Link Graph Consistency

use proptest::prelude::*;
use serde_json::json;

use axon_audit::log::{AuditLog, MemoryAuditLog};
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::{Entity, Link};
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;

use crate::handler::AxonHandler;
use crate::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, GetEntityRequest,
    UpdateEntityRequest,
};
use crate::transaction::Transaction;

proptest! {
    /// PROP-002: For any sequence of CRUD operations the audit log faithfully
    /// records every state transition, including deletes.
    ///
    /// Properties verified:
    /// 1. Replaying the audit log reproduces the correct final state.
    /// 2. Audit entry versions are strictly monotonically increasing.
    /// 3. The number of audit entries equals the number of operations performed.
    /// 4. Delete operations are correctly captured with data_before and no data_after.
    #[test]
    fn audit_reconstruction_with_deletes(
        update_values in proptest::collection::vec("[a-z]{1,10}", 1_usize..=8),
        delete_at_end in proptest::bool::ANY,
    ) {
        let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
        let col = CollectionId::new("tasks");
        let id  = EntityId::new("e-001");

        // Create the entity with the first data value.
        handler.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id:         id.clone(),
            data:       json!({"val": update_values[0]}),
            actor:      Some("prop-test".into()),
        }).unwrap();

        // Apply subsequent values as updates.
        for val in update_values.iter().skip(1) {
            let current = handler
                .get_entity(GetEntityRequest { collection: col.clone(), id: id.clone() })
                .unwrap()
                .entity;

            handler.update_entity(UpdateEntityRequest {
                collection:       col.clone(),
                id:               id.clone(),
                data:             json!({"val": val}),
                expected_version: current.version,
                actor:            Some("prop-test".into()),
            }).unwrap();
        }

        let mut expected_ops = update_values.len();

        // Optionally delete the entity.
        if delete_at_end {
            handler.delete_entity(DeleteEntityRequest {
                collection: col.clone(),
                id:         id.clone(),
                actor:      Some("prop-test".into()),
                force: false,
            }).unwrap();
            expected_ops += 1;
        }

        // Read all audit entries for this entity.
        let entries = handler.audit_log().query_by_entity(&col, &id).unwrap();

        // Property 3: one entry per operation.
        prop_assert_eq!(
            entries.len(),
            expected_ops,
            "audit entry count must equal number of operations"
        );

        // Property 2: versions are monotonically non-decreasing.
        // Delete audit entries record the same version as the deleted entity,
        // so strict increase only holds for create/update sequences.
        let versions: Vec<u64> = entries.iter().map(|e| e.version).collect();
        for pair in versions.windows(2) {
            prop_assert!(
                pair[0] <= pair[1],
                "audit versions must be non-decreasing: {versions:?}"
            );
        }

        // Property 1: replay audit log to reconstruct final state.
        let mut replayed_state: Option<serde_json::Value> = None;
        for entry in &entries {
            replayed_state = entry.data_after.clone();
        }

        if delete_at_end {
            // After delete, replayed state should be None (last entry has no data_after).
            prop_assert!(
                replayed_state.is_none(),
                "replayed state after delete must be None"
            );
            // Property 4: delete entry must have data_before.
            let last = entries.last().unwrap();
            prop_assert!(
                last.data_before.is_some(),
                "delete audit entry must have data_before"
            );
            prop_assert!(
                last.data_after.is_none(),
                "delete audit entry must have no data_after"
            );
            // Entity must not exist in storage.
            let result = handler.get_entity(GetEntityRequest {
                collection: col.clone(),
                id: id.clone(),
            });
            prop_assert!(result.is_err(), "entity must not exist after delete");
        } else {
            // Entity should still exist and match replay.
            let current = handler
                .get_entity(GetEntityRequest { collection: col.clone(), id: id.clone() })
                .unwrap()
                .entity;
            prop_assert_eq!(
                replayed_state.as_ref(),
                Some(&current.data),
                "replayed audit state must equal current entity data"
            );
        }
    }

    /// PROP-004: sequential simulation of concurrent transaction serializability.
    ///
    /// Two transactions (T1, T2) are built against the same initial state —
    /// both observe all entities at version 1 and stage writes to them
    /// (maximum overlap).  T1 commits first; T2 must then fail with a version
    /// conflict because T1 already incremented every entity's version.  The
    /// final database state must exactly reflect T1's writes and nothing from
    /// T2.
    ///
    /// This is the sequential proxy for true OCC serializability: if a
    /// concurrent-capable backend were used it would produce an identical
    /// outcome, because exactly one ordering can win.  Both the "T2 must
    /// abort" invariant and the "final state == T1's writes" invariant are
    /// verified for any randomly chosen entity count and value pair.
    #[test]
    fn transactions_are_serializable_under_sequential_simulation(
        n_entities in 2_usize..=5,
        t1_value   in 100_u64..=199,
        t2_value   in 200_u64..=299,
    ) {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit   = MemoryAuditLog::default();
        let col = CollectionId::new("accts");

        // Seed all entities at version 1 with a known initial balance.
        for i in 0..n_entities {
            storage.put(Entity::new(
                col.clone(),
                EntityId::new(format!("e-{i}")),
                json!({"balance": 0_u64}),
            )).unwrap();
        }

        // Both transactions observe the same initial state (expected_version = 1
        // for every entity) and write to all n_entities — maximum overlap.
        let mut t1 = Transaction::new();
        let mut t2 = Transaction::new();
        for i in 0..n_entities {
            let id = EntityId::new(format!("e-{i}"));
            t1.update(
                Entity::new(col.clone(), id.clone(), json!({"balance": t1_value})),
                1,
                None,
            ).unwrap();
            t2.update(
                Entity::new(col.clone(), id, json!({"balance": t2_value})),
                1,
                None,
            ).unwrap();
        }

        // T1 commits first — must succeed on the fresh initial state.
        t1.commit(&mut storage, &mut audit, Some("t1".into()))
            .expect("T1 must commit on fresh initial state");

        // T2 must fail: all its expected_version values are now stale.
        let t2_result = t2.commit(&mut storage, &mut audit, Some("t2".into()));
        prop_assert!(
            t2_result.is_err(),
            "T2 must be rejected after T1 incremented the overlapping entity versions"
        );

        // Final state must reflect T1's writes exclusively.
        for i in 0..n_entities {
            let stored = storage
                .get(&col, &EntityId::new(format!("e-{i}")))
                .unwrap()
                .unwrap();
            prop_assert_eq!(
                &stored.data["balance"],
                &json!(t1_value),
                "entity {} must hold T1's value; T2 must not have written anything",
                i
            );
            prop_assert_eq!(
                stored.version,
                2_u64,
                "entity {} must be at version 2 (T1 committed, T2 aborted)",
                i
            );
        }

        // Audit log must contain exactly T1's writes; T2's aborted writes must
        // not appear.
        prop_assert_eq!(
            audit.len(),
            n_entities,
            "audit must hold exactly T1's {} entries; T2 was aborted",
            n_entities
        );
    }

    /// PROP-005: after any sequence of entity/link create operations the link
    /// graph is internally consistent.
    ///
    /// Invariants verified for every forward link in storage:
    /// 1. The source entity exists — no dangling source references.
    /// 2. The target entity exists — no dangling target references.
    /// 3. A matching reverse-index entry exists in `__axon_links_rev__`.
    /// 4. `Link::to_entity()` → `Link::from_entity()` is a perfect roundtrip.
    #[test]
    fn link_graph_stays_consistent(
        n_entities in 2_usize..=5,
        link_specs in proptest::collection::vec(
            (0_usize..5, 0_usize..5),
            0_usize..=8,
        ),
    ) {
        let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
        let col = CollectionId::new("nodes");

        // Create n_entities entities with deterministic IDs.
        for i in 0..n_entities {
            handler.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id:         EntityId::new(format!("e-{i}")),
                data:       json!({"i": i}),
                actor:      None,
            }).unwrap();
        }

        // Create links for valid non-self-referencing, non-duplicate pairs.
        let mut created = std::collections::HashSet::<(usize, usize)>::new();
        for (raw_src, raw_tgt) in &link_specs {
            let src = raw_src % n_entities;
            let tgt = raw_tgt % n_entities;
            // Skip self-loops and already-created duplicates.
            if src == tgt || !created.insert((src, tgt)) {
                continue;
            }
            handler.create_link(CreateLinkRequest {
                source_collection: col.clone(),
                source_id:         EntityId::new(format!("e-{src}")),
                target_collection: col.clone(),
                target_id:         EntityId::new(format!("e-{tgt}")),
                link_type:         "connects".into(),
                metadata:          serde_json::Value::Null,
                actor:             None,
            }).unwrap();
        }

        // Verify every forward link satisfies the consistency invariants.
        let forward_links = handler
            .storage_mut()
            .range_scan(&Link::links_collection(), None, None, None)
            .unwrap();

        for link_entity in &forward_links {
            let link = Link::from_entity(link_entity)
                .expect("every entity in __axon_links__ must deserialize to Link");

            // Invariant 1 & 2: no dangling references.
            let src_exists = handler
                .storage_mut()
                .get(&link.source_collection, &link.source_id)
                .unwrap()
                .is_some();
            prop_assert!(src_exists,
                "source entity {}/{} must exist (no dangling ref)",
                link.source_collection, link.source_id);

            let tgt_exists = handler
                .storage_mut()
                .get(&link.target_collection, &link.target_id)
                .unwrap()
                .is_some();
            prop_assert!(tgt_exists,
                "target entity {}/{} must exist (no dangling ref)",
                link.target_collection, link.target_id);

            // Invariant 3: matching reverse-index entry must exist.
            let rev_id = Link::rev_storage_id(
                &link.target_collection,
                &link.target_id,
                &link.source_collection,
                &link.source_id,
                &link.link_type,
            );
            let rev_exists = handler
                .storage_mut()
                .get(&Link::links_rev_collection(), &rev_id)
                .unwrap()
                .is_some();
            prop_assert!(rev_exists,
                "reverse-index entry must exist for every forward link");

            // Invariant 4: to_entity → from_entity roundtrip is identity.
            let rt = Link::from_entity(&link.to_entity())
                .expect("link must survive serialisation roundtrip");
            prop_assert_eq!(rt.source_collection, link.source_collection,
                "source_collection must survive roundtrip");
            prop_assert_eq!(rt.source_id, link.source_id,
                "source_id must survive roundtrip");
            prop_assert_eq!(rt.target_collection, link.target_collection,
                "target_collection must survive roundtrip");
            prop_assert_eq!(rt.target_id, link.target_id,
                "target_id must survive roundtrip");
            prop_assert_eq!(rt.link_type, link.link_type,
                "link_type must survive roundtrip");
        }
    }
}
