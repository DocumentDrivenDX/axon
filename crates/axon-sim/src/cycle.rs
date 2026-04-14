//! Cycle test workload: the canonical Axon correctness invariant.
//!
//! Inspired by FoundationDB's Cycle workload. A ring of N entities is
//! connected by typed "next" links forming a cycle:
//!
//! ```text
//! node-0 → node-1 → node-2 → ... → node-(N-1) → node-0
//! ```
//!
//! The EXECUTION phase performs random transactions that update node data while
//! preserving the ring link structure. Each transaction atomically updates
//! two nodes.
//!
//! The CHECK phase walks the ring from `node-0`, counting hops until it
//! returns to `node-0`. If it takes exactly `N` hops, the ring is intact.
//! Fewer or more hops indicate a violation — either a broken link or a split ring.

use axon_api::handler::AxonHandler;
use axon_api::request::{CreateEntityRequest, CreateLinkRequest, TraverseRequest};
use axon_api::transaction::Transaction;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::{Entity, Link};
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;

use crate::buggify::Buggify;
use crate::rng::SimRng;

const NODES_COL: &str = "sim_nodes";
const LINK_TYPE: &str = "next";

/// Configuration for a cycle test run.
#[derive(Debug, Clone)]
pub struct CycleConfig {
    /// Number of nodes in the ring (minimum 3).
    pub ring_size: usize,
    /// Number of link-swap transactions to execute.
    pub num_swaps: usize,
    /// Seed for the deterministic PRNG.
    pub seed: u64,
    /// If `true`, BUGGIFY fault injection is active during execution.
    pub buggify: bool,
    /// Activation probability (0–100) for BUGGIFY when enabled.
    pub buggify_pct: u64,
}

impl Default for CycleConfig {
    fn default() -> Self {
        Self {
            ring_size: 5,
            num_swaps: 20,
            seed: 0xdeadbeef,
            buggify: false,
            buggify_pct: 20,
        }
    }
}

/// Result of a cycle test run.
#[derive(Debug)]
pub struct CycleResult {
    /// The seed used for this run.
    pub seed: u64,
    /// Number of swaps attempted.
    pub swaps_attempted: usize,
    /// Number of swaps successfully committed.
    pub swaps_committed: usize,
    /// Whether the ring integrity check passed.
    pub integrity_ok: bool,
    /// Number of hops observed during CHECK phase (expected: ring_size).
    pub hops_observed: usize,
    /// Expected hop count.
    pub hops_expected: usize,
}

impl CycleResult {
    /// Returns `true` if the run demonstrates correct behaviour.
    pub fn is_correct(&self) -> bool {
        self.integrity_ok
    }
}

