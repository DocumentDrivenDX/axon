#![forbid(unsafe_code)]
//! Confluent-compatible schema registry HTTP facade for Axon (CONTRACT-006).
//!
//! Endpoints served (default port 8081):
//! - `GET  /subjects`                                          — list subjects
//! - `GET  /subjects/{subject}/versions`                       — list versions
//! - `GET  /subjects/{subject}/versions/{version}`             — get version (or "latest")
//! - `POST /subjects/{subject}/versions`                       — register schema
//! - `GET  /schemas/ids/{id}`                                  — get schema by global ID
//! - `GET  /config`                                            — get global compatibility
//! - `PUT  /config`                                            — set global compatibility
//! - `GET  /config/{subject}`                                  — get per-subject compatibility
//! - `PUT  /config/{subject}`                                  — set per-subject compatibility
//! - `POST /compatibility/subjects/{subject}/versions/{version}` — test compatibility
//!
//! Subjects map to Axon collections as `{collection}-value`. The global schema ID
//! is a deterministic hash of `(subject, version)` that is stable across restarts.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::debug;

use axon_core::id::CollectionId;
use axon_schema::evolution::{classify, diff_schemas, Compatibility};
use axon_schema::schema::CollectionSchema;
use axon_storage::adapter::StorageAdapter;

// ── Wire types ─────────────────────────────────────────────────────────────

/// A schema entry in the registry (Confluent wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaEntry {
    /// Subject name (e.g. `"tasks-value"`).
    pub subject: String,
    /// Schema version (1-based, monotonically increasing).
    pub version: u32,
    /// Globally unique stable ID derived from `(subject, version)`.
    pub id: u32,
    /// Schema type — always `"JSON"` for Axon.
    #[serde(rename = "schemaType")]
    pub schema_type: String,
    /// The schema as a serialised JSON string.
    pub schema: String,
}

/// Confluent-compatible compatibility level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CompatibilityLevel {
    None,
    #[default]
    Backward,
    Forward,
    Full,
}

/// Body for `PUT /config` and `PUT /config/{subject}`.
#[derive(Debug, Deserialize)]
struct ConfigBody {
    compatibility: CompatibilityLevel,
}

/// Response for `GET /config` and `PUT /config`.
#[derive(Serialize)]
struct ConfigResponse {
    compatibility: CompatibilityLevel,
}

/// Body for `POST /subjects/{subject}/versions`.
#[derive(Debug, Deserialize)]
struct RegisterBody {
    /// The schema as a JSON string (Confluent wire format).
    schema: String,
}

/// Response for `POST /subjects/{subject}/versions` on success.
#[derive(Serialize)]
struct RegisterResponse {
    id: u32,
}

/// Body for `POST /compatibility/subjects/{subject}/versions/{version}`.
#[derive(Debug, Deserialize)]
struct CompatCheckBody {
    schema: String,
}

/// Response for the compatibility check endpoint.
#[derive(Serialize)]
struct CompatCheckResponse {
    is_compatible: bool,
}

/// Confluent-style error envelope.
#[derive(Serialize)]
struct RegistryError {
    error_code: u32,
    message: String,
}

// ── Shared registry state ───────────────────────────────────────────────────

#[derive(Clone)]
pub struct RegistryState {
    storage: Arc<Mutex<Box<dyn StorageAdapter + Send + Sync>>>,
    global_compat: Arc<Mutex<CompatibilityLevel>>,
    subject_compat: Arc<Mutex<HashMap<String, CompatibilityLevel>>>,
}

