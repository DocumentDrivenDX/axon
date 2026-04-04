//! BUGGIFY: conditional fault injection for deterministic simulation.
//!
//! Inspired by FoundationDB's BUGGIFY macro. In simulation mode, code blocks
//! inside `buggify!` activate with a configurable probability, injecting
//! delays, errors, or other faults directly in the production code path.
//!
//! Unlike external fault injectors, BUGGIFY is woven into the logic being
//! tested, making it possible to hit internal invariants that external
//! injection cannot reach.
//!
//! ## Supported fault types
//!
//! - **Transaction abort**: return `ConflictingVersion` to simulate a
//!   write-write conflict that would occur under concurrent access.
//! - **Storage error**: return `AxonError::Storage` to simulate I/O failure.
//! - **Write delay**: spin for a configurable number of PRNG iterations to
//!   simulate latency without real sleeps (simulation runs faster than wall time).

use axon_core::error::AxonError;

use crate::rng::SimRng;

/// Fault that can be injected by BUGGIFY.
#[derive(Debug, Clone)]
pub enum Fault {
    /// Simulate a write-write conflict (optimistic concurrency abort).
    TransactionAbort { expected: u64, actual: u64 },
    /// Simulate a storage I/O error.
    StorageError(String),
    /// Simulate a write delay by burning PRNG cycles (no real sleep).
    WriteDelay { iterations: u32 },
}

impl Fault {
    /// Convert this fault into the `AxonError` variant it represents.
    ///
    /// `WriteDelay` faults do not map to an error — the returned `Ok(())` means
    /// the caller should continue normally after the simulated delay.
    pub fn to_error(&self) -> Option<AxonError> {
        match self {
            Fault::TransactionAbort { expected, actual } => Some(AxonError::ConflictingVersion {
                expected: *expected,
                actual: *actual,
            }),
            Fault::StorageError(msg) => Some(AxonError::Storage(msg.clone())),
            Fault::WriteDelay { .. } => None,
        }
    }
}

/// BUGGIFY context: drives fault injection probability and fault type selection.
///
/// All decisions are made via the injected [`SimRng`], so a run is fully
/// reproducible from its seed.
pub struct Buggify<'a> {
    rng: &'a mut SimRng,
    /// Probability (0–100) that any individual buggify point activates.
    pub activation_pct: u64,
}

impl<'a> Buggify<'a> {
    pub fn new(rng: &'a mut SimRng) -> Self {
        Self {
            rng,
            activation_pct: 10,
        }
    }

    pub fn with_activation_pct(mut self, pct: u64) -> Self {
        self.activation_pct = pct;
        self
    }

    /// Potentially inject a fault at this buggify point.
    ///
    /// Returns `Some(fault)` with probability `activation_pct / 100`, or
    /// `None` when the buggify point does not fire.
    pub fn maybe_fault(&mut self) -> Option<Fault> {
        if !self.rng.next_bool_pct(self.activation_pct) {
            return None;
        }
        // Choose which fault to inject.
        let kind = self.rng.next_usize(3);
        Some(match kind {
            0 => Fault::TransactionAbort {
                expected: 1,
                actual: 2,
            },
            1 => Fault::StorageError("simulated I/O error".into()),
            _ => Fault::WriteDelay {
                iterations: (self.rng.next_usize(1000) + 1) as u32,
            },
        })
    }

    /// Burn CPU cycles to simulate a write delay (no real sleep).
    fn simulate_delay(&mut self, iterations: u32) {
        for _ in 0..iterations {
            let _ = self.rng.next_u64();
        }
    }

    /// Potentially inject a fault and return it as an `AxonError` if the fault
    /// maps to one. Write delays are executed silently.
    ///
    /// Callers check `Result::Err` to detect injected errors:
    ///
    /// ```ignore
    /// buggify.maybe_error()?; // propagates injected errors
    /// storage.put(entity)?;   // real write
    /// ```
    pub fn maybe_error(&mut self) -> Result<(), AxonError> {
        match self.maybe_fault() {
            None => Ok(()),
            Some(Fault::WriteDelay { iterations }) => {
                self.simulate_delay(iterations);
                Ok(())
            }
            Some(fault) => match fault.to_error() {
                Some(err) => Err(err),
                None => Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SimRng;

    #[test]
    fn buggify_with_100pct_always_injects() {
        let mut rng = SimRng::new(1);
        let mut b = Buggify::new(&mut rng).with_activation_pct(100);
        let fault = b.maybe_fault();
        assert!(fault.is_some(), "100% activation should always inject");
    }

    #[test]
    fn buggify_with_0pct_never_injects() {
        let mut rng = SimRng::new(1);
        let mut b = Buggify::new(&mut rng).with_activation_pct(0);
        for _ in 0..100 {
            assert!(b.maybe_fault().is_none());
        }
    }

    #[test]
    fn fault_types_cover_all_variants() {
        // Run with a seeded RNG and collect at least one of each fault type.
        let mut rng = SimRng::new(42);
        let mut b = Buggify::new(&mut rng).with_activation_pct(100);
        let mut saw_abort = false;
        let mut saw_storage = false;
        let mut saw_delay = false;

        for _ in 0..200 {
            match b.maybe_fault() {
                Some(Fault::TransactionAbort { .. }) => saw_abort = true,
                Some(Fault::StorageError(_)) => saw_storage = true,
                Some(Fault::WriteDelay { .. }) => saw_delay = true,
                None => {}
            }
        }

        assert!(saw_abort, "should see TransactionAbort fault");
        assert!(saw_storage, "should see StorageError fault");
        assert!(saw_delay, "should see WriteDelay fault");
    }

    #[test]
    fn write_delay_does_not_produce_error() {
        // WriteDelay faults are transparent to the caller.
        let mut rng = SimRng::new(999);
        let mut b = Buggify::new(&mut rng).with_activation_pct(100);
        // Keep injecting until we get a write-delay result from maybe_error.
        // In the worst case one of the other faults fires first.
        let mut got_ok = false;
        for _ in 0..200 {
            if b.maybe_error().is_ok() {
                got_ok = true;
                break;
            }
        }
        assert!(got_ok, "WriteDelay and None should return Ok");
    }
}
