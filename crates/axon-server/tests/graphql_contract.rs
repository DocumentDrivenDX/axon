//! GraphQL API integration tests.
//!
//! Exercise every GraphQL endpoint — queries, mutations, error paths,
//! introspection, and the playground — against an in-process server backed by
//! SQLite in-memory storage. No mocks, no Playwright: pure HTTP round-trips
//! through the same router used in production.
//!
//! Collection used throughout: "item" → GraphQL type "Item"
//!   Queries:   item(id: ID!), items(limit: Int, afterId: ID)
//!   Mutations: createItem, updateItem, patchItem, deleteItem
//!
//! Each test creates its own server instance for full isolation.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;

// ── Server helpers ─────────────────────────────────────────────────────────────

fn test_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// Register a collection with a simple label field via the REST API so it
/// shows up in the per-request GraphQL schema.
async fn seed_collection(server: &axum_test::TestServer, name: &str) {
    server
        .post(&format!("/tenants/default/databases/default/collections/{name}"))
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// Register a collection whose schema enforces a minLength:3 constraint on
/// `title` — used by schema validation error tests.
async fn seed_constrained_collection(server: &axum_test::TestServer, name: &str) {
    server
        .post(&format!("/tenants/default/databases/default/collections/{name}"))
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "required": ["title"],
                    "properties": {
                        "title": { "type": "string", "minLength": 3 }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// POST a GraphQL document and return the parsed JSON response body.
/// GraphQL always returns HTTP 200; errors live in the `errors` field of the body.
async fn gql(server: &axum_test::TestServer, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .json(&json!({"query": query}))
        .await
        .json::<Value>()
}

// ── Playground ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_playground_returns_html() {
    let server = test_server();
    let resp = server.get("/graphql/playground").await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(
        body.contains("GraphQL"),
        "playground response should contain 'GraphQL'"
    );
}

// ── Introspection ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_introspection_root_types() {
    let server = test_server();
    // At least one collection is required — async-graphql rejects a Query with no fields.
    seed_collection(&server, "item").await;
    let body = gql(
        &server,
        r"{ __schema { queryType { name } mutationType { name } subscriptionType { name } } }",
    )
    .await;

    assert_eq!(body["data"]["__schema"]["queryType"]["name"], "Query");
    assert_eq!(body["data"]["__schema"]["mutationType"]["name"], "Mutation");
    // No broker passed to build_router → no Subscription type.
    assert!(
        body["data"]["__schema"]["subscriptionType"].is_null(),
        "subscriptionType should be null without a broker: {body}"
    );
}

#[tokio::test]
async fn graphql_collection_type_visible_after_creation() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r"{ __schema { queryType { fields { name } } } }",
    )
    .await;

    let fields = body["data"]["__schema"]["queryType"]["fields"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();

    assert!(names.contains(&"item"), "should have singular query 'item'");
    assert!(names.contains(&"items"), "should have plural query 'items'");
}

#[tokio::test]
async fn graphql_mutation_fields_registered_per_collection() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r"{ __schema { mutationType { fields { name } } } }",
    )
    .await;

    let fields = body["data"]["__schema"]["mutationType"]["fields"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();

    assert!(names.contains(&"createItem"));
    assert!(names.contains(&"updateItem"));
    assert!(names.contains(&"patchItem"));
    assert!(names.contains(&"deleteItem"));
}

#[tokio::test]
async fn graphql_multiple_collections_in_schema() {
    let server = test_server();
    seed_collection(&server, "item").await;
    seed_collection(&server, "note").await;

    let body = gql(
        &server,
        r"{ __schema { queryType { fields { name } } } }",
    )
    .await;

    let fields = body["data"]["__schema"]["queryType"]["fields"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();

    assert!(names.contains(&"item"));
    assert!(names.contains(&"items"));
    assert!(names.contains(&"note"));
    assert!(names.contains(&"notes"));
}

// ── Queries ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_get_missing_entity_returns_null() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(&server, r#"{ item(id: "ghost") { id } }"#).await;

    // No error — missing entity resolves to null.
    assert!(
        body["errors"].is_null(),
        "no errors expected for missing entity: {body}"
    );
    assert!(
        body["data"]["item"].is_null(),
        "missing entity should resolve to null: {body}"
    );
}

#[tokio::test]
async fn graphql_list_empty_before_any_entities() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(&server, r"{ items { id } }").await;

    let list = body["data"]["items"].as_array().unwrap();
    assert!(list.is_empty(), "no entities created yet — list should be empty");
}

// ── Mutations: create ─────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_create_entity_returns_system_fields() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r#"mutation { createItem(id: "e1", input: "{\"label\":\"hello\"}") { id version } }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["createItem"];
    assert_eq!(entity["id"], "e1");
    assert_eq!(entity["version"], 1);
}

#[tokio::test]
async fn graphql_get_entity_after_create() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "e2", input: "{\"label\":\"world\"}") { id } }"#,
    )
    .await;

    let body = gql(&server, r#"{ item(id: "e2") { id version label } }"#).await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["item"];
    assert_eq!(entity["id"], "e2");
    assert_eq!(entity["version"], 1);
    assert_eq!(entity["label"], "world");
}

// ── Queries: list + pagination ────────────────────────────────────────────────

#[tokio::test]
async fn graphql_list_entities_after_creates() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=3_u32 {
        gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "li-{i:02}", input: "{{\"label\":\"L{i}\"}}") {{ id }} }}"#
            ),
        )
        .await;
    }

    let body = gql(&server, r"{ items { id } }").await;
    let list = body["data"]["items"].as_array().unwrap();
    assert_eq!(list.len(), 3, "should list all 3 created entities");
}

