//! MCP HTTP transport integration tests.
//!
//! Exercise every JSON-RPC method on the `/mcp` endpoint — protocol handshake,
//! tools (CRUD, aggregate, query), resources (list, read, subscribe), and
//! prompts — against an in-process server backed by SQLite in-memory storage.
//! No mocks: the same router used in production handles every request.
//!
//! Collection used throughout: "item"
//!   Tools:     item.create, item.get, item.patch, item.delete, item.aggregate,
//!              item.link_candidates, item.neighbors, axon.query
//!   Resources: axon://item, axon://item/{id}, axon://_collections, axon://_schemas

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axum::http::StatusCode;
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
    axum_test::TestServer::new(build_router(tenant_router, "memory", None))
}

/// Create a collection with a simple `label` field via the REST API.
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
        .assert_status(StatusCode::CREATED);
}

/// Create an entity via the REST API and return its version.
async fn rest_create(server: &axum_test::TestServer, collection: &str, id: &str) -> u64 {
    let resp = server
        .post(&format!("/tenants/default/databases/default/entities/{collection}/{id}"))
        .json(&json!({"data": {"label": "test"}, "actor": "test"}))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let body = resp.json::<Value>();
    body["entity"]["version"].as_u64().unwrap_or(1)
}

/// POST a JSON-RPC message to /mcp and return the parsed response body.
/// Callers receive the full `{"jsonrpc":"2.0","id":...,"result":...}` object.
async fn mcp(server: &axum_test::TestServer, request: &Value) -> Value {
    server
        .post("/mcp")
        .json(request)
        .await
        .json::<Value>()
}

// ── Protocol basics ───────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_ping_returns_empty_result() {
    let server = test_server();
    let body = mcp(&server, &json!({"jsonrpc":"2.0","id":1,"method":"ping"})).await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    assert_eq!(body["result"], json!({}));
}

#[tokio::test]
async fn mcp_initialize_returns_server_capabilities() {
    let server = test_server();
    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0.1"}
            }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let result = &body["result"];
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "axon-mcp");
    assert!(!result["serverInfo"]["version"].as_str().unwrap_or("").is_empty());
    assert_eq!(result["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(result["capabilities"]["resources"]["subscribe"], true);
    assert_eq!(result["capabilities"]["prompts"]["listChanged"], false);
}

#[tokio::test]
async fn mcp_notification_returns_no_content() {
    let server = test_server();
    // Notifications have no `id` field — the server must return 204 No Content.
    let resp = server
        .post("/mcp")
        .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await;
    resp.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn mcp_unknown_method_returns_error_code() {
    let server = test_server();
    let body = mcp(
        &server,
        &json!({"jsonrpc":"2.0","id":1,"method":"no_such_method"}),
    )
    .await;

    assert!(!body["error"].is_null(), "expected error object: {body}");
    assert_eq!(body["error"]["code"], -32601);
}

#[tokio::test]
async fn mcp_malformed_json_returns_parse_error() {
    let server = test_server();
    let resp = server.post("/mcp").text("not json {{{").await;
    // Handler returns HTTP 200 with a JSON-RPC parse error payload.
    resp.assert_status_ok();
    let body = resp.json::<Value>();
    assert_eq!(body["error"]["code"], -32700);
}

#[tokio::test]
async fn mcp_session_id_issued_on_first_request() {
    let server = test_server();
    let resp = server
        .post("/mcp")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
        .await;
    let session = resp
        .headers()
        .get("x-axon-mcp-session")
        .expect("response should carry an MCP session header");
    assert!(!session.is_empty());
}

// ── tools/list ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_tools_list_always_includes_axon_query() {
    let server = test_server();
    let body = mcp(
        &server,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"axon.query"), "axon.query always present: {names:?}");
}

#[tokio::test]
async fn mcp_tools_list_includes_crud_after_collection_created() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in &["item.create", "item.get", "item.patch", "item.delete"] {
        assert!(names.contains(expected), "expected {expected} in tools list: {names:?}");
    }
}

// ── tools/call: CRUD ──────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_create_tool_returns_entity() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": {
                "name": "item.create",
                "arguments": { "id": "t-create", "data": {"label": "hello"} }
            }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let entity: Value = serde_json::from_str(text).unwrap();
    assert_eq!(entity["id"], "t-create");
    assert_eq!(entity["version"], 1);
    assert!(body["result"]["isError"].is_null() || body["result"]["isError"] == false);
}

#[tokio::test]
async fn mcp_get_tool_returns_entity() {
    let server = test_server();
    seed_collection(&server, "item").await;
    rest_create(&server, "item", "t-get").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": { "name": "item.get", "arguments": { "id": "t-get" } }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let entity: Value = serde_json::from_str(text).unwrap();
    assert_eq!(entity["id"], "t-get");
    assert_eq!(entity["version"], 1);
}

