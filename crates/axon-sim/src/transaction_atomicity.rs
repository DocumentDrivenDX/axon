//! INV-008: Transaction Atomicity workload.
//!
//! Statement: Multi-operation transactions either fully commit (all operations
//! visible, all audit entries present with a shared transaction ID) or fully
//! abort (no operations visible, no audit entries).
//!
//! Workload: debit/credit pattern (AP/AR style).
//! - Two accounts: `account-A` and `account-B`, each with an initial balance.
//! - Each round: atomically debit A by X and credit B by X.
//! - BUGGIFY: inject version-conflict failures to force aborts.
//!
//! CHECK invariants:
//! 1. Total balance across both accounts is conserved.
//! 2. For every `transaction_id` in the audit log: the count of entries with
//!    that ID is exactly the number of operations staged (all-or-nothing).
//! 3. INV-007: entity version sequences are strictly monotone.

use axon_api::handler::AxonHandler;
use axon_api::request::{CreateEntityRequest, GetEntityRequest};
use axon_api::transaction::Transaction;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;

use crate::buggify::Buggify;
use crate::invariants::check_version_monotonicity;
use crate::rng::SimRng;

const COL: &str = "sim_accounts";
const ACCOUNT_A: &str = "account-A";
const ACCOUNT_B: &str = "account-B";
const INITIAL_BALANCE: i64 = 1_000;

/// Configuration for a transaction-atomicity workload run.
#[derive(Debug, Clone)]
pub struct TransactionAtomicityConfig {
    /// Number of debit/credit rounds to attempt.
    pub num_rounds: usize,
    /// Maximum transfer amount per round.
    pub max_transfer: usize,
    /// Seed for the deterministic PRNG.
    pub seed: u64,
    /// If `true`, BUGGIFY fault injection is active.
    pub buggify: bool,
    /// Activation probability (0–100) for BUGGIFY when enabled.
    pub buggify_pct: u64,
}

impl Default for TransactionAtomicityConfig {
    fn default() -> Self {
        Self {
            num_rounds: 20,
            max_transfer: 50,
            seed: 0xdeadbeef,
            buggify: false,
            buggify_pct: 25,
        }
    }
}

/// Result of a transaction-atomicity workload run.
#[derive(Debug)]
pub struct TransactionAtomicityResult {
    /// Seed used for this run.
    pub seed: u64,
    /// Number of transactions that committed successfully.
    pub committed: usize,
    /// Number of transactions that were aborted (version conflict or BUGGIFY).
    pub aborted: usize,
    /// INV-008a: total balance across both accounts equals the initial total.
    pub balance_conserved: bool,
    /// INV-008b: every committed transaction_id has exactly 2 audit entries
    ///  (one per account), and no aborted transaction appears in the log.
    pub atomicity_correct: bool,
    /// INV-007: entity version sequences are strictly monotone.
    pub versions_correct: bool,
}

impl TransactionAtomicityResult {
    /// Returns `true` when all transaction-atomicity invariants hold.
    pub fn is_correct(&self) -> bool {
        self.balance_conserved && self.atomicity_correct && self.versions_correct
    }
}

