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

/// Runtime context required to build a CONTRACT-006-compliant CDC envelope.
///
/// Supplies the tenant-aware fields that are not present on [`AuditEntry`]
/// itself. Callers should populate this from the request context or
/// configuration; the `Default` impl uses `"default"` placeholders appropriate
/// for single-tenant deployments.
#[derive(Debug, Clone)]
pub struct CdcContext {
    /// Axon server instance name (e.g. `"axon-prod"`); used as `{instance}` in topic templates.
    pub instance_name: String,
    /// Tenant name (ADR-018).
    pub tenant: String,
    /// Database name (FEAT-014 namespace).
    pub db: String,
    /// Schema/namespace name.
    pub schema: String,
    /// Axon server version string (e.g. `"0.1.0"`).
    pub axon_version: String,
}

impl Default for CdcContext {
    fn default() -> Self {
        Self {
            instance_name: "axon".into(),
            tenant: "default".into(),
            db: "default".into(),
            schema: "default".into(),
            axon_version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

/// Debezium-compatible CDC operation type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CdcOp {
    /// Entity created.
    #[serde(rename = "c")]
    Create,
    /// Entity updated or patched.
    #[serde(rename = "u")]
    Update,
    /// Entity deleted.
    #[serde(rename = "d")]
    Delete,
    /// Snapshot read (initial snapshot — see CONTRACT-006 §Cursor semantics).
    #[serde(rename = "r")]
    Read,
}

/// Debezium-compatible CDC envelope (CONTRACT-006).
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
    /// Entity data before the change (null for creates and snapshot reads).
    pub before: Option<Value>,
    /// Entity data after the change (null for deletes).
    pub after: Option<Value>,
}

/// Source metadata in the Debezium envelope (CONTRACT-006 normative fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcSource {
    /// Axon server version string (CONTRACT-006 `source.version`).
    pub version: String,
    /// Connector type — always `"axon"` (CONTRACT-006 `source.connector`).
    pub connector: String,
    /// Axon instance name (CONTRACT-006 `source.name`).
    pub name: String,
    /// Source timestamp, milliseconds since epoch (CONTRACT-006 `source.ts_ms`).
    pub ts_ms: u64,
    /// Tenant name — ADR-018 isolation boundary (CONTRACT-006 `source.tenant`).
    pub tenant: String,
    /// Database name — FEAT-014 namespace (CONTRACT-006 `source.db`).
    pub db: String,
    /// Schema/namespace name (CONTRACT-006 `source.schema`).
    pub schema: String,
    /// Collection name (CONTRACT-006 `source.collection`).
    pub collection: String,
    /// Entity ID (CONTRACT-006 `source.entity_id`).
    pub entity_id: String,
    /// Monotonic audit sequence number; the consumer offset and dedup key
    /// (CONTRACT-006 `source.audit_id`).
    pub audit_id: u64,
    /// Transaction ID when the mutation was part of a transaction, else `null`
    /// (CONTRACT-006 `source.transaction_id`).
    pub transaction_id: Option<String>,
}

impl CdcEnvelope {
    /// Convert an [`AuditEntry`] to a Debezium-compatible CDC envelope.
    ///
    /// Produces envelopes for entity and link operations (US-077, US-078).
    /// Collection lifecycle and intent events return `None`.
    pub fn from_audit_entry(entry: &AuditEntry, ctx: &CdcContext) -> Option<Self> {
        use crate::entry::MutationType;

        let op = match entry.mutation {
            MutationType::EntityCreate | MutationType::LinkCreate => CdcOp::Create,
            MutationType::EntityUpdate | MutationType::EntityRevert => CdcOp::Update,
            MutationType::EntityDelete | MutationType::LinkDelete => CdcOp::Delete,
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
                version: ctx.axon_version.clone(),
                connector: "axon".into(),
                name: ctx.instance_name.clone(),
                ts_ms,
                tenant: ctx.tenant.clone(),
                db: ctx.db.clone(),
                schema: ctx.schema.clone(),
                collection: entry.collection.to_string(),
                entity_id: entry.entity_id.to_string(),
                audit_id: entry.id,
                transaction_id: entry.transaction_id.clone(),
            },
            op,
            ts_ms,
            before: entry.data_before.clone(),
            after: entry.data_after.clone(),
        })
    }

    /// Create a snapshot read (`op: "r"`) envelope for initial snapshot emission.
    ///
    /// Snapshot reads carry the current full entity state; `before` is always `null`.
    /// See CONTRACT-006 §Cursor semantics — snapshot boundary and replay.
    pub fn snapshot(
        ctx: &CdcContext,
        collection: &str,
        entity_id: &str,
        data: Value,
        audit_id: u64,
        ts_ms: u64,
    ) -> Self {
        CdcEnvelope {
            source: CdcSource {
                version: ctx.axon_version.clone(),
                connector: "axon".into(),
                name: ctx.instance_name.clone(),
                ts_ms,
                tenant: ctx.tenant.clone(),
                db: ctx.db.clone(),
                schema: ctx.schema.clone(),
                collection: collection.to_string(),
                entity_id: entity_id.to_string(),
                audit_id,
                transaction_id: None,
            },
            op: CdcOp::Read,
            ts_ms,
            before: None,
            after: Some(data),
        }
    }
}

