//! Deterministic pseudo-random number generator based on xorshift64.
//!
//! Given the same seed, `SimRng` produces an identical sequence on every run.
//! This is the foundation of reproducible simulation: record the seed, replay
//! the failure.

/// Deterministic PRNG based on xorshift64.
///
/// ## Usage
///
/// ```rust
/// use axon_sim::rng::SimRng;
///
/// let mut rng = SimRng::new(42);
/// let a = rng.next_usize(10); // always the same for seed 42
/// let b = rng.next_usize(10);
/// assert_ne!(a, b); // the sequence advances
/// ```
#[derive(Debug, Clone)]
pub struct SimRng {
    state: u64,
}

impl SimRng {
    /// Create a new `SimRng` with the given seed.
    ///
    /// Seed `0` is bumped to `1` to avoid the degenerate all-zeros state.
    pub fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    /// Return the next pseudorandom `u64`.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Return a pseudorandom `usize` in `[0, range)`.
    ///
    /// # Panics
    ///
    /// Panics if `range == 0`.
    pub fn next_usize(&mut self, range: usize) -> usize {
        assert!(range > 0, "range must be > 0");
        (self.next_u64() % range as u64) as usize
    }

    /// Return `true` with probability `percent / 100` (e.g., `percent = 10`
    /// returns `true` roughly 10% of the time).
    pub fn next_bool_pct(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent.min(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_produces_identical_sequence() {
        let mut a = SimRng::new(12345);
        let mut b = SimRng::new(12345);
        let seq_a: Vec<u64> = (0..20).map(|_| a.next_u64()).collect();
        let seq_b: Vec<u64> = (0..20).map(|_| b.next_u64()).collect();
        assert_eq!(seq_a, seq_b, "same seed must produce identical sequence");
    }

    #[test]
    fn different_seeds_produce_different_sequences() {
        let mut a = SimRng::new(1);
        let mut b = SimRng::new(2);
        let seq_a: Vec<u64> = (0..10).map(|_| a.next_u64()).collect();
        let seq_b: Vec<u64> = (0..10).map(|_| b.next_u64()).collect();
        assert_ne!(seq_a, seq_b);
    }

    #[test]
    fn next_usize_in_range() {
        let mut rng = SimRng::new(99);
        for _ in 0..1000 {
            let v = rng.next_usize(7);
            assert!(v < 7, "value {v} should be < 7");
        }
    }

    #[test]
    fn next_bool_pct_always_true_at_100() {
        let mut rng = SimRng::new(42);
        for _ in 0..100 {
            assert!(rng.next_bool_pct(100));
        }
    }

    #[test]
    fn next_bool_pct_never_true_at_0() {
        let mut rng = SimRng::new(42);
        for _ in 0..100 {
            assert!(!rng.next_bool_pct(0));
        }
    }
}
