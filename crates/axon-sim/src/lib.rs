//! Deterministic simulation testing (DST) framework for Axon.
//!
//! Inspired by FoundationDB's approach to correctness: define invariants
//! as executable workloads, inject faults deterministically using a seeded
//! PRNG, and verify that invariants hold across millions of random scenarios.
//!
//! ## Key components
//!
//! - [`rng`]: Deterministic PRNG (xorshift64). Same seed → identical execution.
//! - [`buggify`]: BUGGIFY fault injector. Activated probabilistically; injects
//!   write delays, transaction aborts, and storage errors.
//! - [`cycle`]: Cycle test workload. A ring of entities connected by "next" links.
//!   Transactions that modify the ring must preserve the ring structure.
//!   Isolation violations are detected by walking the ring and counting hops.
//!
//! ## Usage
//!
//! ```rust
//! use axon_sim::cycle::{CycleConfig, run_cycle_test};
//!
//! let config = CycleConfig {
//!     ring_size: 5,
//!     num_swaps: 20,
//!     seed: 0xdeadbeef,
//!     buggify: false,
//!     buggify_pct: 0,
//! };
//! let result = run_cycle_test(&config);
//! assert!(result.is_correct());
//! ```

pub mod audit_completeness;
pub mod audit_immutability;
pub mod buggify;
pub mod concurrent_writer;
pub mod cycle;
pub mod link_integrity;
pub mod rng;
pub mod schema_enforcement;
pub mod transaction_atomicity;

pub use audit_completeness::{
    run_audit_completeness_workload, AuditCompletenessConfig, AuditCompletenessResult,
};
pub use audit_immutability::{
    run_audit_immutability_workload, AuditImmutabilityConfig, AuditImmutabilityResult,
};
pub use buggify::Buggify;
pub use concurrent_writer::{
    run_concurrent_writer_workload, ConcurrentWriterConfig, ConcurrentWriterResult,
};
pub use cycle::{run_cycle_test, CycleConfig, CycleResult};
pub use link_integrity::{run_link_integrity_workload, LinkIntegrityResult};
pub use rng::SimRng;
pub use schema_enforcement::{run_schema_enforcement_workload, SchemaEnforcementResult};
pub use transaction_atomicity::{
    run_transaction_atomicity_workload, TransactionAtomicityConfig, TransactionAtomicityResult,
};