/// Trait for CDC event sinks.
pub trait CdcSink: Send {
    /// Emit a CDC event.
    ///
    /// For delete events, Kafka sinks automatically follow with a tombstone
    /// via [`CdcSink::emit_tombstone`] (CONTRACT-006 normative).
    fn emit(&mut self, envelope: &CdcEnvelope) -> Result<(), String>;

    /// Emit a tombstone (same key, null value) for Kafka log compaction.
    ///
    /// CONTRACT-006 normative: every delete MUST be followed by a tombstone.
    /// Non-Kafka sinks default to a no-op; [`KafkaCdcSink`] overrides this.
    fn emit_tombstone(&mut self, _source: &CdcSource) -> Result<(), String> {
        Ok(())
    }

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
///
/// The topic template (CONTRACT-006 §Topic naming) supports the placeholders
/// `{instance}`, `{tenant}`, `{db}`, `{schema}`, and `{collection}`.
/// Values are taken from the [`CdcSource`] embedded in each envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KafkaConfig {
    /// Kafka bootstrap servers (comma-separated).
    pub brokers: String,
    /// Topic name template. Default: `"{instance}.{tenant}.{db}.{schema}.{collection}"`
    pub topic_template: String,
    /// Whether to enable the sink. Default: false.
    pub enabled: bool,
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            brokers: "localhost:9092".into(),
            topic_template: "{instance}.{tenant}.{db}.{schema}.{collection}".into(),
            enabled: false,
        }
    }
}

impl KafkaConfig {
    /// Resolve the topic name from a [`CdcSource`], substituting all template placeholders.
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn topic_for_source(&self, source: &CdcSource) -> String {
        self.topic_template
            .replace("{instance}", &source.name)
            .replace("{tenant}", &source.tenant)
            .replace("{db}", &source.db)
            .replace("{schema}", &source.schema)
            .replace("{collection}", &source.collection)
    }

    /// Compute the CONTRACT-006 Kafka event key as a JSON value.
    ///
    /// The key ensures per-entity partition ordering and includes all
    /// namespace dimensions for multi-tenant isolation.
    ///
    /// Shape: `{ "tenant", "db", "schema", "collection", "id" }`
    pub fn event_key(source: &CdcSource) -> Value {
        serde_json::json!({
            "tenant": source.tenant,
            "db": source.db,
            "schema": source.schema,
            "collection": source.collection,
            "id": source.entity_id,
        })
    }

    /// Serialize the event key to a JSON string for use as a Kafka message key.
    pub fn event_key_string(source: &CdcSource) -> String {
        serde_json::to_string(&Self::event_key(source))
            .unwrap_or_else(|_| source.entity_id.clone())
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
    /// All messages sent: (topic, event_key_json, payload_json).
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
    /// Buffered events: (topic, event_key_json, envelope_json).
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

        let topic = self.config.topic_for_source(&envelope.source);
        let key = KafkaConfig::event_key_string(&envelope.source);
        let json = serde_json::to_string(envelope)
            .map_err(|e| format!("Kafka CDC serialization error: {e}"))?;

        self.sent.push((topic, key, json));

        // CONTRACT-006: deletes MUST be followed by a tombstone for Kafka log compaction.
        if envelope.op == CdcOp::Delete {
            self.emit_tombstone(&envelope.source)?;
        }
        Ok(())
    }

