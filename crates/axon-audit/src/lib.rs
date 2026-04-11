#![forbid(unsafe_code)]
//! Immutable audit log: records every mutation with provenance.
//!
//! `axon-audit` provides the append-only audit trail that tracks every
//! mutation made to Axon entities. Each audit entry records who/what made
//! the change, when it occurred, and what changed.

pub mod cdc;
pub mod entry;
pub mod log;

pub use cdc::{
    CdcEnvelope, CdcOp, CdcSink, CdcSource, JsonlFileSink, KafkaCdcSink, KafkaConfig, MemoryCdcSink,
};
pub use entry::{compute_diff, AuditEntry, FieldDiff, MutationType};
pub use log::{AuditLog, AuditPage, AuditQuery, MemoryAuditLog};