#[tokio::test]
async fn graphql_list_with_limit() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=5_u32 {
        gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "pg-{i:02}", input: "{{\"label\":\"P{i}\"}}") {{ id }} }}"#
            ),
        )
        .await;
    }

    let body = gql(&server, r"{ items(limit: 2) { id } }").await;
    let list = body["data"]["items"].as_array().unwrap();
    assert_eq!(list.len(), 2, "limit: 2 should return exactly 2 items");
}

#[tokio::test]
async fn graphql_list_pagination_via_after_id() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=4_u32 {
        gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "ap-{i:02}", input: "{{\"label\":\"A{i}\"}}") {{ id }} }}"#
            ),
        )
        .await;
    }

    // First page: 2 items.
    let page1 = gql(&server, r"{ items(limit: 2) { id } }").await;
    let page1_items = page1["data"]["items"].as_array().unwrap();
    assert_eq!(page1_items.len(), 2, "first page should return 2 items");

    let last_id = page1_items.last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Second page: remaining 2 items.
    let page2 = gql(
        &server,
        &format!(r#"{{ items(limit: 2, afterId: "{last_id}") {{ id }} }}"#),
    )
    .await;
    let page2_items = page2["data"]["items"].as_array().unwrap();
    assert!(!page2_items.is_empty(), "second page should have items");

    // No ID should appear in both pages.
    for item in page2_items {
        let id = item["id"].as_str().unwrap();
        assert!(
            !page1_items.iter().any(|p| p["id"] == id),
            "item {id} appeared in both pages"
        );
    }
}

// ── Mutations: update ─────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_update_entity_success() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "upd-1", input: "{\"label\":\"v1\"}") { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { updateItem(id: "upd-1", version: 1, input: "{\"label\":\"v2\"}") { id version label } }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["updateItem"];
    assert_eq!(entity["id"], "upd-1");
    assert_eq!(entity["version"], 2);
    assert_eq!(entity["label"], "v2");
}

#[tokio::test]
async fn graphql_update_version_conflict_error_code() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "occ-1", input: "{\"label\":\"v1\"}") { id } }"#,
    )
    .await;

    // Submit with wrong expected version (99 instead of 1).
    let body = gql(
        &server,
        r#"mutation { updateItem(id: "occ-1", version: 99, input: "{\"label\":\"v2\"}") { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(!errors.is_empty(), "expected VERSION_CONFLICT error");
    assert_eq!(
        errors[0]["extensions"]["code"].as_str().unwrap(),
        "VERSION_CONFLICT",
        "error code should be VERSION_CONFLICT: {body}"
    );
}

// ── Mutations: patch ──────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_patch_entity_success() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "pat-1", input: "{\"label\":\"original\"}") { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { patchItem(id: "pat-1", version: 1, patch: "{\"label\":\"patched\"}") { id version label } }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["patchItem"];
    assert_eq!(entity["id"], "pat-1");
    assert_eq!(entity["version"], 2);
    assert_eq!(entity["label"], "patched");
}

#[tokio::test]
async fn graphql_patch_version_conflict_error_code() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "poc-1", input: "{\"label\":\"v1\"}") { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { patchItem(id: "poc-1", version: 99, patch: "{\"label\":\"x\"}") { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(!errors.is_empty(), "expected VERSION_CONFLICT error");
    assert_eq!(
        errors[0]["extensions"]["code"].as_str().unwrap(),
        "VERSION_CONFLICT",
        "error code should be VERSION_CONFLICT: {body}"
    );
}

// ── Mutations: delete ─────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_delete_entity_returns_true() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "del-1", input: "{\"label\":\"bye\"}") { id } }"#,
    )
    .await;

    let body = gql(&server, r#"mutation { deleteItem(id: "del-1") }"#).await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    assert_eq!(
        body["data"]["deleteItem"], true,
        "delete should return true: {body}"
    );
}

#[tokio::test]
async fn graphql_get_after_delete_returns_null() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "del-2", input: "{\"label\":\"gone\"}") { id } }"#,
    )
    .await;
    gql(&server, r#"mutation { deleteItem(id: "del-2") }"#).await;

    let body = gql(&server, r#"{ item(id: "del-2") { id } }"#).await;

    assert!(
        body["errors"].is_null(),
        "no error expected for missing entity: {body}"
    );
    assert!(
        body["data"]["item"].is_null(),
        "deleted entity should resolve to null: {body}"
    );
}

// ── Error paths ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn graphql_schema_validation_error() {
    let server = test_server();
    // title must be at least 3 characters.
    seed_constrained_collection(&server, "item").await;

    // Submit a title that is too short (2 chars → fails minLength: 3).
    let body = gql(
        &server,
        r#"mutation { createItem(id: "bad-1", input: "{\"title\":\"ab\"}") { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(!errors.is_empty(), "expected SCHEMA_VALIDATION error: {body}");
    assert_eq!(
        errors[0]["extensions"]["code"].as_str().unwrap(),
        "SCHEMA_VALIDATION",
        "error code should be SCHEMA_VALIDATION: {body}"
    );
}

#[tokio::test]
async fn graphql_invalid_json_input_returns_error() {
    let server = test_server();
    seed_collection(&server, "item").await;

    // The `input` argument must be valid JSON — pass a broken string.
    let body = gql(
        &server,
        r#"mutation { createItem(id: "bad-json", input: "not{{json") { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(!errors.is_empty(), "expected a parse error for invalid JSON input");
    let msg = errors[0]["message"].as_str().unwrap();
    assert!(
        msg.to_lowercase().contains("json"),
        "error message should mention JSON, got: {msg}"
    );
}