    fn emit_tombstone(&mut self, source: &CdcSource) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(());
        }
        let topic = self.config.topic_for_source(source);
        let key = KafkaConfig::event_key_string(source);
        self.sent.push((topic, key, "null".to_string()));
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
/// produced to the topic resolved by [`KafkaConfig::topic_for_source`], with
/// the CONTRACT-006 JSON event key ensuring per-entity partition ordering.
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

        let topic = self.config.topic_for_source(&envelope.source);
        let key = KafkaConfig::event_key_string(&envelope.source);
        let payload =
            serde_json::to_string(envelope).map_err(|e| format!("CDC serialization error: {e}"))?;

        self.producer
            .send(&topic, &key, &payload)
            .map_err(|e| format!("CdcError::ProducerError: {e}"))?;

        // CONTRACT-006: deletes MUST be followed by a tombstone for Kafka log compaction.
        if envelope.op == CdcOp::Delete {
            self.emit_tombstone(&envelope.source)?;
        }
        Ok(())
    }

    fn emit_tombstone(&mut self, source: &CdcSource) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(());
        }
        let topic = self.config.topic_for_source(source);
        let key = KafkaConfig::event_key_string(source);
        self.producer
            .send(&topic, &key, "null")
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

    fn default_ctx() -> CdcContext {
        CdcContext::default()
    }

    fn tenant_ctx() -> CdcContext {
        CdcContext {
            instance_name: "axon-prod".into(),
            tenant: "acme".into(),
            db: "finance".into(),
            schema: "public".into(),
            axon_version: "0.1.0".into(),
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

    // ── CONTRACT-006 envelope field assertions ────────────────────────────

    #[test]
    fn contract_006_create_all_source_fields_present() {
        let ctx = tenant_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
            "entity create should produce a CDC envelope",
        );

        assert_eq!(envelope.op, CdcOp::Create, "op must be 'c' for create");
        assert!(envelope.before.is_none(), "before must be null for create");
        assert!(envelope.after.is_some(), "after must be present for create");

        let src = &envelope.source;
        assert_eq!(src.version, "0.1.0", "source.version must be Axon server version");
        assert_eq!(src.connector, "axon", "source.connector must be 'axon'");
        assert_eq!(src.name, "axon-prod", "source.name must be instance name");
        assert_eq!(src.ts_ms, 1000, "source.ts_ms must be milliseconds");
        assert_eq!(src.tenant, "acme", "source.tenant required (CONTRACT-006)");
        assert_eq!(src.db, "finance", "source.db required (CONTRACT-006)");
        assert_eq!(src.schema, "public", "source.schema required (CONTRACT-006)");
        assert_eq!(src.collection, "tasks", "source.collection required");
        assert_eq!(src.entity_id, "t-001", "source.entity_id required");
        assert_eq!(src.audit_id, 42, "source.audit_id required");
        assert!(src.transaction_id.is_none(), "source.transaction_id null when no tx");
    }

    #[test]
    fn contract_006_update_all_source_fields_present() {
        let ctx = tenant_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_update_entry(), &ctx),
            "entity update should produce a CDC envelope",
        );

        assert_eq!(envelope.op, CdcOp::Update, "op must be 'u' for update");
        assert!(envelope.before.is_some(), "before must be present for update");
        assert!(envelope.after.is_some(), "after must be present for update");

        let src = &envelope.source;
        assert_eq!(src.version, "0.1.0");
        assert_eq!(src.connector, "axon");
        assert_eq!(src.name, "axon-prod");
        assert_eq!(src.tenant, "acme");
        assert_eq!(src.db, "finance");
        assert_eq!(src.schema, "public");
        assert_eq!(src.collection, "tasks");
        assert_eq!(src.entity_id, "t-001");
        assert_eq!(src.audit_id, 43);
    }

    #[test]
    fn contract_006_delete_all_source_fields_present() {
        let ctx = tenant_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_delete_entry(), &ctx),
            "entity delete should produce a CDC envelope",
        );

        assert_eq!(envelope.op, CdcOp::Delete, "op must be 'd' for delete");
        assert!(envelope.before.is_some(), "before must be present for delete");
        assert!(envelope.after.is_none(), "after must be null for delete");

        let src = &envelope.source;
        assert_eq!(src.version, "0.1.0");
        assert_eq!(src.connector, "axon");
        assert_eq!(src.name, "axon-prod");
        assert_eq!(src.tenant, "acme");
        assert_eq!(src.db, "finance");
        assert_eq!(src.schema, "public");
        assert_eq!(src.collection, "tasks");
        assert_eq!(src.entity_id, "t-001");
        assert_eq!(src.audit_id, 44);
    }

    #[test]
    fn contract_006_snapshot_read_op_r() {
        let ctx = tenant_ctx();
        let envelope = CdcEnvelope::snapshot(
            &ctx,
            "tasks",
            "t-001",
            json!({"title": "hello"}),
            42,
            1000,
        );

        assert_eq!(envelope.op, CdcOp::Read, "snapshot must have op 'r'");
        assert!(envelope.before.is_none(), "snapshot before must be null");
        assert!(envelope.after.is_some(), "snapshot after must carry entity data");

        let src = &envelope.source;
        assert_eq!(src.version, "0.1.0");
        assert_eq!(src.connector, "axon");
        assert_eq!(src.name, "axon-prod");
        assert_eq!(src.ts_ms, 1000);
        assert_eq!(src.tenant, "acme");
        assert_eq!(src.db, "finance");
        assert_eq!(src.schema, "public");
        assert_eq!(src.collection, "tasks");
        assert_eq!(src.entity_id, "t-001");
        assert_eq!(src.audit_id, 42);
        assert!(src.transaction_id.is_none());
    }

    #[test]
    fn contract_006_transaction_id_propagated() {
        let mut entry = sample_create_entry();
        entry.transaction_id = Some("tx-abc-123".into());

        let ctx = tenant_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&entry, &ctx),
            "entry with transaction_id should produce envelope",
        );
        assert_eq!(
            envelope.source.transaction_id.as_deref(),
            Some("tx-abc-123"),
            "source.transaction_id must propagate from AuditEntry",
        );
    }

    #[test]
    fn contract_006_ts_ms_derived_from_timestamp_ns() {
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &default_ctx()),
            "entity create should produce a CDC envelope",
        );
        // 1_000_000_000 ns = 1000 ms
        assert_eq!(envelope.ts_ms, 1000);
        assert_eq!(envelope.source.ts_ms, 1000);
    }

    // ── Tenant/database/schema-aware topic derivation (CONTRACT-006) ──────

    #[test]
    fn contract_006_tenant_aware_topic_derivation() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let ctx = tenant_ctx();
        let entry = sample_create_entry();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&entry, &ctx),
            "create envelope for topic test",
        );
        let topic = config.topic_for_source(&envelope.source);
        assert_eq!(
            topic, "axon-prod.acme.finance.public.tasks",
            "topic must include instance, tenant, db, schema, collection",
        );
    }

    #[test]
    fn contract_006_event_key_json_structure() {
        let ctx = tenant_ctx();
        let entry = sample_create_entry();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&entry, &ctx),
            "create envelope for key test",
        );
        let key = KafkaConfig::event_key(&envelope.source);
        assert_eq!(key["tenant"], "acme");
        assert_eq!(key["db"], "finance");
        assert_eq!(key["schema"], "public");
        assert_eq!(key["collection"], "tasks");
        assert_eq!(key["id"], "t-001");
    }

    #[test]
    fn contract_006_event_key_string_is_valid_json() {
        let ctx = tenant_ctx();
        let entry = sample_create_entry();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&entry, &ctx),
            "create envelope for key string test",
        );
        let key_str = KafkaConfig::event_key_string(&envelope.source);
        let parsed: Value = must_ok(
            serde_json::from_str(&key_str),
            "event key string must be valid JSON",
        );
        assert_eq!(parsed["tenant"], "acme");
        assert_eq!(parsed["id"], "t-001");
    }

    #[test]
    fn contract_006_default_context_uses_default_tenant() {
        let ctx = CdcContext::default();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
            "create with default context",
        );
        assert_eq!(envelope.source.tenant, "default");
        assert_eq!(envelope.source.db, "default");
        assert_eq!(envelope.source.schema, "default");
        assert_eq!(envelope.source.name, "axon");
    }

    // ── Existing envelope behavior ────────────────────────────────────────

    #[test]
    fn create_entry_to_cdc_envelope() {
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &default_ctx()),
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
            CdcEnvelope::from_audit_entry(&sample_update_entry(), &default_ctx()),
            "entity update should produce a CDC envelope",
        );
        assert_eq!(envelope.op, CdcOp::Update);
        assert!(envelope.before.is_some());
        assert!(envelope.after.is_some());
    }

    #[test]
    fn delete_entry_to_cdc_envelope() {
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_delete_entry(), &default_ctx()),
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
        assert!(CdcEnvelope::from_audit_entry(&entry, &default_ctx()).is_none());
    }

    #[test]
    fn memory_sink_collects_events() {
        let mut sink = MemoryCdcSink::default();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &default_ctx()),
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
        let ctx = default_ctx();
        let e1 = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
            "entity create should produce a CDC envelope",
        );
        let e2 = must_some(
            CdcEnvelope::from_audit_entry(&sample_update_entry(), &ctx),
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

        for line in &lines {
            let parsed: CdcEnvelope = must_ok(
                serde_json::from_str(line),
                "JSONL line should parse as CDC envelope",
            );
            assert_eq!(parsed.source.connector, "axon");
        }
    }

    #[test]
    fn cdc_envelope_roundtrip_serialization() {
        let ctx = tenant_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
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
        assert_eq!(parsed.source.connector, "axon");
        assert_eq!(parsed.source.tenant, "acme");
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
            CdcEnvelope::from_audit_entry(&entry, &default_ctx()),
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
            CdcEnvelope::from_audit_entry(&entry, &default_ctx()),
            "link delete should produce a CDC envelope",
        );
        assert_eq!(envelope.op, CdcOp::Delete);
        assert!(envelope.before.is_some());
        assert!(envelope.after.is_none());
    }

    // ── Kafka config tests ────────────────────────────────────────────────

    #[test]
    fn kafka_config_topic_template_tenant_aware() {
        let config = KafkaConfig {
            brokers: "b1:9092,b2:9092".into(),
            topic_template: "{instance}.{tenant}.{db}.{schema}.{collection}".into(),
            enabled: true,
        };
        let ctx = tenant_ctx();
        let entry = sample_create_entry();
        let envelope = CdcEnvelope::from_audit_entry(&entry, &ctx).unwrap();
        assert_eq!(
            config.topic_for_source(&envelope.source),
            "axon-prod.acme.finance.public.tasks",
        );
    }

    #[test]
    fn kafka_config_event_key_has_all_dimensions() {
        let ctx = tenant_ctx();
        let entry = sample_create_entry();
        let envelope = CdcEnvelope::from_audit_entry(&entry, &ctx).unwrap();
        let key = KafkaConfig::event_key(&envelope.source);
        assert_eq!(key["tenant"], "acme");
        assert_eq!(key["db"], "finance");
        assert_eq!(key["schema"], "public");
        assert_eq!(key["collection"], "tasks");
        assert_eq!(key["id"], "t-001");
    }

    #[test]
    fn kafka_config_default() {
        let config = KafkaConfig::default();
        assert_eq!(config.brokers, "localhost:9092");
        assert!(!config.enabled);
        assert!(config.topic_template.contains("{tenant}"), "default template must include tenant");
        assert!(config.topic_template.contains("{instance}"), "default template must include instance");
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
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &default_ctx()),
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
    fn kafka_sink_enabled_records_events_with_tenant_topic() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let ctx = tenant_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
            "entity create should produce a CDC envelope",
        );
        must_ok(
            sink.emit(&envelope),
            "enabled Kafka sink should record the event",
        );
        assert_eq!(sink.sent.len(), 1);

        let (topic, key_str, json) = &sink.sent[0];
        assert_eq!(topic, "axon-prod.acme.finance.public.tasks", "topic must be tenant-aware");

        // Key must be a JSON object with tenant/db/schema/collection/id.
        let key: Value = must_ok(
            serde_json::from_str(key_str),
            "event key must be valid JSON",
        );
        assert_eq!(key["tenant"], "acme");
        assert_eq!(key["collection"], "tasks");
        assert_eq!(key["id"], "t-001");

        let parsed: CdcEnvelope = must_ok(
            serde_json::from_str(json),
            "stored Kafka CDC payload should deserialize",
        );
        assert_eq!(parsed.source.audit_id, 42);
        assert_eq!(parsed.source.connector, "axon");
    }

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn kafka_sink_partitions_by_entity_key() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let ctx = default_ctx();

        let e1 = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
            "entity create should produce a CDC envelope",
        );
        let mut entry2 = sample_update_entry();
        entry2.entity_id = EntityId::new("t-002");
        let e2 = must_some(
            CdcEnvelope::from_audit_entry(&entry2, &ctx),
            "entity update should produce a CDC envelope",
        );

        must_ok(sink.emit(&e1), "Kafka sink should emit first partitioned event");
        must_ok(sink.emit(&e2), "Kafka sink should emit second partitioned event");

        // Keys are JSON strings; parse and check the entity id dimension.
        let k1: Value = serde_json::from_str(&sink.sent[0].1).unwrap();
        let k2: Value = serde_json::from_str(&sink.sent[1].1).unwrap();
        assert_eq!(k1["id"], "t-001");
        assert_eq!(k2["id"], "t-002");
    }

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn contract_006_delete_emits_tombstone_with_null_payload() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let ctx = tenant_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_delete_entry(), &ctx),
            "delete should produce a CDC envelope",
        );
        must_ok(sink.emit(&envelope), "emit should succeed for delete");

        // CONTRACT-006: delete envelope + tombstone = 2 messages
        assert_eq!(sink.sent.len(), 2, "delete must emit envelope then tombstone");

        let (del_topic, del_key, del_payload) = &sink.sent[0];
        let (tomb_topic, tomb_key, tomb_payload) = &sink.sent[1];

        assert_eq!(del_topic, tomb_topic, "tombstone topic must match delete topic");
        // Same key ensures log compaction removes the entity's history
        assert_eq!(del_key, tomb_key, "tombstone key must match delete key (CONTRACT-006)");
        assert_eq!(tomb_payload, "null", "tombstone payload must be null");

        let parsed: CdcEnvelope = must_ok(
            serde_json::from_str(del_payload),
            "first message must be a valid delete envelope",
        );
        assert_eq!(parsed.op, CdcOp::Delete);
    }

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn contract_006_create_does_not_emit_tombstone() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let ctx = default_ctx();
        let envelope = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
            "create should produce a CDC envelope",
        );
        must_ok(sink.emit(&envelope), "emit should succeed for create");
        assert_eq!(sink.sent.len(), 1, "create must emit exactly one message (no tombstone)");
    }

    #[cfg(not(feature = "kafka"))]
    #[test]
    fn kafka_sink_audit_id_cursor() {
        let config = KafkaConfig {
            enabled: true,
            ..KafkaConfig::default()
        };
        let mut sink = KafkaCdcSink::new(config);
        let ctx = default_ctx();

        let e1 = must_some(
            CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx),
            "entity create should produce a CDC envelope",
        );
        let e2 = must_some(
            CdcEnvelope::from_audit_entry(&sample_update_entry(), &ctx),
            "entity update should produce a CDC envelope",
        );
        must_ok(sink.emit(&e1), "Kafka sink should emit first cursor event");
        must_ok(sink.emit(&e2), "Kafka sink should emit second cursor event");

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
            let ctx = tenant_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            assert_eq!(msgs.len(), 1);
            let (topic, key_str, json) = &msgs[0];
            assert_eq!(topic, "axon-prod.acme.finance.public.tasks");

            let key: Value = serde_json::from_str(key_str).unwrap();
            assert_eq!(key["tenant"], "acme");
            assert_eq!(key["id"], "t-001");

            let parsed: CdcEnvelope = serde_json::from_str(json).unwrap();
            assert_eq!(parsed.op, CdcOp::Create, "op should be 'c' for create");
            assert_eq!(parsed.source.audit_id, 42);
            assert_eq!(parsed.source.connector, "axon");
        }

        #[test]
        fn update_event_produces_op_u() {
            let (mut sink, sent) = make_enabled_sink();
            let ctx = default_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_update_entry(), &ctx).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            let (_, _, json) = &msgs[0];
            let parsed: CdcEnvelope = serde_json::from_str(json).unwrap();
            assert_eq!(parsed.op, CdcOp::Update, "op should be 'u' for update");
        }

        #[test]
        fn delete_event_produces_op_d() {
            let (mut sink, sent) = make_enabled_sink();
            let ctx = default_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_delete_entry(), &ctx).unwrap();
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
            let ctx = default_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx).unwrap();
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
            let ctx = default_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx).unwrap();
            sink.emit(&envelope).unwrap();
            assert!(
                sent.lock().unwrap().is_empty(),
                "disabled sink should not call producer"
            );
        }

        #[test]
        fn entity_id_used_as_message_key_dimension() {
            let (mut sink, sent) = make_enabled_sink();
            let ctx = default_ctx();

            let e1 = CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx).unwrap();
            let mut entry2 = sample_update_entry();
            entry2.entity_id = EntityId::new("t-002");
            let e2 = CdcEnvelope::from_audit_entry(&entry2, &ctx).unwrap();

            sink.emit(&e1).unwrap();
            sink.emit(&e2).unwrap();

            let msgs = sent.lock().unwrap();
            let k1: Value = serde_json::from_str(&msgs[0].1).unwrap();
            let k2: Value = serde_json::from_str(&msgs[1].1).unwrap();
            assert_eq!(k1["id"], "t-001", "key 'id' should be entity_id for first event");
            assert_eq!(k2["id"], "t-002", "key 'id' should be entity_id for second event");
        }

        #[test]
        fn contract_006_delete_emits_tombstone_after_delete_envelope() {
            let (mut sink, sent) = make_enabled_sink();
            let ctx = tenant_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_delete_entry(), &ctx).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            assert_eq!(msgs.len(), 2, "delete must emit envelope + tombstone");

            let (del_topic, del_key, del_payload) = &msgs[0];
            let (tomb_topic, tomb_key, tomb_payload) = &msgs[1];

            assert_eq!(del_topic, tomb_topic, "tombstone topic must match delete topic");
            assert_eq!(del_key, tomb_key, "tombstone key must match delete key (CONTRACT-006)");
            assert_eq!(tomb_payload, "null", "tombstone payload must be null");

            let parsed: CdcEnvelope = serde_json::from_str(del_payload).unwrap();
            assert_eq!(parsed.op, CdcOp::Delete, "first message must be the delete envelope");
        }

        #[test]
        fn contract_006_create_does_not_emit_tombstone() {
            let (mut sink, sent) = make_enabled_sink();
            let ctx = default_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx).unwrap();
            sink.emit(&envelope).unwrap();
            assert_eq!(sent.lock().unwrap().len(), 1, "create must not emit tombstone");
        }

        #[test]
        fn debezium_envelope_fields_present() {
            let (mut sink, sent) = make_enabled_sink();
            let ctx = tenant_ctx();
            let envelope = CdcEnvelope::from_audit_entry(&sample_create_entry(), &ctx).unwrap();
            sink.emit(&envelope).unwrap();

            let msgs = sent.lock().unwrap();
            let (_, _, json) = &msgs[0];
            let v: serde_json::Value = serde_json::from_str(json).unwrap();
            assert!(v.get("op").is_some(), "envelope must have 'op' field");
            assert!(v.get("source").is_some(), "envelope must have 'source' field");
            assert!(v.get("ts_ms").is_some(), "envelope must have 'ts_ms' field");
            assert_eq!(v["op"], "c");
            assert_eq!(v["source"]["connector"], "axon");
            assert_eq!(v["source"]["tenant"], "acme");
            assert_eq!(v["source"]["version"], "0.1.0");
        }
    }
}
