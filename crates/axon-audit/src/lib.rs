//! Immutable audit log: records every mutation with provenance.
//!
//! `axon-audit` provides the append-only audit trail that tracks every
//! mutation made to Axon entities. Each audit entry records who/what made
//! the change, when it occurred, and what changed.

pub mod entry;
pub mod log;

pub use entry::{AuditEntry, MutationType};
pub use log::{AuditLog, MemoryAuditLog};
