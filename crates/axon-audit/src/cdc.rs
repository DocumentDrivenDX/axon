//! Change Data Capture (CDC) sinks for streaming changes without Kafka (US-077, FEAT-021).
//!
//! Provides Debezium-compatible envelope format and pluggable sinks:
//! - **JSONL file sink**: Appends one JSON line per event to a file
//! - **In-memory sink**: Collects events in memory (for testing)
//!
//! All sinks emit events in the same Debezium envelope format that Kafka
//! CDC (US-074) would use, so consumers work identically regardless of
//! transport.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entry::AuditEntry;

/// Debezium-compatible CDC operation type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CdcOp {
    /// Entity created.
    #[serde(rename = "c")]
    Create,
    /// Entity updated.
    #[serde(rename = "u")]
    Update,
    /// Entity deleted.
    #[serde(rename = "d")]
    Delete,
}

/// Debezium-compatible CDC envelope.
///
/// Matches the Debezium JSON Envelope format so that consumers
/// built for Kafka CDC can process file/SSE events without changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcEnvelope {
    /// Source metadata.
    pub source: CdcSource,
    /// Operation type.
    pub op: CdcOp,
    /// Milliseconds since epoch when the event was produced.
    pub ts_ms: u64,
    /// Entity data before the change (null for creates).
    pub before: Option<Value>,
    /// Entity data after the change (null for deletes).
    pub after: Option<Value>,
}

/// Source metadata in the Debezium envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcSource {
    /// Source system name.
    pub name: String,
    /// Collection name.
    pub collection: String,
    /// Entity ID.
    pub entity_id: String,
    /// Entity version after the change.
    pub version: u64,
    /// Audit log entry ID (acts as cursor for replay).
    pub audit_id: u64,
    /// Actor who performed the change.
    pub actor: String,
}

impl CdcEnvelope {
    /// Convert an [`AuditEntry`] to a Debezium-compatible CDC envelope.
    pub fn from_audit_entry(entry: &AuditEntry) -> Option<Self> {
        use crate::entry::MutationType;

        let op = match entry.mutation {
            MutationType::EntityCreate => CdcOp::Create,
            MutationType::EntityUpdate | MutationType::EntityRevert => CdcOp::Update,
            MutationType::EntityDelete => CdcOp::Delete,
            // Non-entity operations don't produce CDC events.
            _ => return None,
        };

        let ts_ms = if entry.timestamp_ns > 0 {
            entry.timestamp_ns / 1_000_000
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        };

        Some(CdcEnvelope {
            source: CdcSource {
                name: "axon".into(),
                collection: entry.collection.to_string(),
                entity_id: entry.entity_id.to_string(),
                version: entry.version,
                audit_id: entry.id,
                actor: entry.actor.clone(),
            },
            op,
            ts_ms,
            before: entry.data_before.clone(),
            after: entry.data_after.clone(),
        })
    }
}

/// Trait for CDC event sinks.
pub trait CdcSink: Send {
    /// Emit a CDC event.
    fn emit(&mut self, envelope: &CdcEnvelope) -> Result<(), String>;

    /// Flush any buffered events.
    fn flush(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// JSONL file sink: appends one JSON line per event to a file.
pub struct JsonlFileSink {
    writer: Box<dyn Write + Send>,
}

impl JsonlFileSink {
    /// Create a JSONL sink that writes to the given writer.
    pub fn new(writer: impl Write + Send + 'static) -> Self {
        Self {
            writer: Box::new(writer),
        }
    }

    /// Create a JSONL sink that appends to a file at the given path.
    pub fn from_path(path: &std::path::Path) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("failed to open CDC file: {e}"))?;
        Ok(Self::new(file))
    }
}

impl CdcSink for JsonlFileSink {
    fn emit(&mut self, envelope: &CdcEnvelope) -> Result<(), String> {
        let json = serde_json::to_string(envelope)
            .map_err(|e| format!("CDC serialization error: {e}"))?;
        self.writer
            .write_all(json.as_bytes())
            .map_err(|e| format!("CDC write error: {e}"))?;
        self.writer
            .write_all(b"\n")
            .map_err(|e| format!("CDC write error: {e}"))?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        self.writer
            .flush()
            .map_err(|e| format!("CDC flush error: {e}"))
    }
}