impl RegistryState {
    pub fn new(storage: Arc<Mutex<Box<dyn StorageAdapter + Send + Sync>>>) -> Self {
        Self {
            storage,
            global_compat: Arc::new(Mutex::new(CompatibilityLevel::Backward)),
            subject_compat: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// ── Stable schema ID ────────────────────────────────────────────────────────

/// Compute a stable, deterministic u32 schema ID from subject and version.
///
/// Uses djb2 hash mixing so the same (subject, version) always maps to the
/// same ID across registry restarts, as required by CONTRACT-006.
pub fn stable_schema_id(subject: &str, version: u32) -> u32 {
    let mut hash: u32 = 5381;
    for byte in subject.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(byte));
    }
    hash.wrapping_mul(33).wrapping_add(version)
}

// ── Subject ↔ collection mapping ────────────────────────────────────────────

fn collection_to_subject(collection: &str) -> String {
    format!("{collection}-value")
}

fn subject_to_collection(subject: &str) -> Option<&str> {
    subject.strip_suffix("-value")
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn schema_to_entry(subject: &str, version: u32, schema: &CollectionSchema) -> SchemaEntry {
    let schema_json = schema
        .entity_schema
        .as_ref()
        .map(|s| serde_json::to_string(s).unwrap_or_default())
        .unwrap_or_else(|| "{}".to_string());

    SchemaEntry {
        subject: subject.to_string(),
        version,
        id: stable_schema_id(subject, version),
        schema_type: "JSON".to_string(),
        schema: schema_json,
    }
}

fn err_response(status: StatusCode, error_code: u32, message: impl Into<String>) -> Response {
    (
        status,
        Json(RegistryError {
            error_code,
            message: message.into(),
        }),
    )
        .into_response()
}

/// Resolve effective compatibility for a subject (subject-level overrides global).
async fn effective_compat(state: &RegistryState, subject: &str) -> CompatibilityLevel {
    let map = state.subject_compat.lock().await;
    if let Some(level) = map.get(subject) {
        return level.clone();
    }
    drop(map);
    state.global_compat.lock().await.clone()
}

/// Check whether `new_schema_value` is compatible with `existing_schema` under `level`.
fn is_schema_compatible(
    existing: Option<&serde_json::Value>,
    proposed: Option<&serde_json::Value>,
    level: &CompatibilityLevel,
) -> bool {
    if *level == CompatibilityLevel::None {
        return true;
    }
    let diff = diff_schemas(existing, proposed);
    let compat = classify(&diff);
    match level {
        CompatibilityLevel::None => true,
        CompatibilityLevel::Backward => compat != Compatibility::Breaking,
        CompatibilityLevel::Forward => compat != Compatibility::Breaking,
        CompatibilityLevel::Full => compat != Compatibility::Breaking,
    }
}

// ── Route handlers ──────────────────────────────────────────────────────────

/// `GET /subjects` — list all registered subjects.
async fn list_subjects(State(state): State<RegistryState>) -> Response {
    let storage = state.storage.lock().await;
    match storage.list_collections() {
        Ok(collections) => {
            let subjects: Vec<String> = collections
                .iter()
                .map(|c| collection_to_subject(c.as_str()))
                .collect();
            Json(subjects).into_response()
        }
        Err(e) => {
            tracing::error!("list_subjects storage error: {e}");
            err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string())
        }
    }
}

/// `GET /subjects/{subject}/versions` — list all versions for a subject.
async fn list_versions(
    State(state): State<RegistryState>,
    Path(subject): Path<String>,
) -> Response {
    let Some(collection_name) = subject_to_collection(&subject) else {
        return err_response(StatusCode::NOT_FOUND, 40401, "Subject not found.");
    };
    let collection = CollectionId::new(collection_name);
    let storage = state.storage.lock().await;
    match storage.list_schema_versions(&collection) {
        Ok(versions) if versions.is_empty() => {
            err_response(StatusCode::NOT_FOUND, 40401, "Subject not found.")
        }
        Ok(versions) => {
            let nums: Vec<u32> = versions.into_iter().map(|(v, _)| v).collect();
            Json(nums).into_response()
        }
        Err(e) => {
            tracing::error!("list_versions error: {e}");
            err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string())
        }
    }
}

/// `GET /subjects/{subject}/versions/{version}` — get a specific version or "latest".
async fn get_version(
    State(state): State<RegistryState>,
    Path((subject, version_str)): Path<(String, String)>,
) -> Response {
    let Some(collection_name) = subject_to_collection(&subject) else {
        return err_response(StatusCode::NOT_FOUND, 40401, "Subject not found.");
    };
    let collection = CollectionId::new(collection_name);
    let storage = state.storage.lock().await;

    let schema = if version_str == "latest" {
        match storage.get_schema(&collection) {
            Ok(Some(s)) => s,
            Ok(None) => return err_response(StatusCode::NOT_FOUND, 40401, "Subject not found."),
            Err(e) => {
                tracing::error!("get_version latest error: {e}");
                return err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string());
            }
        }
    } else {
        let version: u32 = match version_str.parse() {
            Ok(v) => v,
            Err(_) => {
                return err_response(StatusCode::UNPROCESSABLE_ENTITY, 42202, "Invalid version.")
            }
        };
        match storage.get_schema_version(&collection, version) {
            Ok(Some(s)) => s,
            Ok(None) => return err_response(StatusCode::NOT_FOUND, 40402, "Version not found."),
            Err(e) => {
                tracing::error!("get_version error: {e}");
                return err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string());
            }
        }
    };

    let version = schema.version;
    let entry = schema_to_entry(&subject, version, &schema);
    Json(entry).into_response()
}

