//! INV-001: No Lost Updates workload.
//!
//! Statement: If two concurrent agents update the same entity, exactly one
//! succeeds and the other receives a version conflict error. No acknowledged
//! write is silently overwritten.
//!
//! Simulation: In each round all agents read the same stale version of a shared
//! counter entity and each tries to increment it by a different amount. OCC
//! ensures at most one write succeeds per round. The CHECK phase verifies that
//! the final counter equals exactly the sum of all acknowledged increments.

use axon_api::handler::AxonHandler;
use axon_api::request::{CreateEntityRequest, GetEntityRequest, UpdateEntityRequest};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;

use crate::buggify::Buggify;
use crate::rng::SimRng;

const COL: &str = "sim_counter";
const ENTITY_ID: &str = "shared-counter";

/// Configuration for a concurrent-writer workload run.
#[derive(Debug, Clone)]
pub struct ConcurrentWriterConfig {
    /// Number of simulated agents competing per round.
    pub num_agents: usize,
    /// Number of rounds to execute.
    pub num_rounds: usize,
    /// Maximum per-agent increment value (inclusive; minimum is 1).
    pub max_increment: usize,
    /// Seed for the deterministic PRNG.
    pub seed: u64,
    /// If `true`, BUGGIFY fault injection is active.
    pub buggify: bool,
    /// Activation probability (0–100) for BUGGIFY when enabled.
    pub buggify_pct: u64,
}

impl Default for ConcurrentWriterConfig {
    fn default() -> Self {
        Self {
            num_agents: 3,
            num_rounds: 20,
            max_increment: 10,
            seed: 0xdeadbeef,
            buggify: false,
            buggify_pct: 20,
        }
    }
}

/// Result of a concurrent-writer workload run.
#[derive(Debug)]
pub struct ConcurrentWriterResult {
    /// Seed used for this run.
    pub seed: u64,
    /// Number of rounds in which at least one write was committed.
    pub successful_writes: usize,
    /// Number of write attempts that were rejected (version conflict or BUGGIFY).
    pub contested_writes: usize,
    /// INV-001: final counter == sum of acknowledged increments.
    pub counter_correct: bool,
    /// INV-007: final entity version == 1 (create) + successful_writes.
    pub versions_correct: bool,
}

impl ConcurrentWriterResult {
    /// Returns `true` when both invariants hold.
    pub fn is_correct(&self) -> bool {
        self.counter_correct && self.versions_correct
    }
}

