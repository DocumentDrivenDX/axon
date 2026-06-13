// Bounded seed sweep: runs all seeded correctness workloads across N seeds
// (AXON_SIM_SEEDS env var, default 10) so every CI commit exercises the
// invariants against a small but diverse set of seeds.
//
// Per-commit CI:  AXON_SIM_SEEDS=10  (default, included in `cargo test`)
// Extended local: AXON_SIM_SEEDS=100 cargo test -p axon-sim
// Nightly:        AXON_SIM_SEEDS=1000 cargo test -p axon-sim
//
// Any invariant violation is reported with the failing seed so it can be
// added to the regression seed file and replayed on every CI build forever.

use axon_sim::{
    run_audit_completeness_workload, run_audit_immutability_workload,
    run_concurrent_writer_workload, run_cycle_test, run_transaction_atomicity_workload,
    AuditCompletenessConfig, AuditImmutabilityConfig, ConcurrentWriterConfig, CycleConfig,
    TransactionAtomicityConfig,
};

fn seed_count() -> u64 {
    std::env::var("AXON_SIM_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10)
}

// Derive a distinct well-spread seed for the i-th run using a Knuth LCG step.
fn nth_seed(i: u64) -> u64 {
    0xdead_beef_cafe_babe_u64.wrapping_add(i.wrapping_mul(6_364_136_223_846_793_005))
}

#[test]
fn sim_seed_sweep() {
    let n = seed_count();
    let mut violations: Vec<String> = Vec::new();

    for i in 0..n {
        let seed = nth_seed(i);

        // INV-001 + INV-007: no lost updates / version monotonicity
        let cw = run_concurrent_writer_workload(&ConcurrentWriterConfig {
            num_agents: 3,
            num_rounds: 10,
            seed,
            buggify: true,
            buggify_pct: 10,
            ..ConcurrentWriterConfig::default()
        });
        if !cw.is_correct() {
            violations.push(format!(
                "INV-001/007 seed={seed:#018x}: counter_correct={} versions_correct={}",
                cw.counter_correct, cw.versions_correct
            ));
        }

        // INV-002: cycle test (snapshot isolation)
        let cy = run_cycle_test(&CycleConfig {
            ring_size: 5,
            num_swaps: 20,
            seed,
            buggify: true,
            buggify_pct: 10,
        });
        if !cy.is_correct() {
            violations.push(format!(
                "INV-002 seed={seed:#018x}: hops={}/{}",
                cy.hops_observed, cy.hops_expected
            ));
        }

        // INV-003 + INV-007: audit completeness / version monotonicity
        let ac = run_audit_completeness_workload(&AuditCompletenessConfig {
            seed,
            ..AuditCompletenessConfig::default()
        });
        if !ac.is_correct() {
            violations.push(format!(
                "INV-003 seed={seed:#018x}: entry_count_correct={} reconstruction_correct={} version_monotone={}",
                ac.entry_count_correct, ac.reconstruction_correct, ac.version_monotone
            ));
        }

        // INV-004: audit immutability
        let ai = run_audit_immutability_workload(&AuditImmutabilityConfig {
            seed,
            ..AuditImmutabilityConfig::default()
        });
        if !ai.is_correct() {
            violations.push(format!(
                "INV-004 seed={seed:#018x}: no_entries_removed={} no_entries_mutated={} log_grew={}",
                ai.no_entries_removed, ai.no_entries_mutated, ai.log_grew
            ));
        }

        // INV-008: transaction atomicity
        let ta = run_transaction_atomicity_workload(&TransactionAtomicityConfig {
            seed,
            buggify: true,
            buggify_pct: 10,
            ..TransactionAtomicityConfig::default()
        });
        if !ta.is_correct() {
            violations.push(format!(
                "INV-008 seed={seed:#018x}: balance_conserved={} atomicity_correct={} versions_correct={}",
                ta.balance_conserved, ta.atomicity_correct, ta.versions_correct
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "{n} seeds, {} violation(s):\n{}\n\nAdd failing seeds to scripts/regression-seeds.txt \
         to replay them on every CI build.",
        violations.len(),
        violations.join("\n")
    );
}
