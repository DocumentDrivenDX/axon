//! INV-003: Audit Completeness workload.
//!
//! Statement: Every committed mutation (entity create, update, delete) has a
//! corresponding audit entry. There are no gaps — if a mutation is visible in
//! storage, its audit entry exists.
//!
//! Workload:
//! - Execute mixed CRUD operations across multiple entities.
//! - SETUP → EXECUTION → CHECK
//!
//! CHECK invariants:
//! 1. count(mutations attempted and succeeded) == count(audit entries for those entities)
//! 2. For each entity that still exists, replaying its audit log reconstructs
//!    exactly the current stored state.
//! 3. INV-007: each entity's audit entries show versions 1, 2, 3, … with no
//!    gaps or repeats (create/update/revert strictly monotone; delete records
//!    the version at deletion time).

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateEntityRequest, DeleteEntityRequest, GetEntityRequest, UpdateEntityRequest,
};
use axon_audit::entry::MutationType;
use axon_core::id::{CollectionId, EntityId};
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::{json, Value};

use crate::rng::SimRng;

const COL_A: &str = "sim_audit_a";
const COL_B: &str = "sim_audit_b";

/// Configuration for an audit-completeness workload run.
#[derive(Debug, Clone)]
pub struct AuditCompletenessConfig {
    /// Number of entities to create initially.
    pub num_entities: usize,
    /// Number of update rounds to perform after initial creation.
    pub num_update_rounds: usize,
    /// Seed for the deterministic PRNG.
    pub seed: u64,
}

impl Default for AuditCompletenessConfig {
    fn default() -> Self {
        Self {
            num_entities: 5,
            num_update_rounds: 10,
            seed: 0xdeadbeef,
        }
    }
}

/// Result of an audit-completeness workload run.
#[derive(Debug)]
pub struct AuditCompletenessResult {
    /// Seed used for this run.
    pub seed: u64,
    /// Total mutations successfully applied.
    pub mutations_applied: usize,
    /// INV-003a: audit entry count matches mutation count.
    pub entry_count_correct: bool,
    /// INV-003b: replaying audit log reconstructs current entity state.
    pub reconstruction_correct: bool,
    /// INV-007: all entity version sequences are strictly monotone.
    pub version_monotone: bool,
}

impl AuditCompletenessResult {
    /// Returns `true` when all audit-completeness invariants hold.
    pub fn is_correct(&self) -> bool {
        self.entry_count_correct && self.reconstruction_correct && self.version_monotone
    }
}

/// Run the audit-completeness workload and return the result.
pub fn run_audit_completeness_workload(
    config: &AuditCompletenessConfig,
) -> AuditCompletenessResult {
    assert!(config.num_entities >= 2, "need at least 2 entities");

    let mut rng = SimRng::new(config.seed);
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

    let col_a = CollectionId::new(COL_A);
    let col_b = CollectionId::new(COL_B);

    // Track entity IDs we create (for later update/delete rounds).
    let mut entity_ids_a: Vec<EntityId> = Vec::new();
    let mut entity_ids_b: Vec<EntityId> = Vec::new();
    let mut mutations_applied: usize = 0;

    // ── SETUP: create initial entities in both collections ────────────────────
    let half = config.num_entities / 2;
    for i in 0..config.num_entities {
        let (col, ids) = if i < half {
            (&col_a, &mut entity_ids_a)
        } else {
            (&col_b, &mut entity_ids_b)
        };
        let eid = EntityId::new(format!("e-{i:04}"));
        handler
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: eid.clone(),
                data: json!({ "index": i, "value": i * 10 }),
                actor: Some("sim".into()),
            })
            .expect("entity creation must not fail during setup");
        ids.push(eid);
        mutations_applied += 1;
    }

    // ── EXECUTION: random updates and one delete per collection ───────────────
    for round in 0..config.num_update_rounds {
        // Update a random entity in col_a.
        if !entity_ids_a.is_empty() {
            let idx = rng.next_usize(entity_ids_a.len());
            let eid = entity_ids_a[idx].clone();
            if let Ok(resp) = handler.get_entity(GetEntityRequest {
                collection: col_a.clone(),
                id: eid.clone(),
            }) {
                let old_value = resp.entity.data["value"].as_i64().unwrap_or(0);
                if handler
                    .update_entity(UpdateEntityRequest {
                        collection: col_a.clone(),
                        id: eid.clone(),
                        data: json!({ "index": idx, "value": old_value + 1 }),
                        expected_version: resp.entity.version,
                        actor: Some("sim".into()),
                    })
                    .is_ok()
                {
                    mutations_applied += 1;
                }
            }
        }

        // Update a random entity in col_b.
        if !entity_ids_b.is_empty() {
            let idx = rng.next_usize(entity_ids_b.len());
            let eid = entity_ids_b[idx].clone();
            if let Ok(resp) = handler.get_entity(GetEntityRequest {
                collection: col_b.clone(),
                id: eid.clone(),
            }) {
                let old_value = resp.entity.data["value"].as_i64().unwrap_or(0);
                if handler
                    .update_entity(UpdateEntityRequest {
                        collection: col_b.clone(),
                        id: eid.clone(),
                        data: json!({ "index": idx, "value": old_value + 1 }),
                        expected_version: resp.entity.version,
                        actor: Some("sim".into()),
                    })
                    .is_ok()
                {
                    mutations_applied += 1;
                }
            }
        }

        // On even rounds, delete one entity from col_b (last one to keep it simple).
        if round % 4 == 3 && !entity_ids_b.is_empty() {
            let eid = entity_ids_b.last().unwrap().clone();
            if handler
                .delete_entity(DeleteEntityRequest {
                    collection: col_b.clone(),
                    id: eid.clone(),
                    actor: Some("sim".into()),
                })
                .is_ok()
            {
                entity_ids_b.pop();
                mutations_applied += 1;
            }
        }
    }

    // ── CHECK ─────────────────────────────────────────────────────────────────

    let all_entries = handler.audit_log().entries();

    // INV-003a: count entity mutations in the audit log.
    let audited_entity_mutations = all_entries
        .iter()
        .filter(|e| {
            matches!(
                e.mutation,
                MutationType::EntityCreate
                    | MutationType::EntityUpdate
                    | MutationType::EntityDelete
            )
        })
        .count();
    let entry_count_correct = audited_entity_mutations == mutations_applied;

    // INV-003b: for each surviving entity, replay its audit log and compare to
    // the currently stored value.
    let mut reconstruction_correct = true;
    let mut version_monotone = true;

    let all_col_ids = [(&col_a, &entity_ids_a), (&col_b, &entity_ids_b)];

    for (col, ids) in &all_col_ids {
        for eid in *ids {
            // Collect audit entries for this entity.
            let entity_entries: Vec<_> = all_entries
                .iter()
                .filter(|e| &e.collection == *col && &e.entity_id == eid)
                .collect();

            // INV-007: version monotonicity.
            if !check_version_monotonicity(&entity_entries) {
                version_monotone = false;
            }

            // INV-003b: reconstruct state.
            let reconstructed = reconstruct_state(&entity_entries);
            match handler.get_entity(GetEntityRequest {
                collection: (*col).clone(),
                id: eid.clone(),
            }) {
                Ok(resp) => {
                    if reconstructed.as_ref() != Some(&resp.entity.data) {
                        reconstruction_correct = false;
                    }
                }
                Err(_) => {
                    // Entity no longer exists; reconstructed state should be None.
                    if reconstructed.is_some() {
                        reconstruction_correct = false;
                    }
                }
            }
        }
    }

    AuditCompletenessResult {
        seed: config.seed,
        mutations_applied,
        entry_count_correct,
        reconstruction_correct,
        version_monotone,
    }
}