/// `POST /subjects/{subject}/versions` — register a new schema version.
async fn register_schema(
    State(state): State<RegistryState>,
    Path(subject): Path<String>,
    Json(body): Json<RegisterBody>,
) -> Response {
    debug!("register_schema subject={subject}");

    let Some(collection_name) = subject_to_collection(&subject) else {
        return err_response(StatusCode::NOT_FOUND, 40401, "Subject not found.");
    };

    let proposed_value: serde_json::Value = match serde_json::from_str(&body.schema) {
        Ok(v) => v,
        Err(_) => return err_response(StatusCode::UNPROCESSABLE_ENTITY, 42201, "Invalid schema."),
    };

    let collection = CollectionId::new(collection_name);
    let compat_level = effective_compat(&state, &subject).await;

    let mut storage = state.storage.lock().await;

    // Check compatibility against the current latest schema.
    if compat_level != CompatibilityLevel::None {
        if let Ok(Some(existing)) = storage.get_schema(&collection) {
            if !is_schema_compatible(
                existing.entity_schema.as_ref(),
                Some(&proposed_value),
                &compat_level,
            ) {
                return err_response(
                    StatusCode::CONFLICT,
                    409,
                    "Schema being registered is incompatible with an earlier schema.",
                );
            }
        }
    }

    // Build the updated schema by cloning the existing one or creating a new skeleton.
    let new_schema = match storage.get_schema(&collection) {
        Ok(Some(mut existing)) => {
            existing.entity_schema = Some(proposed_value);
            existing
        }
        Ok(None) => {
            let mut s = CollectionSchema::new(collection.clone());
            s.entity_schema = Some(proposed_value);
            s
        }
        Err(e) => {
            tracing::error!("register_schema get error: {e}");
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string());
        }
    };

    if let Err(e) = storage.put_schema(&new_schema) {
        tracing::error!("register_schema put error: {e}");
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string());
    }

    // Fetch back to get the assigned version number.
    let stored = match storage.get_schema(&collection) {
        Ok(Some(s)) => s,
        _ => {
            return err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                50001,
                "Schema stored but could not be retrieved.",
            );
        }
    };

    let id = stable_schema_id(&subject, stored.version);
    (StatusCode::OK, Json(RegisterResponse { id })).into_response()
}

/// `GET /schemas/ids/{id}` — look up a schema by its global ID.
async fn get_schema_by_id(State(state): State<RegistryState>, Path(id): Path<u32>) -> Response {
    let storage = state.storage.lock().await;
    let collections = match storage.list_collections() {
        Ok(c) => c,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string()),
    };

    for collection in &collections {
        let subject = collection_to_subject(collection.as_str());
        let versions = match storage.list_schema_versions(collection) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for (version, _) in versions {
            if stable_schema_id(&subject, version) == id {
                if let Ok(Some(schema)) = storage.get_schema_version(collection, version) {
                    let schema_json = schema
                        .entity_schema
                        .as_ref()
                        .map(|s| serde_json::to_string(s).unwrap_or_default())
                        .unwrap_or_else(|| "{}".to_string());
                    return Json(serde_json::json!({
                        "schemaType": "JSON",
                        "schema": schema_json
                    }))
                    .into_response();
                }
            }
        }
    }

    err_response(StatusCode::NOT_FOUND, 40403, "Schema not found.")
}

/// `GET /config` — return global compatibility level.
async fn get_global_config(State(state): State<RegistryState>) -> Response {
    let compat = state.global_compat.lock().await.clone();
    Json(ConfigResponse {
        compatibility: compat,
    })
    .into_response()
}

