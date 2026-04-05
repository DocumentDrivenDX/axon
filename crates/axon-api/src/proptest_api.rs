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
    CreateEntityRequest, CreateLinkRequest, GetEntityRequest, UpdateEntityRequest,
};
use crate::transaction::Transaction;

proptest! {
    /// PROP-002: For any sequence of entity mutations the audit log faithfully
    /// records every state transition.
    ///
    /// Property verified:
    /// 1. The last audit entry's `data_after` equals the current stored entity data.
    /// 2. Audit entry versions are strictly monotonically increasing.
    /// 3. The number of audit entries equals the number of operations performed.
    #[test]
    fn audit_last_state_matches_current_entity(
        data_values in proptest::collection::vec("[a-z]{1,10}", 1_usize..=10),
    ) {
        let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
        let col = CollectionId::new("tasks");
        let id  = EntityId::new("e-001");

        // Create the entity with the first data value.
        handler.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id:         id.clone(),
            data:       json!({"val": data_values[0]}),
            actor:      Some("prop-test".into()),
        }).unwrap();

        // Apply subsequent values as updates.
        for val in data_values.iter().skip(1) {
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

        // Read the current stored entity.
        let current = handler
            .get_entity(GetEntityRequest { collection: col.clone(), id: id.clone() })
            .unwrap()
            .entity;

        // Read all audit entries for this entity.
        let entries = handler.audit_log().query_by_entity(&col, &id).unwrap();

        prop_assert!(!entries.is_empty(), "audit log must have at least one entry");

        // Property 1: last entry's data_after == current stored data.
        let last = entries.last().unwrap();
        prop_assert_eq!(
            last.data_after.as_ref(),
            Some(&current.data),
            "last audit entry data_after must equal current entity data"
        );

        // Property 2: versions are strictly monotonically increasing.
        let versions: Vec<u64> = entries.iter().map(|e| e.version).collect();
        for pair in versions.windows(2) {
            prop_assert!(
                pair[0] < pair[1],
                "audit versions must be strictly increasing: {versions:?}"
            );
        }

        // Property 3: one entry per operation.
        prop_assert_eq!(
            entries.len(),
            data_values.len(),
            "audit entry count must equal number of operations"
        );
    }

    /// PROP-004: a committed multi-entity transaction is atomic — either every
    /// staged write succeeds or none do.
    ///
    /// Test structure:
    /// - Seed `n_entities` entities, all at version 1.
    /// - Build a transaction that updates all of them.
    /// - When `corrupt_idx` is `Some(i)`, entity `i % n_entities` is given an
    ///   incorrect expected version (99), forcing the entire transaction to abort.
    /// - When `corrupt_idx` is `None`, all versions are correct and every write
    ///   must be committed.
    #[test]
    fn transaction_atomically_commits_or_aborts(
        n_entities  in 2_usize..=5,
        corrupt_idx in proptest::option::of(0_usize..5),
    ) {
        let mut storage = MemoryStorageAdapter::default();
        let mut audit   = MemoryAuditLog::default();
        let col = CollectionId::new("accts");

        // Seed all entities at version 1 with initial balance values.
        for i in 0..n_entities {
            storage.put(Entity::new(
                col.clone(),
                EntityId::new(format!("e-{i}")),
                json!({"balance": 100 + i}),
            )).unwrap();
        }

        // Determine which entity (if any) receives a stale version.
        let bad_entity = corrupt_idx.map(|c| c % n_entities);

        let mut tx = Transaction::new();
        for i in 0..n_entities {
            let id = EntityId::new(format!("e-{i}"));
            let expected_version = if bad_entity == Some(i) { 99 } else { 1 };
            tx.update(
                Entity::new(col.clone(), id, json!({"balance": 200 + i})),
                expected_version,
                None,
            );
        }

        let result = tx.commit(&mut storage, &mut audit, Some("prop-test".into()));

        if bad_entity.is_some() {
            // Transaction must fail — no entity must be modified.
            prop_assert!(result.is_err(), "transaction with stale version must fail");
            prop_assert_eq!(audit.len(), 0,
                "no audit entries must be written after a failed transaction");

            for i in 0..n_entities {
                let stored = storage
                    .get(&col, &EntityId::new(format!("e-{i}")))
                    .unwrap()
                    .unwrap();
                prop_assert_eq!(&stored.data["balance"], &json!(100 + i),
                    "entity {} must be unmodified after aborted transaction", i);
                prop_assert_eq!(stored.version, 1_u64,
                    "entity {} version must be unchanged after aborted transaction", i);
            }
        } else {
            // Transaction must succeed — all entities must be updated.
            result.expect("transaction with all-correct versions must succeed");
            prop_assert_eq!(audit.len(), n_entities,
                "audit must record one entry per committed write");

            for i in 0..n_entities {
                let stored = storage
                    .get(&col, &EntityId::new(format!("e-{i}")))
                    .unwrap()
                    .unwrap();
                prop_assert_eq!(&stored.data["balance"], &json!(200 + i),
                    "entity {} must be updated after successful transaction", i);
                prop_assert_eq!(stored.version, 2_u64,
                    "entity {} version must be 2 after one committed update", i);
            }
        }
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
