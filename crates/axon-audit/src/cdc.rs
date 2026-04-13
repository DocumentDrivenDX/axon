//! Change Data Capture (CDC) sinks for streaming changes (US-077, FEAT-021).
//!
//! Provides Debezium-compatible envelope format and pluggable sinks:
//! - **JSONL file sink**: Appends one JSON line per event to a file
//! - **In-memory sink**: Collects events in memory (for testing)
//! - **Kafka sink**: Produces events to Kafka via rdkafka (requires `kafka` feature)
//!
//! All sinks emit events in the same Debezium envelope format so that
//! consumers work identically regardless of transport.
//!
//! # Kafka feature flag
//!
//! The `kafka` feature enables real rdkafka producer integration in
//! [`KafkaCdcSink`]. Without the feature the struct is a stub that buffers
//! events in memory for routing verification.
//!
//! To run Kafka-specific tests (uses [`MockProducer`], no broker required):
//! ```text
//! cargo test -p axon-audit --features kafka
//! ```

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
        let json =
            serde_json::to_string(envelope).map_err(|e| format!("CDC serialization error: {e}"))?;
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

// ── Internal Kafka producer backend trait (kafka feature) ─────────────────

/// Internal trait for Kafka producer backends.
///
/// Enables dependency injection for testing without a real broker.
#[cfg(feature = "kafka")]
pub(crate) trait KafkaProducerBackend: Send + std::fmt::Debug {
    /// Send a message to the given topic with the given key and payload.
    fn send(&self, topic: &str, key: &str, payload: &str) -> Result<(), String>;
}

// ── MockProducer (kafka feature) ──────────────────────────────────────────

/// Mock Kafka producer for testing.
///
/// Records all sent messages in an [`Arc<Mutex<Vec<...>>>`] so tests can
/// inspect them without a real Kafka broker.
#[cfg(feature = "kafka")]
#[derive(Debug, Default, Clone)]
pub struct MockProducer {
    /// All messages sent: (topic, partition_key, payload_json).
    pub sent: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
}

#[cfg(feature = "kafka")]
impl KafkaProducerBackend for MockProducer {
    fn send(&self, topic: &str, key: &str, payload: &str) -> Result<(), String> {
        self.sent
            .lock()
            .unwrap()
            .push((topic.to_string(), key.to_string(), payload.to_string()));
        Ok(())
    }
}

// ── RdkafkaProducer (kafka feature) ──────────────────────────────────────

/// rdkafka `FutureProducer`-backed Kafka producer.
///
/// Serializes each send to a blocking call. In production async code prefer
/// restructuring the CDC pipeline to be fully async.
#[cfg(feature = "kafka")]
struct RdkafkaProducer {
    inner: rdkafka::producer::FutureProducer,
}

#[cfg(feature = "kafka")]
impl std::fmt::Debug for RdkafkaProducer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RdkafkaProducer").finish_non_exhaustive()
    }
}

#[cfg(feature = "kafka")]
impl KafkaProducerBackend for RdkafkaProducer {
    fn send(&self, topic: &str, key: &str, payload: &str) -> Result<(), String> {
        use rdkafka::producer::FutureRecord;
        use std::time::Duration;

        let record = FutureRecord::to(topic).key(key).payload(payload);
        // Create a temporary runtime to drive the async delivery future.
        // NOTE: Do not call this from within a tokio async context — use
        // spawn_blocking or restructure as fully async instead.
        tokio::runtime::Runtime::new()
            .map_err(|e| format!("failed to create tokio runtime: {e}"))?
            .block_on(async {
                self.inner
                    .send(record, Duration::from_secs(5))
                    .await
                    .map(|_| ())
                    .map_err(|(e, _)| format!("CdcError::ProducerError: {e}"))
            })
    }
}

// ── KafkaCdcSink — stub (no kafka feature) ────────────────────────────────

/// Stub Kafka CDC sink (compiled without the `kafka` feature).
///
/// Validates routing logic and records what *would* be sent to Kafka.
/// Enable the `kafka` feature for the real rdkafka-backed implementation.
///
/// Run Kafka integration tests: `cargo test -p axon-audit --features kafka`
#[cfg(not(feature = "kafka"))]
#[derive(Debug)]
pub struct KafkaCdcSink {
    config: KafkaConfig,
    /// Buffered events: (topic, partition_key, envelope_json).
    pub sent: Vec<(String, String, String)>,
}

#[cfg(not(feature = "kafka"))]
impl KafkaCdcSink {
    /// Create a new stub Kafka CDC sink.
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

#[cfg(not(feature = "kafka"))]
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
        Ok(())
    }
}