/// `PUT /config` — set global compatibility level.
async fn set_global_config(
    State(state): State<RegistryState>,
    Json(body): Json<ConfigBody>,
) -> Response {
    *state.global_compat.lock().await = body.compatibility.clone();
    Json(ConfigResponse {
        compatibility: body.compatibility,
    })
    .into_response()
}

/// `GET /config/{subject}` — return per-subject compatibility, falling back to global.
async fn get_subject_config(
    State(state): State<RegistryState>,
    Path(subject): Path<String>,
) -> Response {
    let compat = effective_compat(&state, &subject).await;
    Json(ConfigResponse {
        compatibility: compat,
    })
    .into_response()
}

/// `PUT /config/{subject}` — set per-subject compatibility level.
async fn set_subject_config(
    State(state): State<RegistryState>,
    Path(subject): Path<String>,
    Json(body): Json<ConfigBody>,
) -> Response {
    state
        .subject_compat
        .lock()
        .await
        .insert(subject, body.compatibility.clone());
    Json(ConfigResponse {
        compatibility: body.compatibility,
    })
    .into_response()
}

/// `POST /compatibility/subjects/{subject}/versions/{version}` — check schema compatibility.
async fn check_compatibility(
    State(state): State<RegistryState>,
    Path((subject, version_str)): Path<(String, String)>,
    Json(body): Json<CompatCheckBody>,
) -> Response {
    let Some(collection_name) = subject_to_collection(&subject) else {
        return err_response(StatusCode::NOT_FOUND, 40401, "Subject not found.");
    };

    let proposed: serde_json::Value = match serde_json::from_str(&body.schema) {
        Ok(v) => v,
        Err(_) => return err_response(StatusCode::UNPROCESSABLE_ENTITY, 42201, "Invalid schema."),
    };

    let collection = CollectionId::new(collection_name);
    let storage = state.storage.lock().await;

    let existing_schema = if version_str == "latest" {
        match storage.get_schema(&collection) {
            Ok(s) => s,
            Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string()),
        }
    } else {
        let version: u32 = match version_str.parse() {
            Ok(v) => v,
            Err(_) => {
                return err_response(StatusCode::UNPROCESSABLE_ENTITY, 42202, "Invalid version.");
            }
        };
        match storage.get_schema_version(&collection, version) {
            Ok(s) => s,
            Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, 50001, e.to_string()),
        }
    };

    drop(storage);

    let compat_level = effective_compat(&state, &subject).await;
    let existing_value = existing_schema.and_then(|s| s.entity_schema);
    let compatible = is_schema_compatible(existing_value.as_ref(), Some(&proposed), &compat_level);

    Json(CompatCheckResponse {
        is_compatible: compatible,
    })
    .into_response()
}

// ── Router builder ──────────────────────────────────────────────────────────

