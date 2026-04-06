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
    ///
    /// Produces envelopes for entity and link operations (US-077, US-078).
    /// Collection lifecycle events return `None`.
    pub fn from_audit_entry(entry: &AuditEntry) -> Option<Self> {
        use crate::entry::MutationType;

        let op = match entry.mutation {
            MutationType::EntityCreate | MutationType::LinkCreate => CdcOp::Create,
            MutationType::EntityUpdate | MutationType::EntityRevert => CdcOp::Update,
            MutationType::EntityDelete | MutationType::LinkDelete => CdcOp::Delete,
            // Non-entity/link operations don't produce CDC events.
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

// ── Kafka CDC sink (US-074, FEAT-021) ──────────────────────────────────────

/// Configuration for the Kafka CDC sink.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KafkaConfig {
    /// Kafka bootstrap servers (comma-separated).
    pub brokers: String,
    /// Topic name template. `{collection}` is replaced with the collection name.
    /// Default: `"axon.{collection}"`
    pub topic_template: String,
    /// Whether to enable the sink. Default: false.
    pub enabled: bool,
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            brokers: "localhost:9092".into(),
            topic_template: "axon.{collection}".into(),
            enabled: false,
        }
    }
}

impl KafkaConfig {
    /// Resolve the topic name for a given collection.
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn topic_for(&self, collection: &str) -> String {
        self.topic_template.replace("{collection}", collection)
    }

    /// Compute partition key for an entity.
    ///
    /// Uses the entity ID as the partition key so that all events for the
    /// same entity land in the same partition, preserving order.
    pub fn partition_key(entity_id: &str) -> String {
        entity_id.to_string()
    }
}

/// Stub Kafka CDC sink.
///
/// This implementation validates the interface and records what *would* be
/// sent to Kafka. The actual rdkafka integration is deferred to avoid
/// blocking on native dependency compilation.
///
/// Events are buffered in memory with the computed topic and partition key
/// so tests can verify correct routing.
#[derive(Debug)]
pub struct KafkaCdcSink {
    config: KafkaConfig,
    /// Buffered events: (topic, partition_key, envelope_json).
    pub sent: Vec<(String, String, String)>,
}

impl KafkaCdcSink {
    /// Create a new Kafka CDC sink with the given configuration.
    pub fn new(config: KafkaConfig) -> Self {
        Self {
            config,
            sent: Vec::new(),
        }
    }

    /// Access the configuration.
    pub fn config(&self) -> &KafkaConfig {
        &self.config
    }
}

impl CdcSink for KafkaCdcSink {
    fn emit(&mut self, envelope: &CdcEnvelope) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(());
        }

        let topic = self.config.topic_for(&envelope.source.collection);
        let partition_key = KafkaConfig::partition_key(&envelope.source.entity_id);
        let json = serde_json::to_string(envelope)
            .map_err(|e| format!("Kafka CDC serialization error: {e}"))?;

        self.sent.push((topic, partition_key, json));
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        // Stub: in real implementation, this would flush the rdkafka producer.
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
    fn link_create_produces_cdc_event() {
        let mut entry = AuditEntry::new(
            CollectionId::new("__axon_links__"),
            EntityId::new("tasks/t-001/depends-on/tasks/t-002"),
            1,
            MutationType::LinkCreate,
            None,
            Some(json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "depends-on"
            })),
            Some("agent-1".into()),
        );
        entry.id = 50;
        entry.timestamp_ns = 5_000_000_000;

        let envelope = CdcEnvelope::from_audit_entry(&entry).unwrap();
        assert_eq!(envelope.op, CdcOp::Create);
        assert_eq!(envelope.source.collection, "__axon_links__");
        assert!(envelope.after.is_some());
    }

    #[test]
    fn link_delete_produces_cdc_event() {
        let mut entry = AuditEntry::new(
            CollectionId::new("__axon_links__"),
            EntityId::new("tasks/t-001/depends-on/tasks/t-002"),
            1,
            MutationType::LinkDelete,
            Some(json!({
                "source_collection": "tasks",
                "source_id": "t-001",
                "target_collection": "tasks",
                "target_id": "t-002",
                "link_type": "depends-on"
            })),
            None,
            Some("agent-1".into()),
        );
        entry.id = 51;
        entry.timestamp_ns = 6_000_000_000;

        let envelope = CdcEnvelope::from_audit_entry(&entry).unwrap();
        assert_eq!(envelope.op, CdcOp::Delete);
        assert!(envelope.before.is_some());
        assert!(envelope.after.is_none());
    }

    #[test]
    fn ts_ms_from_timestamp_ns() {
        let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        assert_eq!(envelope.ts_ms, 1000); // 1_000_000_000 ns = 1000 ms
    }

    // ── Kafka CDC sink tests (US-074) ──────────────────────────────────

    #[test]
    fn kafka_config_topic_template() {
        let config = KafkaConfig {
            brokers: "b1:9092,b2:9092".into(),
            topic_template: "axon.{collection}".into(),
            enabled: true,
        };
        assert_eq!(config.topic_for("tasks"), "axon.tasks");
        assert_eq!(config.topic_for("users"), "axon.users");
    }

    #[test]
    fn kafka_config_partition_key_is_entity_id() {
        assert_eq!(KafkaConfig::partition_key("t-001"), "t-001");
    }

    #[test]
    fn kafka_sink_disabled_does_not_emit() {
        let config = KafkaConfig {
            enabled: false,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        sink.emit(&envelope).unwrap();
        assert!(sink.sent.is_empty());
    }

    #[test]
    fn kafka_sink_enabled_records_events() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        sink.emit(&envelope).unwrap();
        assert_eq!(sink.sent.len(), 1);

        let (topic, key, json) = &sink.sent[0];
        assert_eq!(topic, "axon.tasks");
        assert_eq!(key, "t-001");

        // Verify the JSON is valid and contains the right audit_id.
        let parsed: CdcEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.source.audit_id, 42);
    }

    #[test]
    fn kafka_sink_partitions_by_entity_key() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);

        // Emit events for different entities.
        let e1 = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        let mut entry2 = sample_update_entry();
        entry2.entity_id = EntityId::new("t-002");
        let e2 = CdcEnvelope::from_audit_entry(&entry2).unwrap();

        sink.emit(&e1).unwrap();
        sink.emit(&e2).unwrap();

        assert_eq!(sink.sent[0].1, "t-001"); // partition key for entity t-001
        assert_eq!(sink.sent[1].1, "t-002"); // partition key for entity t-002
    }

    #[test]
    fn kafka_sink_audit_id_cursor() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);

        let e1 = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
        let e2 = CdcEnvelope::from_audit_entry(&sample_update_entry()).unwrap();
        sink.emit(&e1).unwrap();
        sink.emit(&e2).unwrap();

        // Verify that audit_ids increase, providing a cursor for consumers.
        let parsed1: CdcEnvelope = serde_json::from_str(&sink.sent[0].2).unwrap();
        let parsed2: CdcEnvelope = serde_json::from_str(&sink.sent[1].2).unwrap();
        assert!(
            parsed2.source.audit_id > parsed1.source.audit_id,
            "audit_id should increase: {} > {}",
            parsed2.source.audit_id,
            parsed1.source.audit_id
        );
    }

    #[test]
    fn kafka_config_default() {
        let config = KafkaConfig::default();
        assert_eq!(config.brokers, "localhost:9092");
        assert!(!config.enabled);
    }
}
