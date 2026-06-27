//! INV-009: Write-skew prevention under Serializable isolation.
//!
//! Statement: Snapshot isolation permits *write skew* — two transactions that
//! read disjoint records and write into each other's read set can both commit,
//! violating an invariant that spans the records. Serializable isolation
//! (key-addressed read-set validation; FEAT-008 TXN-05 / ADR-004 / B-104)
//! prevents it: the second committer's recorded read is stale, so it aborts.
//!
//! Workload: two "guard" entities with the invariant **"at least one guard is
//! active"**. Each round both guards are (re)activated so the invariant sits on
//! a knife's edge, then two transactions each read the OTHER guard (observing it
//! active) and deactivate themselves. Under [`IsolationLevel::Snapshot`] both
//! commit and the invariant drops to zero active guards; under
//! [`IsolationLevel::Serializable`] the second transaction aborts and the
//! invariant always holds.
//!
//! This is a sequential simulation (the `MemoryStorageAdapter` is `&mut self`,
//! so two transactions cannot be live at once), but the SI-vs-Serializable
//! outcome is identical to a concurrent backend because exactly one commit
//! ordering can win.

use axon_api::handler::AxonHandler;
use axon_api::request::{CreateEntityRequest, GetEntityRequest, UpdateEntityRequest};
use axon_api::transaction::{IsolationLevel, Transaction};
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;

const COL: &str = "sim_guards";

/// Configuration for a write-skew workload run.
#[derive(Debug, Clone)]
pub struct WriteSkewConfig {
    /// Number of write-skew rounds to execute.
    pub num_rounds: usize,
    /// Seed recorded with the result (the workload itself is deterministic).
    pub seed: u64,
    /// Isolation level the write-skew transactions run at.
    pub isolation: IsolationLevel,
}

impl Default for WriteSkewConfig {
    fn default() -> Self {
        Self {
            num_rounds: 20,
            seed: 0xdeadbeef,
            isolation: IsolationLevel::Serializable,
        }
    }
}

/// Result of a write-skew workload run.
#[derive(Debug)]
pub struct WriteSkewResult {
    /// Seed used for this run.
    pub seed: u64,
    /// Rounds where the second transaction was rejected (skew prevented).
    pub skews_prevented: usize,
    /// Rounds where both transactions committed (skew occurred — SI only).
    pub skews_occurred: usize,
    /// INV-009: at least one guard remained active in every round.
    pub invariant_preserved: bool,
}

impl WriteSkewResult {
    /// Returns `true` when the spanning invariant held in every round.
    pub fn is_correct(&self) -> bool {
        self.invariant_preserved
    }
}

fn guard_id(i: usize) -> EntityId {
    EntityId::new(format!("guard-{i}"))
}