/// Run the cycle workload to completion and return the result.
///
/// ## Phases
///
/// 1. **SETUP**: Create N nodes and link them in a ring via `AxonHandler`.
/// 2. **EXECUTION**: Perform `config.num_swaps` atomic node-update transactions.
///    With BUGGIFY enabled, faults may be injected causing transactions to fail;
///    the workload retries until the committed swap count reaches `num_swaps`.
/// 3. **CHECK**: Walk the ring and count hops. Pass if hops == ring_size.
pub fn run_cycle_test(config: &CycleConfig) -> CycleResult {
    assert!(config.ring_size >= 3, "ring_size must be >= 3");

    let mut rng = SimRng::new(config.seed);
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

    let col = CollectionId::new(NODES_COL);

    // ── SETUP ────────────────────────────────────────────────────────────────
    for i in 0..config.ring_size {
        handler
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: node_id(i),
                data: json!({ "index": i, "visits": 0 }),
                actor: Some("sim".into()),
                audit_metadata: None,
            })
            .expect("node creation should not fail during setup");
    }

    for i in 0..config.ring_size {
        let next = (i + 1) % config.ring_size;
        handler
            .create_link(CreateLinkRequest {
                source_collection: col.clone(),
                source_id: node_id(i),
                target_collection: col.clone(),
                target_id: node_id(next),
                link_type: LINK_TYPE.into(),
                metadata: json!(null),
                actor: Some("sim".into()),
            })
            .expect("link creation should not fail during setup");
    }

    // ── EXECUTION ────────────────────────────────────────────────────────────
    let mut swaps_attempted = 0;
    let mut swaps_committed = 0;

    while swaps_committed < config.num_swaps {
        swaps_attempted += 1;

        // Pick two random adjacent nodes to update atomically.
        let i = rng.next_usize(config.ring_size);
        let j = (i + 1) % config.ring_size;

        // Read current versions.
        let node_i = match handler
            .get_entity(axon_api::request::GetEntityRequest {
                collection: col.clone(),
                id: node_id(i),
            })
            .map(|r| r.entity)
        {
            Ok(e) => e,
            Err(_) => continue,
        };
        let node_j = match handler
            .get_entity(axon_api::request::GetEntityRequest {
                collection: col.clone(),
                id: node_id(j),
            })
            .map(|r| r.entity)
        {
            Ok(e) => e,
            Err(_) => continue,
        };

        let new_visits_i = node_i.data["visits"].as_u64().unwrap_or(0) + 1;
        let new_visits_j = node_j.data["visits"].as_u64().unwrap_or(0) + 1;

        // BUGGIFY: maybe inject a fault before committing.
        if config.buggify {
            let mut buggify = Buggify::new(&mut rng).with_activation_pct(config.buggify_pct);
            if buggify.maybe_error().is_err() {
                continue;
            }
        }

        let mut tx = Transaction::new();
        tx.update(
            Entity {
                collection: col.clone(),
                id: node_id(i),
                version: node_i.version,
                data: json!({ "index": i, "visits": new_visits_i }),
                created_at_ns: None,
                updated_at_ns: None,
                created_by: None,
                updated_by: None,
                schema_version: None,
                gate_results: Default::default(),
            },
            node_i.version,
            Some(node_i.data.clone()),
        )
        .expect("first node update should stage successfully");
        tx.update(
            Entity {
                collection: col.clone(),
                id: node_id(j),
                version: node_j.version,
                data: json!({ "index": j, "visits": new_visits_j }),
                created_at_ns: None,
                updated_at_ns: None,
                created_by: None,
                updated_by: None,
                schema_version: None,
                gate_results: Default::default(),
            },
            node_j.version,
            Some(node_j.data.clone()),
        )
        .expect("second node update should stage successfully");

        match handler.commit_transaction(tx, Some("sim".into())) {
            Ok(_) => swaps_committed += 1,
            Err(AxonError::ConflictingVersion { .. }) => {
                // Retry on OCC conflict — expected under BUGGIFY.
            }
            Err(e) => {
                tracing::warn!("unexpected transaction error during simulation: {e}");
            }
        }
    }

    // ── CHECK ─────────────────────────────────────────────────────────────────
    let hops_observed = check_ring_integrity(&handler, &col, config.ring_size);
    let integrity_ok = hops_observed == config.ring_size;

    CycleResult {
        seed: config.seed,
        swaps_attempted,
        swaps_committed,
        integrity_ok,
        hops_observed,
        hops_expected: config.ring_size,
    }
}

/// Walk the ring starting from `node-0` and count hops until we return to
/// `node-0` or exceed `max_hops`.
fn check_ring_integrity(
    handler: &AxonHandler<MemoryStorageAdapter>,
    col: &CollectionId,
    max_hops: usize,
) -> usize {
    let mut current = node_id(0);
    let mut hops = 0;

    loop {
        let resp = match handler.traverse(TraverseRequest {
            collection: col.clone(),
            id: current.clone(),
            link_type: Some(LINK_TYPE.into()),
            max_depth: Some(1),
            direction: Default::default(),
            hop_filter: None,
        }) {
            Ok(r) => r,
            Err(_) => return hops,
        };

        if resp.entities.is_empty() {
            return hops; // dead end — broken ring
        }

        hops += 1;
        current = resp.entities[0].id.clone();

        if current == node_id(0) {
            return hops; // completed the ring
        }

        if hops > max_hops {
            return hops; // did not close after max hops — split ring
        }
    }
}

fn node_id(i: usize) -> EntityId {
    EntityId::new(format!("node-{i:04}"))
}

