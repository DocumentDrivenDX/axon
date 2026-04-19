//! INV-004: Audit Immutability workload.
//!
//! Statement: No audit entry is ever modified or deleted through any API path.
//! The audit log is strictly append-only.
//!
//! Workload:
//! 1. Create entities and record a snapshot of all current audit entries
//!    (ID → serialised content).
//! 2. Execute more operations that append new entries.
//! 3. CHECK: every pre-snapshot entry still exists with identical content.
//! 4. CHECK: no pre-snapshot entry was removed.
//! 5. CHECK: new entries were appended (log only grew).

use axon_api::handler::AxonHandler;
use axon_api::request::{CreateEntityRequest, UpdateEntityRequest};
use axon_core::id::{CollectionId, EntityId};
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;

use crate::rng::SimRng;

const COL: &str = "sim_immutable";

/// Configuration for an audit-immutability workload run.
#[derive(Debug, Clone)]
pub struct AuditImmutabilityConfig {
    /// Number of entities to create in the first phase (before snapshot).
    pub num_initial_entities: usize,
    /// Number of additional operations to run after taking the snapshot.
    pub num_post_snapshot_ops: usize,
    /// Seed for the deterministic PRNG.
    pub seed: u64,
}

impl Default for AuditImmutabilityConfig {
    fn default() -> Self {
        Self {
            num_initial_entities: 4,
            num_post_snapshot_ops: 6,
            seed: 0xdeadbeef,
        }
    }
}

/// Result of an audit-immutability workload run.
#[derive(Debug)]
pub struct AuditImmutabilityResult {
    /// Seed used for this run.
    pub seed: u64,
    /// Number of entries recorded in the snapshot.
    pub snapshot_size: usize,
    /// Number of entries in the log after post-snapshot operations.
    pub final_log_size: usize,
    /// INV-004a: no pre-existing entry was removed.
    pub no_entries_removed: bool,
    /// INV-004b: no pre-existing entry was mutated.
    pub no_entries_mutated: bool,
    /// INV-004c: the log only grew (new entries were appended).
    pub log_grew: bool,
}

impl AuditImmutabilityResult {
    /// Returns `true` when all audit-immutability invariants hold.
    pub fn is_correct(&self) -> bool {
        self.no_entries_removed && self.no_entries_mutated && self.log_grew
    }
}

/// Run the audit-immutability workload and return the result.
pub fn run_audit_immutability_workload(
    config: &AuditImmutabilityConfig,
) -> AuditImmutabilityResult {
    assert!(
        config.num_initial_entities >= 1,
        "need at least 1 initial entity"
    );
    assert!(
        config.num_post_snapshot_ops >= 1,
        "need at least 1 post-snapshot operation"
    );

    let mut rng = SimRng::new(config.seed);
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

    let col = CollectionId::new(COL);

    // ── Phase 1: create initial entities ─────────────────────────────────────
    let mut entity_ids: Vec<EntityId> = Vec::new();
    for i in 0..config.num_initial_entities {
        let eid = EntityId::new(format!("e-{i:04}"));
        handler
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: eid.clone(),
                data: json!({ "index": i }),
                actor: Some("sim".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect("initial entity creation must not fail");
        entity_ids.push(eid);
    }

    // ── Snapshot: record each entry's ID and serialised content ──────────────
    let snapshot: Vec<(u64, String)> = handler
        .audit_log()
        .entries()
        .iter()
        .map(|e| {
            (
                e.id,
                serde_json::to_string(e).expect("AuditEntry must serialise"),
            )
        })
        .collect();
    let snapshot_size = snapshot.len();

    // ── Phase 2: post-snapshot operations ─────────────────────────────────────
    for _ in 0..config.num_post_snapshot_ops {
        let idx = rng.next_usize(entity_ids.len());
        let eid = entity_ids[idx].clone();
        if let Ok(resp) = handler.get_entity(axon_api::request::GetEntityRequest {
            collection: col.clone(),
            id: eid.clone(),
        }) {
            let old = resp.entity.data["index"].as_i64().unwrap_or(0);
            let _ = handler.update_entity(UpdateEntityRequest {
                collection: col.clone(),
                id: eid,
                data: json!({ "index": old + 1 }),
                expected_version: resp.entity.version,
                actor: Some("sim".into()),
                audit_metadata: None,
                attribution: None,
            });
        }
    }

    // ── CHECK ─────────────────────────────────────────────────────────────────
    let final_entries = handler.audit_log().entries();
    let final_log_size = final_entries.len();

    // INV-004a & 004b: every snapshot entry must still exist, unchanged.
    let mut no_entries_removed = true;
    let mut no_entries_mutated = true;

    for (snap_id, snap_content) in &snapshot {
        match final_entries.iter().find(|e| e.id == *snap_id) {
            None => {
                no_entries_removed = false;
            }
            Some(e) => {
                let current_content = serde_json::to_string(e).expect("AuditEntry must serialise");
                if &current_content != snap_content {
                    no_entries_mutated = false;
                }
            }
        }
    }

    // INV-004c: the log must have grown.
    let log_grew = final_log_size > snapshot_size;

    AuditImmutabilityResult {
        seed: config.seed,
        snapshot_size,
        final_log_size,
        no_entries_removed,
        no_entries_mutated,
        log_grew,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_entries_are_immutable_after_more_operations() {
        let config = AuditImmutabilityConfig {
            num_initial_entities: 4,
            num_post_snapshot_ops: 8,
            seed: 0xc0ffee,
        };
        let result = run_audit_immutability_workload(&config);
        assert!(
            result.no_entries_removed,
            "INV-004a: pre-existing audit entries must not be removed"
        );
        assert!(
            result.no_entries_mutated,
            "INV-004b: pre-existing audit entries must not be mutated"
        );
        assert!(
            result.log_grew,
            "INV-004c: audit log must grow after post-snapshot operations"
        );
        assert!(
            result.is_correct(),
            "overall audit-immutability check failed"
        );
    }

    #[test]
    fn snapshot_size_equals_initial_entity_count() {
        let config = AuditImmutabilityConfig {
            num_initial_entities: 3,
            num_post_snapshot_ops: 2,
            seed: 1,
        };
        let result = run_audit_immutability_workload(&config);
        assert_eq!(
            result.snapshot_size, config.num_initial_entities,
            "each create should produce exactly one audit entry"
        );
    }

    #[test]
    fn log_size_increases_by_post_ops() {
        let config = AuditImmutabilityConfig {
            num_initial_entities: 2,
            num_post_snapshot_ops: 5,
            seed: 42,
        };
        let result = run_audit_immutability_workload(&config);
        assert_eq!(
            result.final_log_size,
            result.snapshot_size + config.num_post_snapshot_ops,
            "log should grow by exactly the number of post-snapshot updates"
        );
    }
}