/// Run the concurrent-writer workload and return the result.
pub fn run_concurrent_writer_workload(config: &ConcurrentWriterConfig) -> ConcurrentWriterResult {
    assert!(config.num_agents >= 1, "num_agents must be >= 1");

    let mut rng = SimRng::new(config.seed);
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

    let col = CollectionId::new(COL);
    let entity_id = EntityId::new(ENTITY_ID);

    // ── SETUP ─────────────────────────────────────────────────────────────────
    handler
        .create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: entity_id.clone(),
            data: json!({ "counter": 0_i64 }),
            actor: Some("sim".into()),
            audit_metadata: None,
                attribution: None,
        })
        .expect("counter entity creation must not fail during setup");

    let mut acknowledged_sum: i64 = 0;
    let mut successful_writes: usize = 0;
    let mut contested_writes: usize = 0;

    // ── EXECUTION ─────────────────────────────────────────────────────────────
    for _ in 0..config.num_rounds {
        // All agents read the SAME current state — simulating concurrent reads.
        let current = match handler.get_entity(GetEntityRequest {
            collection: col.clone(),
            id: entity_id.clone(),
        }) {
            Ok(r) => r.entity,
            Err(_) => continue,
        };

        let current_counter = current.data["counter"].as_i64().unwrap_or(0);
        let stale_version = current.version;

        // Each agent picks a random increment and tries to write with the
        // stale version.  All agents run; only the first write succeeds,
        // and the rest receive ConflictingVersion (because the version was
        // bumped by the winning agent).
        let mut round_succeeded = false;
        for _ in 0..config.num_agents {
            let increment = (rng.next_usize(config.max_increment) + 1) as i64;

            // BUGGIFY: maybe abort before writing.
            if config.buggify {
                let mut buggify = Buggify::new(&mut rng).with_activation_pct(config.buggify_pct);
                if buggify.maybe_error().is_err() {
                    contested_writes += 1;
                    continue;
                }
            }

            match handler.update_entity(UpdateEntityRequest {
                collection: col.clone(),
                id: entity_id.clone(),
                data: json!({ "counter": current_counter + increment }),
                expected_version: stale_version,
                actor: Some("sim".into()),
                audit_metadata: None,
                        attribution: None,
            }) {
                Ok(_) => {
                    // Only the first success per round contributes to the
                    // acknowledged sum.  Subsequent agents use the same stale
                    // version and will all fail — see ConflictingVersion arm.
                    if !round_succeeded {
                        acknowledged_sum += increment;
                        successful_writes += 1;
                        round_succeeded = true;
                    }
                }
                Err(AxonError::ConflictingVersion { .. }) => {
                    contested_writes += 1;
                }
                Err(e) => {
                    tracing::warn!("unexpected error in concurrent writer: {e}");
                }
            }
        }
    }

    // ── CHECK ─────────────────────────────────────────────────────────────────
    let final_entity = handler
        .get_entity(GetEntityRequest {
            collection: col.clone(),
            id: entity_id.clone(),
        })
        .expect("final read must succeed")
        .entity;

    let final_counter = final_entity.data["counter"].as_i64().unwrap_or(0);

    // INV-001: no lost updates — counter equals the sum of all acked increments.
    let counter_correct = final_counter == acknowledged_sum;

    // INV-007: version == 1 (initial create) + number of successful updates.
    let versions_correct = final_entity.version == 1 + successful_writes as u64;

    ConcurrentWriterResult {
        seed: config.seed,
        successful_writes,
        contested_writes,
        counter_correct,
        versions_correct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_lost_updates_under_normal_execution() {
        let config = ConcurrentWriterConfig {
            num_agents: 3,
            num_rounds: 20,
            seed: 0xc0ffee,
            buggify: false,
            ..Default::default()
        };
        let result = run_concurrent_writer_workload(&config);
        assert!(
            result.is_correct(),
            "INV-001/007 violated: counter_correct={}, versions_correct={}, \
             successful_writes={}, counter should equal acknowledged sum",
            result.counter_correct,
            result.versions_correct,
            result.successful_writes,
        );
        assert_eq!(
            result.successful_writes, config.num_rounds,
            "with 3 agents, at least one should succeed every round"
        );
    }

    #[test]
    fn concurrent_writers_reject_stale_versions() {
        // With num_agents > 1, all agents beyond the first must fail per round.
        let config = ConcurrentWriterConfig {
            num_agents: 5,
            num_rounds: 10,
            seed: 42,
            buggify: false,
            ..Default::default()
        };
        let result = run_concurrent_writer_workload(&config);
        // Expect exactly num_agents - 1 contested writes per round.
        assert_eq!(
            result.contested_writes,
            (config.num_agents - 1) * config.num_rounds,
            "each round should produce exactly num_agents-1 version conflicts"
        );
        assert!(result.is_correct(), "invariants must hold");
    }

    #[test]
    fn same_seed_produces_identical_execution() {
        let config = ConcurrentWriterConfig {
            num_agents: 2,
            num_rounds: 10,
            seed: 7,
            buggify: false,
            ..Default::default()
        };
        let r1 = run_concurrent_writer_workload(&config);
        let r2 = run_concurrent_writer_workload(&config);
        assert_eq!(r1.successful_writes, r2.successful_writes);
        assert_eq!(r1.contested_writes, r2.contested_writes);
        assert_eq!(r1.counter_correct, r2.counter_correct);
    }

    #[test]
    fn buggify_may_reduce_successful_writes_but_invariants_hold() {
        let config = ConcurrentWriterConfig {
            num_agents: 3,
            num_rounds: 30,
            max_increment: 10,
            seed: 99,
            buggify: true,
            buggify_pct: 40,
        };
        let result = run_concurrent_writer_workload(&config);
        assert!(
            result.is_correct(),
            "INV-001/007 must hold even with BUGGIFY faults"
        );
    }
}