/// Run the write-skew workload at the configured isolation level.
pub fn run_write_skew_workload(config: &WriteSkewConfig) -> WriteSkewResult {
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
    let col = CollectionId::new(COL);

    // ── SETUP: two active guards ───────────────────────────────────────────────
    for i in 0..2 {
        handler
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: guard_id(i),
                data: json!({ "active": 1 }),
                actor: Some("sim".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect("guard creation must not fail during setup");
    }

    let mut skews_prevented = 0;
    let mut skews_occurred = 0;
    let mut invariant_preserved = true;

    for _ in 0..config.num_rounds {
        // Reactivate any deactivated guard so each round starts from the
        // at-risk state: exactly two active guards.
        for i in 0..2 {
            let cur = handler
                .get_entity(GetEntityRequest {
                    collection: col.clone(),
                    id: guard_id(i),
                })
                .expect("guard read must succeed")
                .entity;
            if cur.data["active"].as_i64().unwrap_or(0) != 1 {
                handler
                    .update_entity(UpdateEntityRequest {
                        collection: col.clone(),
                        id: guard_id(i),
                        data: json!({ "active": 1 }),
                        expected_version: cur.version,
                        actor: Some("sim".into()),
                        audit_metadata: None,
                        attribution: None,
                    })
                    .expect("guard reactivation must succeed");
            }
        }

        // Both transactions observe the SAME pre-round snapshot.
        let g0 = handler
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: guard_id(0),
            })
            .expect("read g0")
            .entity;
        let g1 = handler
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: guard_id(1),
            })
            .expect("read g1")
            .entity;

        // T_a reads the OTHER guard (g1, active) then deactivates g0.
        let mut ta = Transaction::with_isolation(config.isolation);
        ta.record_read(col.clone(), guard_id(1), g1.version)
            .expect("record read within MAX_READS");
        ta.update(
            Entity::new(col.clone(), guard_id(0), json!({ "active": 0 })),
            g0.version,
            Some(g0.data.clone()),
        )
        .expect("stage g0 deactivation");
        let first_committed = handler
            .commit_transaction(ta, Some("ta".into()), None)
            .is_ok();

        // T_b read g0 from the same snapshot (before T_a committed) and
        // deactivates g1. Its recorded read of g0 is now stale.
        let mut tb = Transaction::with_isolation(config.isolation);
        tb.record_read(col.clone(), guard_id(0), g0.version)
            .expect("record read within MAX_READS");
        tb.update(
            Entity::new(col.clone(), guard_id(1), json!({ "active": 0 })),
            g1.version,
            Some(g1.data.clone()),
        )
        .expect("stage g1 deactivation");
        let second_committed = handler
            .commit_transaction(tb, Some("tb".into()), None)
            .is_ok();

        if first_committed && second_committed {
            skews_occurred += 1;
        } else {
            skews_prevented += 1;
        }

        // ── CHECK: at least one guard must still be active ───────────────────
        let a0 = active_count(&handler, &col, 0);
        let a1 = active_count(&handler, &col, 1);
        if a0 + a1 < 1 {
            invariant_preserved = false;
        }
    }

    WriteSkewResult {
        seed: config.seed,
        skews_prevented,
        skews_occurred,
        invariant_preserved,
    }
}

fn active_count(handler: &AxonHandler<MemoryStorageAdapter>, col: &CollectionId, i: usize) -> i64 {
    handler
        .get_entity(GetEntityRequest {
            collection: col.clone(),
            id: guard_id(i),
        })
        .expect("guard read must succeed")
        .entity
        .data["active"]
        .as_i64()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializable_prevents_write_skew() {
        let config = WriteSkewConfig {
            num_rounds: 25,
            seed: 0xc0ffee,
            isolation: IsolationLevel::Serializable,
        };
        let result = run_write_skew_workload(&config);
        assert!(
            result.is_correct(),
            "serializable isolation must preserve the spanning invariant"
        );
        assert_eq!(
            result.skews_occurred, 0,
            "no write skew may occur under serializable isolation"
        );
        assert_eq!(
            result.skews_prevented, config.num_rounds,
            "every round's second transaction must be rejected"
        );
    }

    #[test]
    fn snapshot_isolation_allows_write_skew() {
        let config = WriteSkewConfig {
            num_rounds: 25,
            seed: 0xc0ffee,
            isolation: IsolationLevel::Snapshot,
        };
        let result = run_write_skew_workload(&config);
        // This documents the SI gap: write skew occurs and the invariant breaks.
        assert!(
            result.skews_occurred > 0,
            "snapshot isolation must allow write skew"
        );
        assert!(
            !result.invariant_preserved,
            "the spanning invariant is violated under snapshot isolation"
        );
    }

    #[test]
    fn same_seed_produces_identical_execution() {
        let config = WriteSkewConfig {
            num_rounds: 10,
            seed: 42,
            isolation: IsolationLevel::Serializable,
        };
        let r1 = run_write_skew_workload(&config);
        let r2 = run_write_skew_workload(&config);
        assert_eq!(r1.skews_prevented, r2.skews_prevented);
        assert_eq!(r1.skews_occurred, r2.skews_occurred);
        assert_eq!(r1.invariant_preserved, r2.invariant_preserved);
    }
}