// ── KafkaCdcSink — real rdkafka (kafka feature) ───────────────────────────

/// Kafka CDC sink backed by an rdkafka `FutureProducer`.
///
/// Each [`CdcEnvelope`] is serialized to a Debezium JSON envelope and
/// produced to the topic resolved by [`KafkaConfig::topic_for`], with the
/// entity ID as the message key (preserving per-entity ordering).
///
/// Producer errors surface as `Err(String)` from [`CdcSink::emit`] and
/// never panic.
///
/// # Testing
///
/// Use [`KafkaCdcSink::with_producer`] and [`MockProducer`] to test without
/// a real Kafka broker:
///
/// ```ignore
/// let mock = MockProducer::default();
/// let sent = Arc::clone(&mock.sent);
/// let mut sink = KafkaCdcSink::with_producer(config, mock);
/// ```
#[cfg(feature = "kafka")]
pub struct KafkaCdcSink {
    config: KafkaConfig,
    producer: Box<dyn KafkaProducerBackend>,
}

#[cfg(feature = "kafka")]
impl std::fmt::Debug for KafkaCdcSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KafkaCdcSink")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "kafka")]
impl KafkaCdcSink {
    /// Create a sink backed by a real rdkafka `FutureProducer`.
    ///
    /// Returns an error if the producer cannot be configured (e.g. invalid
    /// broker address format). The actual broker connection is lazy.
    pub fn new(config: KafkaConfig) -> Result<Self, String> {
        use rdkafka::config::ClientConfig;
        let producer: rdkafka::producer::FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| format!("CdcError::ProducerError: failed to create producer: {e}"))?;
        Ok(Self {
            config,
            producer: Box::new(RdkafkaProducer { inner: producer }),
        })
    }

    /// Create a sink backed by a custom producer (for testing with [`MockProducer`]).
    pub fn with_producer(
        config: KafkaConfig,
        producer: impl KafkaProducerBackend + 'static,
    ) -> Self {
        Self {
            config,
            producer: Box::new(producer),
        }
    }

    /// Access the configuration.
    pub fn config(&self) -> &KafkaConfig {
        &self.config
    }
}

