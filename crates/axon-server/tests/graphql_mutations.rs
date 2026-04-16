//! GraphQL mutation integration tests (FEAT-015, salvage of eitri-apr13).
//!
//! Exercises the `/graphql` endpoint end-to-end through a real axum test
//! server so that the wiring between HTTP → `resolve_caller_identity`
//! middleware → dynamic GraphQL schema builder → `AxonHandler` is validated
//! as a single pipeline. Unit tests on the handler or the schema builder
//! alone do not cover this integration.
//!
//! Tests here intentionally use `/graphql` rather than the REST
//! `/entities/*` routes so the coverage is specific to the GraphQL transport
//! and the `_with_caller` wrappers invoked by the mutation resolvers.

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_api::request::CreateCollectionRequest;
use axon_core::id::CollectionId;
use axon_schema::schema::{CollectionSchema, LifecycleDef};
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use serde_json::{json, Value};
use tokio::sync::Mutex;

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// Build a test server with a `tasks` collection whose schema has no lifecycle
/// and whose entity schema allows free-form fields. The shared handler is
/// returned so tests that need to pre-seed state may do so before the server
/// accepts requests.
fn test_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// Build a test server with a `tasks` collection that has a `status`
/// lifecycle: `draft -> submitted -> approved`.
///
/// The collection is installed directly on the handler before the test server
/// is constructed because the HTTP create-collection route does not yet
/// expose the `lifecycles` field.
async fn lifecycle_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));

    let mut transitions = HashMap::new();
    transitions.insert("draft".to_string(), vec!["submitted".to_string()]);
    transitions.insert("submitted".to_string(), vec!["approved".to_string()]);
    transitions.insert("approved".to_string(), vec![]);

    let lifecycle = LifecycleDef {
        field: "status".to_string(),
        initial: "draft".to_string(),
        transitions,
    };

    let mut lifecycles = HashMap::new();
    lifecycles.insert("status".to_string(), lifecycle);

    let mut schema = CollectionSchema::new(CollectionId::new("tasks"));
    schema.entity_schema = Some(json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "status": { "type": "string" }
        }
    }));
    schema.lifecycles = lifecycles;

    handler
        .lock()
        .await
        .create_collection(CreateCollectionRequest {
            name: CollectionId::new("tasks"),
            schema,
            actor: Some("test-setup".into()),
        })
        .expect("create_collection should succeed");

    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// Register a plain `tasks` collection via the REST create-collection route.