/// Build the Confluent-compatible schema registry axum [`Router`].
///
/// Mount at the root of a server (port 8081 by default) or nest under a prefix.
pub fn registry_router(state: RegistryState) -> Router {
    Router::new()
        .route("/subjects", get(list_subjects))
        .route(
            "/subjects/{subject}/versions",
            get(list_versions).post(register_schema),
        )
        .route("/subjects/{subject}/versions/{version}", get(get_version))
        .route("/schemas/ids/{id}", get(get_schema_by_id))
        .route("/config", get(get_global_config).put(set_global_config))
        .route(
            "/config/{subject}",
            get(get_subject_config).put(set_subject_config),
        )
        .route(
            "/compatibility/subjects/{subject}/versions/{version}",
            post(check_compatibility),
        )
        .with_state(state)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axon_storage::MemoryStorageAdapter;
    use axum_test::TestServer;
    use serde_json::{json, Value};

    fn test_state() -> RegistryState {
        let storage: Box<dyn StorageAdapter + Send + Sync> =
            Box::new(MemoryStorageAdapter::default());
        RegistryState::new(Arc::new(Mutex::new(storage)))
    }

    fn test_server() -> TestServer {
        TestServer::new(registry_router(test_state()))
    }

    #[tokio::test]
    async fn get_subjects_empty() {
        let server = test_server();
        let resp = server.get("/subjects").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body, json!([]));
    }

    #[tokio::test]
    async fn register_and_list_subject() {
        let state = test_state();
        // Pre-register the collection so list_collections returns it.
        {
            let mut storage = state.storage.lock().await;
            storage
                .register_collection(&CollectionId::new("tasks"))
                .unwrap();
        }
        let server = TestServer::new(registry_router(state));

        // Register a schema.
        let resp = server
            .post("/subjects/tasks-value/versions")
            .json(&json!({
                "schemaType": "JSON",
                "schema": r#"{"type":"object","properties":{"title":{"type":"string"}}}"#
            }))
            .await;
        resp.assert_status_ok();
        let reg: Value = resp.json();
        assert!(reg["id"].as_u64().unwrap() > 0);

        // List subjects should now include tasks-value.
        let resp = server.get("/subjects").await;
        resp.assert_status_ok();
        let subjects: Vec<String> = resp.json();
        assert!(subjects.contains(&"tasks-value".to_string()));
    }

    #[tokio::test]
    async fn list_versions_not_found() {
        let server = test_server();
        let resp = server.get("/subjects/unknown-value/versions").await;
        resp.assert_status(StatusCode::NOT_FOUND);
        let body: Value = resp.json();
        assert_eq!(body["error_code"], 40401);
    }

    #[tokio::test]
    async fn get_version_latest_not_found() {
        let server = test_server();
        let resp = server.get("/subjects/noexist-value/versions/latest").await;
        resp.assert_status(StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn schema_id_stable_across_calls() {
        let id1 = stable_schema_id("tasks-value", 1);
        let id2 = stable_schema_id("tasks-value", 1);
        assert_eq!(id1, id2, "schema ID must be deterministic");
    }

    #[tokio::test]
    async fn schema_id_differs_by_version() {
        assert_ne!(
            stable_schema_id("tasks-value", 1),
            stable_schema_id("tasks-value", 2)
        );
    }

    #[tokio::test]
    async fn schema_id_differs_by_subject() {
        assert_ne!(
            stable_schema_id("tasks-value", 1),
            stable_schema_id("users-value", 1)
        );
    }

    #[tokio::test]
    async fn global_config_default_is_backward() {
        let server = test_server();
        let resp = server.get("/config").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["compatibility"], "BACKWARD");
    }

    #[tokio::test]
    async fn set_global_config_none() {
        let server = test_server();
        let resp = server
            .put("/config")
            .json(&json!({"compatibility": "NONE"}))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["compatibility"], "NONE");

        // Verify it persisted.
        let resp = server.get("/config").await;
        let body: Value = resp.json();
        assert_eq!(body["compatibility"], "NONE");
    }

    #[tokio::test]
    async fn set_subject_config_overrides_global() {
        let server = test_server();
        server
            .put("/config")
            .json(&json!({"compatibility": "BACKWARD"}))
            .await;
        server
            .put("/config/tasks-value")
            .json(&json!({"compatibility": "NONE"}))
            .await;

        let resp = server.get("/config/tasks-value").await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["compatibility"], "NONE");

        // Global is unchanged.
        let resp = server.get("/config").await;
        let body: Value = resp.json();
        assert_eq!(body["compatibility"], "BACKWARD");
    }

    #[tokio::test]
    async fn compatibility_check_no_existing_schema_is_compatible() {
        let server = test_server();
        let resp = server
            .post("/compatibility/subjects/tasks-value/versions/latest")
            .json(&json!({
                "schemaType": "JSON",
                "schema": r#"{"type":"object"}"#
            }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["is_compatible"], true);
    }

    #[tokio::test]
    async fn get_schema_by_id_not_found() {
        let server = test_server();
        let resp = server.get("/schemas/ids/99999999").await;
        resp.assert_status(StatusCode::NOT_FOUND);
        let body: Value = resp.json();
        assert_eq!(body["error_code"], 40403);
    }

    #[tokio::test]
    async fn subject_to_collection_roundtrip() {
        let subject = collection_to_subject("tasks");
        assert_eq!(subject, "tasks-value");
        assert_eq!(subject_to_collection(&subject), Some("tasks"));
    }

    #[tokio::test]
    async fn subject_without_suffix_returns_none() {
        assert_eq!(subject_to_collection("tasks"), None);
        assert_eq!(subject_to_collection(""), None);
    }
}
