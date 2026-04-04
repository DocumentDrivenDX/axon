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

pub mod buggify;
pub mod cycle;
pub mod rng;

pub use buggify::Buggify;
pub use cycle::{run_cycle_test, CycleConfig, CycleResult};
pub use rng::SimRng;