async fn seed_tasks_collection(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/tasks")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "label": { "type": "string" }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// POST a GraphQL document and return the parsed JSON response body.
async fn gql(server: &axum_test::TestServer, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

/// Same as [`gql`] but attaches an `x-axon-actor` header so the gateway's
/// identity middleware records the request as coming from the given actor.
async fn gql_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .add_header("x-axon-actor", actor)
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

// ── Happy-path mutations ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_create_entity_mutation_happy_path() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    let create_body = gql(
        &server,
        r#"mutation { createTasks(id: "t1", input: "{\"title\":\"ship it\"}") { id version title } }"#,
    )
    .await;
    assert!(
        create_body["errors"].is_null(),
        "unexpected errors: {create_body}"
    );
    assert_eq!(create_body["data"]["createTasks"]["id"], "t1");
    assert_eq!(create_body["data"]["createTasks"]["version"], 1);
    assert_eq!(create_body["data"]["createTasks"]["title"], "ship it");

    // Entity must be visible through a subsequent get query.
    let get_body = gql(&server, r#"{ tasks(id: "t1") { id version title } }"#).await;
    assert!(
        get_body["errors"].is_null(),
        "unexpected errors: {get_body}"
    );
    assert_eq!(get_body["data"]["tasks"]["id"], "t1");
    assert_eq!(get_body["data"]["tasks"]["version"], 1);
    assert_eq!(get_body["data"]["tasks"]["title"], "ship it");
}

// ── Error contracts ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_update_entity_version_conflict_returns_structured_error() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    gql(
        &server,
        r#"mutation { createTasks(id: "t1", input: "{\"title\":\"v1\"}") { id } }"#,
    )
    .await;

    // Stale expected_version (99) — must produce a VERSION_CONFLICT error
    // with the structured currentEntity extension.
    let body = gql(
        &server,
        r#"mutation { updateTasks(id: "t1", version: 99, input: "{\"title\":\"stale\"}") { id } }"#,
    )
    .await;

    let errors = body["errors"]
        .as_array()
        .expect("errors array for stale version");
    assert!(!errors.is_empty(), "expected errors: {body}");
    let ext = &errors[0]["extensions"];
    assert_eq!(
        ext["code"].as_str().unwrap(),
        "VERSION_CONFLICT",
        "error code should be VERSION_CONFLICT: {body}"
    );
    assert_eq!(ext["expected"], 99);
    assert_eq!(ext["actual"], 1);
    assert_eq!(
        ext["currentEntity"]["version"], 1,
        "currentEntity extension should expose the live version: {body}"
    );
    assert_eq!(ext["currentEntity"]["id"], "t1");
    assert_eq!(ext["currentEntity"]["data"]["title"], "v1");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_delete_entity_mutation() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    gql(
        &server,
        r#"mutation { createTasks(id: "del-1", input: "{\"title\":\"bye\"}") { id } }"#,
    )
    .await;

    let del_body = gql(&server, r#"mutation { deleteTasks(id: "del-1") }"#).await;
    assert!(
        del_body["errors"].is_null(),
        "unexpected errors: {del_body}"
    );
    assert_eq!(del_body["data"]["deleteTasks"], true);

    let get_body = gql(&server, r#"{ tasks(id: "del-1") { id } }"#).await;
    assert!(
        get_body["errors"].is_null(),
        "unexpected errors: {get_body}"
    );
    assert!(
        get_body["data"]["tasks"].is_null(),
        "deleted entity should be null: {get_body}"
    );
}

// ── Lifecycle transitions ────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_transition_lifecycle_mutation() {
    let server = lifecycle_server().await;

    // Create a task. The lifecycle field is auto-populated with "draft".
    let create_body = gql(
        &server,
        r#"mutation { createTasks(id: "t-100", input: "{\"title\":\"design\"}") { id version status } }"#,
    )
    .await;
    assert!(
        create_body["errors"].is_null(),
        "unexpected errors: {create_body}"
    );
    assert_eq!(create_body["data"]["createTasks"]["status"], "draft");

    // Transition draft -> submitted.
    let transition_body = gql(
        &server,
        r#"mutation {
            transitionTasksLifecycle(
                id: "t-100",
                lifecycleName: "status",
                targetState: "submitted",
                expectedVersion: 1
            ) { id version status title }
        }"#,
    )
    .await;
    assert!(
        transition_body["errors"].is_null(),
        "unexpected errors: {transition_body}"
    );
    assert_eq!(
        transition_body["data"]["transitionTasksLifecycle"]["version"],
        2
    );
    assert_eq!(
        transition_body["data"]["transitionTasksLifecycle"]["status"],
        "submitted"
    );
    assert_eq!(
        transition_body["data"]["transitionTasksLifecycle"]["title"],
        "design"
    );

    // New state must be visible from a subsequent read.
    let get_body = gql(&server, r#"{ tasks(id: "t-100") { id version status } }"#).await;
    assert_eq!(get_body["data"]["tasks"]["version"], 2);
    assert_eq!(get_body["data"]["tasks"]["status"], "submitted");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_invalid_transition_error_has_valid_transitions_extension() {
    let server = lifecycle_server().await;

    gql(
        &server,
        r#"mutation { createTasks(id: "t-bad", input: "{\"title\":\"x\"}") { id } }"#,
    )
    .await;

    // Attempt draft -> approved directly (not allowed; only draft -> submitted).
    let body = gql(
        &server,
        r#"mutation {
            transitionTasksLifecycle(
                id: "t-bad",
                lifecycleName: "status",
                targetState: "approved",
                expectedVersion: 1
            ) { id }
        }"#,
    )
    .await;

    let errors = body["errors"]
        .as_array()
        .expect("errors array for invalid transition");
    assert!(!errors.is_empty(), "expected errors: {body}");
    let ext = &errors[0]["extensions"];
    assert_eq!(
        ext["code"].as_str().unwrap(),
        "INVALID_TRANSITION",
        "error code should be INVALID_TRANSITION: {body}"
    );
    assert_eq!(ext["lifecycleName"], "status");
    assert_eq!(ext["currentState"], "draft");
    assert_eq!(ext["targetState"], "approved");
    let valid = ext["validTransitions"]
        .as_array()
        .expect("validTransitions must be a list");
    let valid_strings: Vec<&str> = valid.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        valid_strings,
        vec!["submitted"],
        "valid_transitions should list only `submitted`: {body}"
    );
}

// ── Caller identity propagation ──────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_mutation_respects_caller_identity() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    let body = gql_as(
        &server,
        "agent-1",
        r#"mutation { createTasks(id: "aud-1", input: "{\"title\":\"hello\"}") { id } }"#,
    )
    .await;
    assert!(body["errors"].is_null(), "unexpected errors: {body}");

    // Verify the audit entry for this entity was attributed to `agent-1`.
    let audit = server
        .get("/tenants/default/databases/default/audit/entity/tasks/aud-1")
        .await
        .json::<Value>();
    let entries = audit["entries"].as_array().expect("audit entries array");
    assert!(
        entries.iter().any(|e| e["actor"] == "agent-1"),
        "expected an audit entry attributed to agent-1, got: {audit}"
    );
}