#[cfg(feature = "kafka")]
impl CdcSink for KafkaCdcSink {
    fn emit(&mut self, envelope: &CdcEnvelope) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(());
        }

        let topic = self.config.topic_for(&envelope.source.collection);
        let key = KafkaConfig::partition_key(&envelope.source.entity_id);
        let payload = serde_json::to_string(envelope)
            .map_err(|e| format!("CDC serialization error: {e}"))?;

        self.producer
            .send(&topic, &key, &payload)
            .map_err(|e| format!("CdcError::ProducerError: {e}"))
    }

    fn flush(&mut self) -> Result<(), String> {
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{AuditEntry, MutationType};
    use axon_core::id::{CollectionId, EntityId};
    use serde_json::json;

    fn must_some<T>(value: Option<T>, context: &str) -> T {
        match value {
            Some(value) => value,
            None => panic!("{context}"),
        }
    }

    fn must_ok<T, E: std::fmt::Debug>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("{context}: {error:?}"),
        }
    }

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
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        assert_eq!(envelope.op, CdcOp::Create);
        assert_eq!(envelope.source.collection, "tasks");
        assert_eq!(envelope.source.entity_id, "t-001");
        assert_eq!(envelope.source.audit_id, 42);
        assert!(envelope.before.is_none());
        assert!(envelope.after.is_some());
    }

    #[test]
    fn update_entry_to_cdc_envelope() {
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_update_entry()),
            "entity update should produce a CDC envelope",
        );
        assert_eq!(envelope.op, CdcOp::Update);
        assert!(envelope.before.is_some());
        assert!(envelope.after.is_some());
    }

    #[test]
    fn delete_entry_to_cdc_envelope() {
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_delete_entry()),
            "entity delete should produce a CDC envelope",
        );
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
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        must_ok(
            sink.emit(&envelope),
            "memory sink should collect emitted events",
        );
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
                    match self.0.lock() {
                        Ok(mut buffer) => buffer.write(data),
                        Err(error) => Err(std::io::Error::other(format!(
                            "shared writer mutex poisoned: {error}"
                        ))),
                    }
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            SharedWriter(buf)
        };

        let mut sink = JsonlFileSink::new(writer);
        let e1 = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        let e2 = must_some(
            CdcEnvelope::from_audit_entry(&sample_update_entry()),
            "entity update should produce a CDC envelope",
        );
        must_ok(sink.emit(&e1), "jsonl sink should emit first event");
        must_ok(sink.emit(&e2), "jsonl sink should emit second event");
        must_ok(sink.flush(), "jsonl sink should flush");

        let data = must_ok(buf.lock(), "acquire JSONL test buffer lock");
        let output = must_ok(
            String::from_utf8(data.clone()),
            "JSONL output should remain valid UTF-8",
        );
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2, "should have 2 JSONL lines");

        // Each line should be valid JSON.
        for line in &lines {
            let parsed: CdcEnvelope = must_ok(
                serde_json::from_str(line),
                "JSONL line should parse as CDC envelope",
            );
            assert_eq!(parsed.source.name, "axon");
        }
    }

    #[test]
    fn cdc_envelope_roundtrip_serialization() {
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        let json = must_ok(
            serde_json::to_string(&envelope),
            "CDC envelope should serialize to JSON",
        );
        let parsed: CdcEnvelope = must_ok(
            serde_json::from_str(&json),
            "CDC envelope JSON should deserialize",
        );
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

        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&entry),
            "link create should produce a CDC envelope",
        );
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

        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&entry),
            "link delete should produce a CDC envelope",
        );
        assert_eq!(envelope.op, CdcOp::Delete);
        assert!(envelope.before.is_some());
        assert!(envelope.after.is_none());
    }

    #[test]
    fn ts_ms_from_timestamp_ns() {
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        assert_eq!(envelope.ts_ms, 1000); // 1_000_000_000 ns = 1000 ms
    }

    // ── Kafka config tests (always compiled) ──────────────────────────────

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
    fn kafka_config_default() {
        let config = KafkaConfig::default();
        assert_eq!(config.brokers, "localhost:9092");
        assert!(!config.enabled);
    }

    // ── Kafka CDC sink stub tests (no `kafka` feature) ────────────────────

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn kafka_sink_disabled_does_not_emit() {
        let config = KafkaConfig {
            enabled: false,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        must_ok(
            sink.emit(&envelope),
            "disabled Kafka sink emit should be a no-op",
        );
        assert!(sink.sent.is_empty());
    }

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn kafka_sink_enabled_records_events() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        must_ok(
            sink.emit(&envelope),
            "enabled Kafka sink should record the event",
        );
        assert_eq!(sink.sent.len(), 1);

        let (topic, key, json) = &sink.sent[0];
        assert_eq!(topic, "axon.tasks");
        assert_eq!(key, "t-001");

        // Verify the JSON is valid and contains the right audit_id.
        let parsed: CdcEnvelope = must_ok(
            serde_json::from_str(json),
            "stored Kafka CDC payload should deserialize",
        );
        assert_eq!(parsed.source.audit_id, 42);
    }

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn kafka_sink_partitions_by_entity_key() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);

        // Emit events for different entities.
        let e1 = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        let mut entry2 = sample_update_entry();
        entry2.entity_id = EntityId::new("t-002");
        let e2 = must_some(
            CdcEnvelope::from_audit_entry(&entry2),
            "entity update should produce a CDC envelope",
        );

        must_ok(
            sink.emit(&e1),
            "Kafka sink should emit first partitioned event",
        );
        must_ok(
            sink.emit(&e2),
            "Kafka sink should emit second partitioned event",
        );

        assert_eq!(sink.sent[0].1, "t-001"); // partition key for entity t-001
        assert_eq!(sink.sent[1].1, "t-002"); // partition key for entity t-002
    }

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn kafka_sink_audit_id_cursor() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);

        let e1 = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry()),
            "entity create should produce a CDC envelope",
        );
        let e2 = must_some(
            CdcEnvelope::from_audit_entry(&sample_update_entry()),
            "entity update should produce a CDC envelope",
        );
        must_ok(sink.emit(&e1), "Kafka sink should emit first cursor event");
        must_ok(sink.emit(&e2), "Kafka sink should emit second cursor event");

        // Verify that audit_ids increase, providing a cursor for consumers.
        let parsed1: CdcEnvelope = must_ok(
            serde_json::from_str(&sink.sent[0].2),
            "first Kafka CDC payload should deserialize",
        );
        let parsed2: CdcEnvelope = must_ok(
            serde_json::from_str(&sink.sent[1].2),
            "second Kafka CDC payload should deserialize",
        );
        assert!(
            parsed2.source.audit_id > parsed1.source.audit_id,
            "audit_id should increase: {} > {}",
            parsed2.source.audit_id,
            parsed1.source.audit_id
        );
    }

    // ── Kafka CDC sink tests with MockProducer (requires `kafka` feature) ─

    #[cfg(feature = "kafka")]
    mod kafka_producer_tests {
        use super::*;
        use std::sync::{Arc, Mutex};

        type SentMessages = Arc<Mutex<Vec<(String, String, String)>>>;

        /// Failing producer for error-path tests.
        #[derive(Debug)]
        struct FailingProducer;

        impl KafkaProducerBackend for FailingProducer {
            fn send(&self, _topic: &str, _key: &str, _payload: &str) -> Result<(), String> {
                Err("simulated producer error".to_string())
            }
        }

        fn make_enabled_sink() -> (KafkaCdcSink, SentMessages) {
            let config = KafkaConfig {
                enabled: true,
                ..KafkaConfig::default()
            };
            let mock = MockProducer::default();
            let sent = Arc::clone(&mock.sent);
            let sink = KafkaCdcSink::with_producer(config, mock);
            (sink, sent)
        }

        #[test]
        fn create_event_produces_op_c_on_correct_topic() {
            let (mut sink, sent) = make_enabled_sink();
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            assert_eq!(msgs.len(), 1);
            let (topic, key, json) = &msgs[0];
            assert_eq!(topic, "axon.tasks");
            assert_eq!(key, "t-001");

            let parsed: CdcEnvelope = serde_json::from_str(json).unwrap();
            assert_eq!(parsed.op, CdcOp::Create, "op should be 'c' for create");
            assert_eq!(parsed.source.audit_id, 42);
        }

        #[test]
        fn update_event_produces_op_u() {
            let (mut sink, sent) = make_enabled_sink();
            let envelope = CdcEnvelope::from_audit_entry(&sample_update_entry()).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            let (_, _, json) = &msgs[0];
            let parsed: CdcEnvelope = serde_json::from_str(json).unwrap();
            assert_eq!(parsed.op, CdcOp::Update, "op should be 'u' for update");
        }

        #[test]
        fn delete_event_produces_op_d() {
            let (mut sink, sent) = make_enabled_sink();
            let envelope = CdcEnvelope::from_audit_entry(&sample_delete_entry()).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            let (_, _, json) = &msgs[0];
            let parsed: CdcEnvelope = serde_json::from_str(json).unwrap();
            assert_eq!(parsed.op, CdcOp::Delete, "op should be 'd' for delete");
        }

        #[test]
        fn producer_error_returns_err_not_panic() {
            let config = KafkaConfig {
                enabled: true,
                ..KafkaConfig::default()
            };
            let mut sink = KafkaCdcSink::with_producer(config, FailingProducer);
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
            let result = sink.emit(&envelope);
            assert!(result.is_err(), "expected Err from failing producer");
            let msg = result.unwrap_err();
            assert!(
                msg.contains("ProducerError") || msg.contains("simulated"),
                "error should indicate producer failure, got: {msg}"
            );
        }

        #[test]
        fn disabled_sink_does_not_produce_to_mock() {
            let config = KafkaConfig {
                enabled: false,
                ..KafkaConfig::default()
            };
            let mock = MockProducer::default();
            let sent = Arc::clone(&mock.sent);
            let mut sink = KafkaCdcSink::with_producer(config, mock);
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
            sink.emit(&envelope).unwrap();
            assert!(
                sent.lock().unwrap().is_empty(),
                "disabled sink should not call producer"
            );
        }

        #[test]
        fn entity_id_used_as_message_key() {
            let (mut sink, sent) = make_enabled_sink();

            let e1 = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
            let mut entry2 = sample_update_entry();
            entry2.entity_id = EntityId::new("t-002");
            let e2 = CdcEnvelope::from_audit_entry(&entry2).unwrap();

            sink.emit(&e1).unwrap();
            sink.emit(&e2).unwrap();

            let msgs = sent.lock().unwrap();
            assert_eq!(msgs[0].1, "t-001", "key should be entity_id for first event");
            assert_eq!(msgs[1].1, "t-002", "key should be entity_id for second event");
        }

        #[test]
        fn debezium_envelope_fields_present() {
            let (mut sink, sent) = make_enabled_sink();
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry()).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            let (_, _, json) = &msgs[0];
            let v: serde_json::Value = serde_json::from_str(json).unwrap();
            assert!(v.get("op").is_some(), "envelope must have 'op' field");
            assert!(v.get("source").is_some(), "envelope must have 'source' field");
            assert!(v.get("ts_ms").is_some(), "envelope must have 'ts_ms' field");
            assert_eq!(v["op"], "c");
            assert_eq!(v["source"]["name"], "axon");
        }
    }
}
