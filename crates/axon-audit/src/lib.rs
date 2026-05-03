#![forbid(unsafe_code)]
//! Immutable audit log: records every mutation with provenance.
//!
//! `axon-audit` provides the append-only audit trail that tracks every
//! mutation made to Axon entities. Each audit entry records who/what made
//! the change, when it occurred, and what changed.

pub mod cdc;
pub mod entry;
pub mod log;
pub mod prov;

pub use cdc::{
    CdcEnvelope, CdcOp, CdcSink, CdcSource, JsonlFileSink, KafkaCdcSink, KafkaConfig, MemoryCdcSink,
};
pub use entry::{
    compute_diff, AuditAttribution, AuditEntry, FieldDiff, MutationIntentApproverMetadata,
    MutationIntentAuditMetadata, MutationIntentAuditOrigin, MutationIntentAuditOriginSurface,
    MutationIntentLineageLink, MutationIntentLineageRelation, MutationType,
};
pub use log::{AuditLog, AuditPage, AuditQuery, MemoryAuditLog};
pub use prov::{
    audit_entries_from_prov_json, audit_entries_to_prov_json, audit_entry_to_native_json,
    validate_prov_o_json, PROV_NAMESPACE,
};