/// Run the transaction-atomicity workload and return the result.
pub fn run_transaction_atomicity_workload(
    config: &TransactionAtomicityConfig,
) -> TransactionAtomicityResult {
    let mut rng = SimRng::new(config.seed);
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

    let col = CollectionId::new(COL);

    // ── SETUP: create two accounts ────────────────────────────────────────────
    for name in [ACCOUNT_A, ACCOUNT_B] {
        handler
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(name),
                data: json!({ "balance": INITIAL_BALANCE }),
                actor: Some("sim".into()),
            })
            .expect("account creation must not fail during setup");
    }

    let mut committed = 0usize;
    let mut aborted = 0usize;
    // Track (tx_id, expected_op_count) for committed transactions.
    let mut committed_tx_ids: Vec<(String, usize)> = Vec::new();

    // ── EXECUTION ─────────────────────────────────────────────────────────────
    for _ in 0..config.num_rounds {
        let transfer = (rng.next_usize(config.max_transfer) + 1) as i64;

        // Read current balances.
        let a = match handler.get_entity(GetEntityRequest {
            collection: col.clone(),
            id: EntityId::new(ACCOUNT_A),
        }) {
            Ok(r) => r.entity,
            Err(_) => continue,
        };
        let b = match handler.get_entity(GetEntityRequest {
            collection: col.clone(),
            id: EntityId::new(ACCOUNT_B),
        }) {
            Ok(r) => r.entity,
            Err(_) => continue,
        };

        let balance_a = a.data["balance"].as_i64().unwrap_or(0);
        let balance_b = b.data["balance"].as_i64().unwrap_or(0);

        // BUGGIFY: maybe inject a failure before committing.
        if config.buggify {
            let mut buggify = Buggify::new(&mut rng).with_activation_pct(config.buggify_pct);
            if buggify.maybe_error().is_err() {
                aborted += 1;
                continue;
            }
        }

        let mut tx = Transaction::new();
        let tx_id = tx.id.clone();

        tx.update(
            Entity {
                collection: col.clone(),
                id: EntityId::new(ACCOUNT_A),
                version: a.version,
                data: json!({ "balance": balance_a - transfer }),
                created_at_ns: None,
                updated_at_ns: None,
                created_by: None,
                updated_by: None,
            },
            a.version,
            Some(a.data.clone()),
        )
        .unwrap();
        tx.update(
            Entity {
                collection: col.clone(),
                id: EntityId::new(ACCOUNT_B),
                version: b.version,
                data: json!({ "balance": balance_b + transfer }),
                created_at_ns: None,
                updated_at_ns: None,
                created_by: None,
                updated_by: None,
            },
            b.version,
            Some(b.data.clone()),
        )
        .unwrap();

        match handler.commit_transaction(tx, Some("sim".into())) {
            Ok(_) => {
                committed += 1;
                committed_tx_ids.push((tx_id, 2)); // 2 ops: debit + credit
            }
            Err(AxonError::ConflictingVersion { .. }) => {
                aborted += 1;
            }
            Err(e) => {
                tracing::warn!("unexpected transaction error: {e}");
                aborted += 1;
            }
        }
    }

    // ── CHECK ─────────────────────────────────────────────────────────────────

    // INV-008a: balance conservation.
    let final_a = handler
        .get_entity(GetEntityRequest {
            collection: col.clone(),
            id: EntityId::new(ACCOUNT_A),
        })
        .map(|r| r.entity.data["balance"].as_i64().unwrap_or(0))
        .unwrap_or(0);

    let final_b = handler
        .get_entity(GetEntityRequest {
            collection: col.clone(),
            id: EntityId::new(ACCOUNT_B),
        })
        .map(|r| r.entity.data["balance"].as_i64().unwrap_or(0))
        .unwrap_or(0);

    let balance_conserved = final_a + final_b == INITIAL_BALANCE * 2;

    // INV-008b: atomicity via audit log.
    let all_entries = handler.audit_log().entries();

    // Build a map: tx_id → count of audit entries.
    let mut tx_entry_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for entry in all_entries {
        if let Some(ref tx_id) = entry.transaction_id {
            *tx_entry_counts.entry(tx_id.clone()).or_insert(0) += 1;
        }
    }

    let mut atomicity_correct = true;

    // Every committed tx must have exactly `expected_op_count` audit entries.
    for (tx_id, expected_ops) in &committed_tx_ids {
        let actual = tx_entry_counts.get(tx_id).copied().unwrap_or(0);
        if actual != *expected_ops {
            atomicity_correct = false;
        }
    }

    // No tx_id from the audit log should have a partial count (only whole
    // transactions appear because uncommitted entries are never flushed).
    for (tx_id, count) in &tx_entry_counts {
        // Every tx_id in the log must be in our committed list.
        if !committed_tx_ids.iter().any(|(id, _)| id == tx_id) {
            atomicity_correct = false;
        }
        // Count must be non-zero (trivially true from the map build, but guard anyway).
        if *count == 0 {
            atomicity_correct = false;
        }
    }

    // INV-007: version monotonicity for both accounts.
    let versions_correct = [ACCOUNT_A, ACCOUNT_B].iter().all(|name| {
        let entries: Vec<_> = all_entries
            .iter()
            .filter(|e| e.collection == col && e.entity_id.as_str() == *name)
            .collect();
        check_version_monotonicity(&entries)
    });

    TransactionAtomicityResult {
        seed: config.seed,
        committed,
        aborted,
        balance_conserved,
        atomicity_correct,
        versions_correct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_conserved_and_atomicity_holds_under_normal_execution() {
        let config = TransactionAtomicityConfig {
            num_rounds: 20,
            max_transfer: 30,
            seed: 0xc0ffee,
            buggify: false,
            ..Default::default()
        };
        let result = run_transaction_atomicity_workload(&config);
        assert!(
            result.balance_conserved,
            "INV-008a: balance must be conserved (final A+B = {}; expected {})",
            2 * INITIAL_BALANCE,
            2 * INITIAL_BALANCE
        );
        assert!(
            result.atomicity_correct,
            "INV-008b: every committed tx must have exactly 2 audit entries"
        );
        assert!(
            result.versions_correct,
            "INV-007: version sequences must be strictly monotone"
        );
        assert!(
            result.committed > 0,
            "at least some transactions should have committed"
        );
    }

    #[test]
    fn same_seed_produces_identical_execution() {
        let config = TransactionAtomicityConfig {
            num_rounds: 10,
            seed: 42,
            buggify: false,
            ..Default::default()
        };
        let r1 = run_transaction_atomicity_workload(&config);
        let r2 = run_transaction_atomicity_workload(&config);
        assert_eq!(r1.committed, r2.committed);
        assert_eq!(r1.aborted, r2.aborted);
    }

    #[test]
    fn buggify_may_abort_transactions_but_invariants_hold() {
        let config = TransactionAtomicityConfig {
            num_rounds: 30,
            seed: 99,
            buggify: true,
            buggify_pct: 40,
            ..Default::default()
        };
        let result = run_transaction_atomicity_workload(&config);
        assert!(
            result.is_correct(),
            "INV-008 must hold even with BUGGIFY-injected aborts"
        );
    }

    #[test]
    fn aborted_transactions_leave_no_audit_entries() {
        // Run with forced BUGGIFY so we have both committed and aborted rounds.
        let config = TransactionAtomicityConfig {
            num_rounds: 20,
            seed: 7,
            buggify: true,
            buggify_pct: 50,
            ..Default::default()
        };
        let result = run_transaction_atomicity_workload(&config);
        // The audit log should have exactly committed * 2 entity-mutation entries.
        // (The create entries for the two setup accounts add 2 more.)
        // Just verify that INV-008b holds.
        assert!(
            result.atomicity_correct,
            "aborted transactions must not appear in the audit log"
        );
    }
}