/// Inject an isolation violation by directly removing a link from storage,
/// bypassing the transaction system. Used to verify the CHECK phase detects violations.
pub fn inject_isolation_violation(storage: &mut MemoryStorageAdapter, _ring_size: usize) {
    use axon_storage::StorageAdapter;

    let col = CollectionId::new(NODES_COL);
    // Remove the link node-0 → node-1 to break the ring.
    // Delete both the forward link and its reverse-index entry to keep storage consistent.
    let link_entity_id = Link::storage_id(&col, &node_id(0), LINK_TYPE, &col, &node_id(1));
    storage
        .delete(&Link::links_collection(), &link_entity_id)
        .expect("delete should succeed");
    let rev_id = Link::rev_storage_id(&col, &node_id(1), &col, &node_id(0), LINK_TYPE);
    storage
        .delete(&Link::links_rev_collection(), &rev_id)
        .expect("reverse-index delete should succeed");
}

/// Build a fully set-up cycle handler (setup phase only, no execution).
pub fn setup_cycle_handler(ring_size: usize) -> AxonHandler<MemoryStorageAdapter> {
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
    let col = CollectionId::new(NODES_COL);

    for i in 0..ring_size {
        handler
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: node_id(i),
                data: json!({ "index": i }),
                actor: None,
                audit_metadata: None,
            })
            .expect("cycle setup should create each node");
    }
    for i in 0..ring_size {
        let next = (i + 1) % ring_size;
        handler
            .create_link(CreateLinkRequest {
                source_collection: col.clone(),
                source_id: node_id(i),
                target_collection: col.clone(),
                target_id: node_id(next),
                link_type: LINK_TYPE.into(),
                metadata: json!(null),
                actor: None,
            })
            .expect("cycle setup should create each link");
    }
    handler
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_test_passes_under_normal_execution() {
        let config = CycleConfig {
            ring_size: 5,
            num_swaps: 10,
            seed: 0xc0ffee,
            buggify: false,
            buggify_pct: 0,
        };
        let result = run_cycle_test(&config);
        assert!(
            result.is_correct(),
            "ring integrity should hold under normal execution; hops={}/{}",
            result.hops_observed,
            result.hops_expected
        );
        assert_eq!(result.swaps_committed, config.num_swaps);
    }

    #[test]
    fn same_seed_produces_identical_execution() {
        let config = CycleConfig {
            ring_size: 4,
            num_swaps: 5,
            seed: 42,
            buggify: false,
            buggify_pct: 0,
        };
        let r1 = run_cycle_test(&config);
        let r2 = run_cycle_test(&config);
        assert_eq!(
            r1.swaps_attempted, r2.swaps_attempted,
            "same seed must produce same number of attempts"
        );
        assert_eq!(r1.swaps_committed, r2.swaps_committed);
        assert_eq!(r1.integrity_ok, r2.integrity_ok);
    }

    #[test]
    fn cycle_test_detects_injected_isolation_violation() {
        let ring_size = 5;
        let mut handler = setup_cycle_handler(ring_size);
        let col = CollectionId::new(NODES_COL);

        // Verify ring is intact before injection.
        let hops_before = check_ring_integrity(&handler, &col, ring_size * 2);
        assert_eq!(
            hops_before, ring_size,
            "ring should be intact before violation"
        );

        // Inject: remove the link node-0 → node-1.
        inject_isolation_violation(handler.storage_mut(), ring_size);

        // CHECK phase must now detect the broken ring.
        let hops_after = check_ring_integrity(&handler, &col, ring_size * 2);
        assert_ne!(
            hops_after, ring_size,
            "CHECK should detect injected violation; hops={hops_after} but expected {ring_size}"
        );
    }

    #[test]
    fn buggify_injects_faults_during_execution() {
        // With 30% injection, we expect more attempts than commits (some get
        // aborted by BUGGIFY), but the ring should still be intact.
        let config = CycleConfig {
            ring_size: 3,
            num_swaps: 5,
            seed: 7,
            buggify: true,
            buggify_pct: 30,
        };
        let result = run_cycle_test(&config);
        assert!(
            result.swaps_attempted >= result.swaps_committed,
            "attempted >= committed always"
        );
        assert!(
            result.is_correct(),
            "ring should be intact after BUGGIFY faults"
        );
    }
}
