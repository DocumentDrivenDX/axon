//! Shared invariant checkers used across axon-sim workloads.
//!
//! Functions here implement named invariants (INV-NNN) that are verified
//! by multiple workloads. Centralizing them prevents logic drift between
//! workloads that check the same property.

use axon_audit::entry::{AuditEntry, MutationType};

/// INV-007: verify that create/update/revert audit entries have strictly
/// increasing versions starting at 1, and that any delete entry records the
/// last non-delete version.
///
/// Entries must be pre-filtered to a single entity and sorted by sequence
/// number before calling this function.
pub fn check_version_monotonicity(entries: &[&AuditEntry]) -> bool {
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