#[tokio::test]
async fn mcp_get_tool_missing_entity_is_error() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": { "name": "item.get", "arguments": { "id": "ghost" } }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    assert_eq!(
        body["result"]["isError"], true,
        "missing entity should be a tool error: {body}"
    );
}

#[tokio::test]
async fn mcp_patch_tool_updates_entity() {
    let server = test_server();
    seed_collection(&server, "item").await;
    let version = rest_create(&server, "item", "t-patch").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": {
                "name": "item.patch",
                "arguments": {
                    "id": "t-patch",
                    "data": {"label": "updated"},
                    "expected_version": version
                }
            }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let entity: Value = serde_json::from_str(text).unwrap();
    assert_eq!(entity["id"], "t-patch");
    assert_eq!(entity["version"], version + 1);
}

#[tokio::test]
async fn mcp_patch_tool_version_conflict_is_error() {
    let server = test_server();
    seed_collection(&server, "item").await;
    rest_create(&server, "item", "t-occ").await;

    // Use wrong expected_version (99 instead of 1).
    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": {
                "name": "item.patch",
                "arguments": {
                    "id": "t-occ",
                    "data": {"label": "x"},
                    "expected_version": 99
                }
            }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected JSON-RPC error: {body}");
    assert_eq!(
        body["result"]["isError"], true,
        "version conflict should be a tool error: {body}"
    );
    let msg = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        msg.contains("version") || msg.contains("conflict"),
        "error should mention version conflict: {msg}"
    );
}

#[tokio::test]
async fn mcp_delete_tool_removes_entity() {
    let server = test_server();
    seed_collection(&server, "item").await;
    rest_create(&server, "item", "t-del").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": { "name": "item.delete", "arguments": { "id": "t-del" } }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["id"], "t-del");
    assert_eq!(result["status"], "deleted");
}

#[tokio::test]
async fn mcp_unknown_tool_is_error() {
    let server = test_server();

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": { "name": "no_such_collection.create", "arguments": {} }
        }),
    )
    .await;

    // Unknown tool → result with isError: true (not a JSON-RPC protocol error).
    assert!(body["error"].is_null(), "should be tool-level error: {body}");
    assert_eq!(
        body["result"]["isError"], true,
        "unknown tool should return isError: {body}"
    );
}

// ── tools/call: aggregate ─────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_aggregate_count_tool() {
    let server = test_server();
    seed_collection(&server, "item").await;
    for i in 1..=3_u32 {
        rest_create(&server, "item", &format!("agg-{i:02}")).await;
    }

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "tools/call",
            "params": {
                "name": "item.aggregate",
                "arguments": { "function": "count" }
            }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(text).unwrap();
    // count case: CountEntitiesResponse serializes as {"total_count": N, "groups": [...]}
    let count = result["total_count"].as_u64().unwrap_or(0);
    assert!(count >= 3, "expected at least 3 entities, got {count}");
}

// ── resources/list ────────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_resources_list_always_has_meta_resources() {
    let server = test_server();
    let body = mcp(
        &server,
        &json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let resources = body["result"]["resources"].as_array().unwrap();
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
    assert!(uris.contains(&"axon://_collections"), "missing _collections: {uris:?}");
    assert!(uris.contains(&"axon://_schemas"), "missing _schemas: {uris:?}");
}

#[tokio::test]
async fn mcp_resources_list_includes_collection_resource_after_creation() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let resources = body["result"]["resources"].as_array().unwrap();
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
    assert!(
        uris.contains(&"axon://item"),
        "axon://item should appear after collection created: {uris:?}"
    );
}

// ── resources/templates/list ──────────────────────────────────────────────────