/// In-memory CDC sink for testing.
#[derive(Debug, Default)]
pub struct MemoryCdcSink {
    pub events: Vec<CdcEnvelope>,
}

impl CdcSink for MemoryCdcSink {
    fn emit(&mut self, envelope: &CdcEnvelope) -> Result<(), String> {
        self.events.push(envelope.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{AuditEntry, MutationType};
    use axon_core::id::{CollectionId, EntityId};
    use serde_json::json;

    fn sample_create_entry() -> AuditEntry {
        let mut entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            1,
            MutationType::EntityCreate,
            None,
            Some(json!({"title": "hello"})),
            Some("agent-1".into()),
        );
        entry.id = 42;
        entry.timestamp_ns = 1_000_000_000;
        entry
    }

    fn sample_update_entry() -> AuditEntry {
        let mut entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            2,
            MutationType::EntityUpdate,
            Some(json!({"title": "hello"})),
            Some(json!({"title": "world"})),
            Some("agent-1".into()),
        );
        entry.id = 43;
        entry.timestamp_ns = 2_000_000_000;
        entry
    }

    fn sample_delete_entry() -> AuditEntry {
        let mut entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            2,
            MutationType::EntityDelete,
            Some(json!({"title": "world"})),
            None,
            Some("agent-1".into()),
        );
        entry.id = 44;
        entry.timestamp_ns = 3_000_000_000;
        entry
    }

    #[test]
    fn create_entry_to_cdc_envelope() {
        let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        assert_eq!(envelope.op, CdcOp::Create);
        assert_eq!(envelope.source.collection, "tasks");
        assert_eq!(envelope.source.entity_id, "t-001");
        assert_eq!(envelope.source.audit_id, 42);
        assert!(envelope.before.is_none());
        assert!(envelope.after.is_some());
    }

    #[test]
    fn update_entry_to_cdc_envelope() {
        let envelope = CdcEnvelope::from_audit_entry(&sample_update_entry()).unwrap();
        assert_eq!(envelope.op, CdcOp::Update);
        assert!(envelope.before.is_some());
        assert!(envelope.after.is_some());
    }

    #[test]
    fn delete_entry_to_cdc_envelope() {
        let envelope = CdcEnvelope::from_audit_entry(&sample_delete_entry()).unwrap();
        assert_eq!(envelope.op, CdcOp::Delete);
        assert!(envelope.before.is_some());
        assert!(envelope.after.is_none());
    }

    #[test]
    fn collection_events_produce_no_cdc() {
        let entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new(""),
            0,
            MutationType::CollectionCreate,
            None,
            None,
            None,
        );
        assert!(CdcEnvelope::from_audit_entry(&entry).is_none());
    }

    #[test]
    fn memory_sink_collects_events() {
        let mut sink = MemoryCdcSink::default();
        let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        sink.emit(&envelope).unwrap();
        assert_eq!(sink.events.len(), 1);
    }

    #[test]
    fn jsonl_sink_writes_newline_delimited() {
        use std::sync::{Arc, Mutex};

        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let buf = Arc::clone(&buf);
            // Wrap in a struct that implements Write + Send.
            struct SharedWriter(Arc<Mutex<Vec<u8>>>);
            impl Write for SharedWriter {
                fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().write(data)
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            SharedWriter(buf)
        };

        let mut sink = JsonlFileSink::new(writer);
        let e1 = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        let e2 = CdcEnvelope::from_audit_entry(&sample_update_entry()).unwrap();
        sink.emit(&e1).unwrap();
        sink.emit(&e2).unwrap();
        sink.flush().unwrap();

        let data = buf.lock().unwrap();
        let output = String::from_utf8(data.clone()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 2, "should have 2 JSONL lines");

        // Each line should be valid JSON.
        for line in &lines {
            let parsed: CdcEnvelope = serde_json::from_str(line).unwrap();
            assert_eq!(parsed.source.name, "axon");
        }
    }

    #[test]
    fn cdc_envelope_roundtrip_serialization() {
        let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: CdcEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.op, CdcOp::Create);
        assert_eq!(parsed.source.audit_id, 42);
    }

    #[test]
    fn ts_ms_from_timestamp_ns() {
        let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        assert_eq!(envelope.ts_ms, 1000); // 1_000_000_000 ns = 1000 ms
    }
}