/// Reconstruct the current entity state by replaying its audit entries in order.
///
/// Returns the expected current `data` value, or `None` if the entity was
/// deleted.
fn reconstruct_state(entries: &[&axon_audit::entry::AuditEntry]) -> Option<Value> {
    let mut state: Option<Value> = None;
    for entry in entries {
        match entry.mutation {
            MutationType::EntityCreate
            | MutationType::EntityUpdate
            | MutationType::EntityRevert => {
                state = entry.data_after.clone();
            }
            MutationType::EntityDelete => {
                state = None;
            }
            _ => {}
        }
    }
    state
}

/// INV-007: verify that create/update/revert audit entries have strictly
/// increasing versions starting at 1, and that any delete entry records the
/// last non-delete version.
fn check_version_monotonicity(entries: &[&axon_audit::entry::AuditEntry]) -> bool {
    let mut expected = 1u64;
    for entry in entries {
        match entry.mutation {
            MutationType::EntityCreate
            | MutationType::EntityUpdate
            | MutationType::EntityRevert => {
                if entry.version != expected {
                    return false;
                }
                expected += 1;
            }
            MutationType::EntityDelete => {
                // Delete records the version at deletion — must equal the last
                // non-delete version (expected - 1 after at least one write).
                if expected == 1 || entry.version != expected - 1 {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_completeness_holds_under_mixed_crud() {
        let config = AuditCompletenessConfig {
            num_entities: 6,
            num_update_rounds: 12,
            seed: 0xc0ffee,
        };
        let result = run_audit_completeness_workload(&config);
        assert!(
            result.entry_count_correct,
            "INV-003a: audited mutations ({}) != applied mutations ({})",
            result.mutations_applied, result.mutations_applied
        );
        assert!(
            result.reconstruction_correct,
            "INV-003b: audit-log reconstruction does not match stored state"
        );
        assert!(
            result.version_monotone,
            "INV-007: entity version sequence is not strictly monotone"
        );
        assert!(result.is_correct(), "overall result must be correct");
    }

    #[test]
    fn same_seed_produces_identical_result() {
        let config = AuditCompletenessConfig {
            num_entities: 4,
            num_update_rounds: 8,
            seed: 42,
        };
        let r1 = run_audit_completeness_workload(&config);
        let r2 = run_audit_completeness_workload(&config);
        assert_eq!(r1.mutations_applied, r2.mutations_applied);
        assert_eq!(r1.entry_count_correct, r2.entry_count_correct);
        assert_eq!(r1.reconstruction_correct, r2.reconstruction_correct);
    }
}