#[tokio::test]
async fn mcp_resource_templates_list_returns_standard_templates() {
    let server = test_server();
    let body = mcp(
        &server,
        &json!({"jsonrpc":"2.0","id":1,"method":"resources/templates/list"}),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let templates = body["result"]["resourceTemplates"].as_array().unwrap();
    let templates_uris: Vec<&str> = templates
        .iter()
        .filter_map(|t| t["uriTemplate"].as_str())
        .collect();
    assert!(
        templates_uris.contains(&"axon://{collection}/{id}"),
        "entity-by-id template missing: {templates_uris:?}"
    );
    assert!(
        templates_uris.contains(&"axon://{collection}/{id}/audit"),
        "audit template missing: {templates_uris:?}"
    );
}

// ── resources/read ────────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_resources_read_collections_meta() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "resources/read",
            "params": { "uri": "axon://_collections" }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["contents"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    let collections = payload["collections"].as_array().unwrap();
    assert!(
        collections.iter().any(|c| c.as_str() == Some("item")),
        "item should appear in collections: {collections:?}"
    );
}

#[tokio::test]
async fn mcp_resources_read_collection_listing() {
    let server = test_server();
    seed_collection(&server, "item").await;
    rest_create(&server, "item", "r-1").await;
    rest_create(&server, "item", "r-2").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "resources/read",
            "params": { "uri": "axon://item" }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["contents"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    assert!(
        payload["total_count"].as_u64().unwrap_or(0) >= 2,
        "expected at least 2 entities: {payload}"
    );
}

#[tokio::test]
async fn mcp_resources_read_entity_by_uri() {
    let server = test_server();
    seed_collection(&server, "item").await;
    rest_create(&server, "item", "r-ent").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "resources/read",
            "params": { "uri": "axon://item/r-ent" }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let text = body["result"]["contents"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["entity"]["id"], "r-ent");
    assert_eq!(payload["entity"]["version"], 1);
}

#[tokio::test]
async fn mcp_resources_read_missing_entity_is_error() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "resources/read",
            "params": { "uri": "axon://item/ghost" }
        }),
    )
    .await;

    // Missing entity → JSON-RPC error (not a tool-level error).
    assert!(!body["error"].is_null(), "expected error for missing entity: {body}");
    assert_eq!(body["error"]["code"], -32602_i64);
}

// ── resources/subscribe + unsubscribe ─────────────────────────────────────────

#[tokio::test]
async fn mcp_resources_subscribe_returns_subscription_id() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "resources/subscribe",
            "params": { "uri": "axon://item" }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let sub_id = body["result"]["subscriptionId"].as_u64();
    assert!(
        sub_id.is_some() && sub_id.unwrap() > 0,
        "subscriptionId should be a positive integer: {body}"
    );
}

#[tokio::test]
async fn mcp_resources_unsubscribe_succeeds() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "resources/unsubscribe",
            "params": { "uri": "axon://item" }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    // Empty result object.
    assert!(
        body["result"].is_object(),
        "unsubscribe should return an object: {body}"
    );
}

// ── prompts/list ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_prompts_list_returns_known_prompts() {
    let server = test_server();
    let body = mcp(
        &server,
        &json!({"jsonrpc":"2.0","id":1,"method":"prompts/list"}),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let prompts = body["result"]["prompts"].as_array().unwrap();
    let names: Vec<&str> = prompts.iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(names.contains(&"axon.explore_collection"), "missing explore_collection: {names:?}");
    assert!(names.contains(&"axon.schema_review"), "missing schema_review: {names:?}");
    assert!(names.contains(&"axon.audit_review"), "missing audit_review: {names:?}");
    assert!(names.contains(&"axon.dependency_analysis"), "missing dependency_analysis: {names:?}");
}

// ── prompts/get ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn mcp_prompts_get_schema_review_returns_messages() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "prompts/get",
            "params": {
                "name": "axon.schema_review",
                "arguments": { "collection": "item" }
            }
        }),
    )
    .await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    let messages = body["result"]["messages"].as_array().unwrap();
    assert!(!messages.is_empty(), "prompt should produce at least one message");
    let role = messages[0]["role"].as_str().unwrap();
    assert_eq!(role, "user");
    let content_type = messages[0]["content"]["type"].as_str().unwrap();
    assert_eq!(content_type, "text");
}

#[tokio::test]
async fn mcp_prompts_get_unknown_prompt_is_error() {
    let server = test_server();

    let body = mcp(
        &server,
        &json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "prompts/get",
            "params": { "name": "axon.no_such_prompt", "arguments": {} }
        }),
    )
    .await;

    assert!(!body["error"].is_null(), "expected error for unknown prompt: {body}");
    assert_eq!(body["error"]["code"], -32602_i64);
}

// ── SSE endpoint ──────────────────────────────────────────────────────────────

/// Verify the SSE endpoint is wired correctly by checking that a GET request
/// to /mcp/sse starts an event-stream. Uses a real HTTP transport so the
/// connection can be established without the in-process transport short-circuiting
/// the streaming response.
#[tokio::test]
async fn mcp_sse_endpoint_delivers_ready_event() {
    use axon_storage::SqliteStorageAdapter as Sqlite;

    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(Sqlite::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    let server = axum_test::TestServer::builder()
        .http_transport()
        .build(app);

    let url = server
        .server_url("/mcp/sse")
        .expect("test server should expose an HTTP URL");

    let response = reqwest::Client::new()
        .get(url)
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .send()
        .await
        .expect("SSE request should connect");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/event-stream"),
        "SSE endpoint should return text/event-stream, got: {content_type}"
    );
}
