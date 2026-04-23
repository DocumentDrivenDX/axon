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
use axon_api::test_fixtures::{seed_nexiq_reference_fixture, NexiqReferenceFixture};
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;

// ── Server helpers ─────────────────────────────────────────────────────────────

fn test_server() -> axum_test::TestServer {
    test_server_with_handler().0
}

type TestStorage = Box<dyn StorageAdapter + Send + Sync>;
type TestHandler = Arc<Mutex<AxonHandler<TestStorage>>>;

fn test_server_with_handler() -> (axum_test::TestServer, TestHandler) {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(Arc::clone(&handler)));
    (
        axum_test::TestServer::new(build_router(tenant_router, "memory", None)),
        handler,
    )
}

/// Create a collection with a simple `label` field via the REST API.
async fn seed_collection(server: &axum_test::TestServer, name: &str) {
    server
        .post(&format!(
            "/tenants/default/databases/default/collections/{name}"
        ))
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
        .post(&format!(
            "/tenants/default/databases/default/entities/{collection}/{id}"
        ))
        .json(&json!({"data": {"label": "test"}, "actor": "test"}))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let body = resp.json::<Value>();
    body["entity"]["version"].as_u64().unwrap_or(1)
}

/// POST a JSON-RPC message to /mcp and return the parsed response body.
/// Callers receive the full `{"jsonrpc":"2.0","id":...,"result":...}` object.
async fn mcp(server: &axum_test::TestServer, request: &Value) -> Value {
    server.post("/mcp").json(request).await.json::<Value>()
}

async fn mcp_as(server: &axum_test::TestServer, actor: &str, request: &Value) -> Value {
    server
        .post("/mcp")
        .add_header("x-axon-actor", actor)
        .json(request)
        .await
        .json::<Value>()
}

async fn mcp_as_with_session(
    server: &axum_test::TestServer,
    actor: &str,
    session_id: &str,
    request: &Value,
) -> Value {
    server
        .post("/mcp")
        .add_header("x-axon-actor", actor)
        .add_header("x-axon-mcp-session", session_id)
        .json(request)
        .await
        .json::<Value>()
}

async fn gql_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .add_header("x-axon-actor", actor)
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

async fn explain_policy_as(server: &axum_test::TestServer, actor: &str, input: &str) -> Value {
    let body = gql_as(
        server,
        actor,
        &format!(
            r#"{{
                explainPolicy(input: {input}) {{
                    operation
                    collection
                    entityId
                    decision
                    reason
                    policyVersion
                    fieldPaths
                    deniedFields
                    approval {{
                        name
                        decision
                        role
                        reasonRequired
                    }}
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        body["errors"].is_null(),
        "unexpected GraphQL explainPolicy error for {actor}: {body}"
    );
    body["data"]["explainPolicy"].clone()
}

async fn seed_nexiq_fixture(handler: &TestHandler) -> NexiqReferenceFixture {
    let mut guard = handler.lock().await;
    seed_nexiq_reference_fixture(&mut *guard).expect("nexiq reference fixture should seed")
}

async fn seed_policy_collection(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/policy_item")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "secret": { "type": "string" },
                        "amount_cents": { "type": "integer" }
                    }
                },
                "access_control": {
                    "read": {
                        "allow": [{ "name": "all-read" }]
                    },
                    "create": {
                        "allow": [{
                            "name": "admins-create",
                            "when": { "subject": "user_id", "eq": "admin" }
                        }]
                    },
                    "update": {
                        "allow": [{ "name": "all-update" }]
                    },
                    "delete": {
                        "allow": [{
                            "name": "admins-delete",
                            "when": { "subject": "user_id", "eq": "admin" }
                        }]
                    },
                    "fields": {
                        "secret": {
                            "read": {
                                "deny": [{
                                    "name": "contractors-cannot-read-secret",
                                    "when": { "subject": "user_id", "eq": "contractor" },
                                    "redact_as": null
                                }]
                            },
                            "write": {
                                "deny": [{
                                    "name": "contractors-cannot-write-secret",
                                    "when": { "subject": "user_id", "eq": "contractor" }
                                }]
                            }
                        }
                    },
                    "envelopes": {
                        "write": [{
                            "name": "large-amount-needs-approval",
                            "when": { "field": "amount_cents", "gt": 10000 },
                            "decision": "needs_approval",
                            "approval": {
                                "role": "finance_approver",
                                "reason_required": true
                            }
                        }]
                    }
                }
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);
}

async fn seed_query_policy_fixture(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/policy_user")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                },
                "link_types": {
                    "assigned-to": {
                        "target_collection": "policy_task",
                        "cardinality": "many-to-many"
                    }
                },
                "access_control": {
                    "read": { "allow": [{ "name": "users-visible" }] },
                    "create": { "allow": [{ "name": "admins-create-users" }] }
                }
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

    server
        .post("/tenants/default/databases/default/collections/policy_task")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "requester_id": { "type": "string" },
                        "assigned_contractor_id": { "type": "string" },
                        "secret": { "type": "string" }
                    }
                },
                "access_control": {
                    "read": {
                        "allow": [
                            {
                                "name": "admins-read-tasks",
                                "when": { "subject": "user_id", "eq": "admin" }
                            },
                            {
                                "name": "requesters-read-own-tasks",
                                "where": { "field": "requester_id", "eq_subject": "user_id" }
                            },
                            {
                                "name": "contractors-read-assigned-tasks",
                                "where": { "field": "assigned_contractor_id", "eq_subject": "user_id" }
                            }
                        ]
                    },
                    "create": {
                        "allow": [{
                            "name": "admins-create-tasks",
                            "when": { "subject": "user_id", "eq": "admin" }
                        }]
                    },
                    "fields": {
                        "secret": {
                            "read": {
                                "deny": [{
                                    "name": "contractors-cannot-read-secret",
                                    "when": { "subject": "user_id", "eq": "contractor" },
                                    "redact_as": null
                                }]
                            }
                        }
                    }
                }
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

    for (collection, id, data) in [
        ("policy_user", "u1", json!({ "name": "Ada" })),
        (
            "policy_task",
            "task-a",
            json!({
                "title": "Visible A",
                "requester_id": "requester",
                "assigned_contractor_id": "contractor",
                "secret": "alpha"
            }),
        ),
        (
            "policy_task",
            "task-b",
            json!({
                "title": "Hidden B",
                "requester_id": "other-requester",
                "assigned_contractor_id": "other-contractor",
                "secret": "beta"
            }),
        ),
        (
            "policy_task",
            "task-c",
            json!({
                "title": "Visible C",
                "requester_id": "requester",
                "assigned_contractor_id": "other-contractor",
                "secret": "gamma"
            }),
        ),
        (
            "policy_task",
            "task-contractor",
            json!({
                "title": "Contractor visible",
                "requester_id": "other-requester",
                "assigned_contractor_id": "contractor",
                "secret": "classified"
            }),
        ),
    ] {
        server
            .post(&format!(
                "/tenants/default/databases/default/entities/{collection}/{id}"
            ))
            .add_header("x-axon-actor", "admin")
            .json(&json!({ "data": data, "actor": "setup" }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    for target in ["task-a", "task-b"] {
        server
            .post("/tenants/default/databases/default/links")
            .add_header("x-axon-actor", "admin")
            .json(&json!({
                "source_collection": "policy_user",
                "source_id": "u1",
                "target_collection": "policy_task",
                "target_id": target,
                "link_type": "assigned-to"
            }))
            .await
            .assert_status(StatusCode::CREATED);
    }
}

async fn mcp_query_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    let response = mcp_as(
        server,
        actor,
        &json!({
            "jsonrpc": "2.0",
            "id": "query",
            "method": "tools/call",
            "params": {
                "name": "axon.query",
                "arguments": {
                    "query": query
                }
            }
        }),
    )
    .await;
    assert!(
        response["error"].is_null(),
        "unexpected MCP JSON-RPC error: {response}"
    );
    assert!(
        response["result"]["isError"].is_null(),
        "unexpected MCP tool error: {response}"
    );
    serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("axon.query should return text content"),
    )
    .expect("axon.query text should be a GraphQL JSON response")
}

// ── Protocol basics ───────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn mcp_ping_returns_empty_result() {
    let server = test_server();
    let body = mcp(&server, &json!({"jsonrpc":"2.0","id":1,"method":"ping"})).await;

    assert!(body["error"].is_null(), "unexpected error: {body}");
    assert_eq!(body["result"], json!({}));
}

#[tokio::test(flavor = "multi_thread")]
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
    assert!(!result["serverInfo"]["version"]
        .as_str()
        .unwrap_or("")
        .is_empty());
    assert_eq!(result["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(result["capabilities"]["resources"]["subscribe"], true);
    assert_eq!(result["capabilities"]["prompts"]["listChanged"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_notification_returns_no_content() {
    let server = test_server();
    // Notifications have no `id` field — the server must return 204 No Content.
    let resp = server
        .post("/mcp")
        .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await;
    resp.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
async fn mcp_malformed_json_returns_parse_error() {
    let server = test_server();
    let resp = server.post("/mcp").text("not json {{{").await;
    // Handler returns HTTP 200 with a JSON-RPC parse error payload.
    resp.assert_status_ok();
    let body = resp.json::<Value>();
    assert_eq!(body["error"]["code"], -32700);
}

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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
    assert!(
        names.contains(&"axon.query"),
        "axon.query always present: {names:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
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
        assert!(
            names.contains(expected),
            "expected {expected} in tools list: {names:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_tools_list_exposes_policy_metadata_matching_graphql_effective_policy() {
    let server = test_server();
    seed_policy_collection(&server).await;
    server
        .post("/tenants/default/databases/default/entities/policy_item/p1")
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "label": "policy target",
                "secret": "classified",
                "amount_cents": 100
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

    let effective = gql_as(
        &server,
        "contractor",
        r#"{
            effectivePolicy(collection: "policy_item") {
                canRead
                canCreate
                canUpdate
                canDelete
                redactedFields
                deniedFields
                policyVersion
            }
        }"#,
    )
    .await;
    assert!(
        effective["errors"].is_null(),
        "unexpected GraphQL effectivePolicy error: {effective}"
    );
    let effective = &effective["data"]["effectivePolicy"];

    let list = mcp_as(
        &server,
        "contractor",
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
    assert!(
        list["error"].is_null(),
        "unexpected MCP tools/list error: {list}"
    );
    let tools = list["result"]["tools"].as_array().unwrap();
    let patch_tool = tools
        .iter()
        .find(|tool| tool["name"] == "policy_item.patch")
        .expect("policy_item.patch should be listed");
    let policy = &patch_tool["policy"];
    let capabilities = &policy["capabilities"];

    assert_eq!(capabilities["canRead"], effective["canRead"]);
    assert_eq!(capabilities["canCreate"], effective["canCreate"]);
    assert_eq!(capabilities["canUpdate"], effective["canUpdate"]);
    assert_eq!(capabilities["canDelete"], effective["canDelete"]);
    assert_eq!(policy["redactedFields"], effective["redactedFields"]);
    assert_eq!(policy["deniedFields"], effective["deniedFields"]);
    assert_eq!(policy["policyVersion"], effective["policyVersion"]);
    assert_eq!(
        policy["envelopes"][0]["name"],
        "large-amount-needs-approval"
    );
    assert_eq!(policy["envelopes"][0]["operation"], "write");
    assert_eq!(policy["envelopes"][0]["decision"], "needs_approval");
    assert_eq!(
        policy["envelopes"][0]["approval"]["role"],
        "finance_approver"
    );
    assert_eq!(policy["envelopes"][0]["approval"]["reasonRequired"], true);
    assert_eq!(policy["toolOperation"], "patch");
    assert_eq!(
        policy["applicableEnvelopes"][0]["name"],
        "large-amount-needs-approval"
    );
    assert_eq!(
        policy["applicableEnvelopes"][0]["decision"],
        "needs_approval"
    );
    assert_eq!(
        policy["envelopeSummary"],
        "write:large-amount-needs-approval=needs_approval role=finance_approver reasonRequired=true"
    );
    assert_eq!(&patch_tool["inputSchema"]["x-axon-policy"], policy);
    let description = patch_tool["description"].as_str().unwrap();
    assert!(description.contains("Policy: canRead=true"));
    assert!(description.contains("toolOperation=patch"));
    assert!(description.contains("redactedFields=secret"));
    assert!(description.contains("deniedFields=secret"));
    assert!(description.contains(
        "envelopes=write:large-amount-needs-approval=needs_approval role=finance_approver reasonRequired=true"
    ));

    let autonomous = explain_policy_as(
        &server,
        "contractor",
        r#"{
            operation: "patch",
            collection: "policy_item",
            entityId: "p1",
            expectedVersion: 1,
            patch: { label: "safe autonomous update" }
        }"#,
    )
    .await;
    assert_eq!(autonomous["operation"], policy["toolOperation"]);
    assert_eq!(autonomous["decision"], "allow");
    assert_eq!(capabilities["canUpdate"], true);

    let approval = explain_policy_as(
        &server,
        "contractor",
        r#"{
            operation: "patch",
            collection: "policy_item",
            entityId: "p1",
            expectedVersion: 1,
            patch: { amount_cents: 20000 }
        }"#,
    )
    .await;
    assert_eq!(approval["operation"], policy["toolOperation"]);
    assert_eq!(approval["policyVersion"], policy["policyVersion"]);
    assert_eq!(approval["decision"], "needs_approval");
    assert_eq!(
        approval["approval"]["name"],
        policy["applicableEnvelopes"][0]["name"]
    );
    assert_eq!(
        approval["approval"]["decision"],
        policy["applicableEnvelopes"][0]["decision"]
    );
    assert_eq!(
        approval["approval"]["role"],
        policy["applicableEnvelopes"][0]["approval"]["role"]
    );
    assert_eq!(
        approval["approval"]["reasonRequired"],
        policy["applicableEnvelopes"][0]["approval"]["reasonRequired"]
    );
    assert!(description.contains(
        approval["approval"]["name"]
            .as_str()
            .expect("approval envelope should have a name")
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_tool_descriptions_summarize_autonomous_and_approval_envelopes() {
    let server = test_server();
    server
        .post("/tenants/default/databases/default/collections/policy_invoice")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "amount_cents": { "type": "integer" },
                        "memo": { "type": "string" }
                    }
                },
                "access_control": {
                    "read": { "allow": [{ "name": "all-read" }] },
                    "create": {
                        "allow": [{
                            "name": "finance-agent-create",
                            "when": { "subject": "user_id", "eq": "finance-agent" }
                        }]
                    },
                    "update": {
                        "allow": [{
                            "name": "finance-agent-update",
                            "when": { "subject": "user_id", "eq": "finance-agent" }
                        }]
                    },
                    "envelopes": {
                        "write": [
                            {
                                "name": "small-amount-autonomous",
                                "when": { "field": "amount_cents", "lte": 10000 },
                                "decision": "allow"
                            },
                            {
                                "name": "large-amount-needs-approval",
                                "when": { "field": "amount_cents", "gt": 10000 },
                                "decision": "needs_approval",
                                "approval": {
                                    "role": "finance_approver",
                                    "reason_required": true
                                }
                            }
                        ]
                    }
                }
            },
            "actor": "schema-admin"
        }))
        .await
        .assert_status(StatusCode::CREATED);
    server
        .post("/tenants/default/databases/default/entities/policy_invoice/inv-1")
        .add_header("x-axon-actor", "finance-agent")
        .json(&json!({
            "data": {
                "amount_cents": 5000,
                "memo": "initial"
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

    let effective = gql_as(
        &server,
        "finance-agent",
        r#"{
            effectivePolicy(collection: "policy_invoice") {
                canUpdate
                policyVersion
            }
        }"#,
    )
    .await;
    assert!(
        effective["errors"].is_null(),
        "unexpected GraphQL effectivePolicy error: {effective}"
    );
    let effective = &effective["data"]["effectivePolicy"];

    let list = mcp_as(
        &server,
        "finance-agent",
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    )
    .await;
    assert!(
        list["error"].is_null(),
        "unexpected MCP tools/list error: {list}"
    );
    let patch_tool = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "policy_invoice.patch")
        .expect("policy_invoice.patch should be listed");
    let policy = &patch_tool["policy"];
    assert_eq!(policy["toolOperation"], "patch");
    assert_eq!(policy["policyVersion"], effective["policyVersion"]);
    assert_eq!(policy["capabilities"]["canUpdate"], effective["canUpdate"]);
    assert_eq!(&patch_tool["inputSchema"]["x-axon-policy"], policy);

    let applicable = policy["applicableEnvelopes"].as_array().unwrap();
    let autonomous = applicable
        .iter()
        .find(|envelope| envelope["name"] == "small-amount-autonomous")
        .expect("autonomous envelope should be applicable to patch");
    assert_eq!(autonomous["operation"], "write");
    assert_eq!(autonomous["decision"], "allow");
    let approval_envelope = applicable
        .iter()
        .find(|envelope| envelope["name"] == "large-amount-needs-approval")
        .expect("approval envelope should be applicable to patch");
    assert_eq!(approval_envelope["operation"], "write");
    assert_eq!(approval_envelope["decision"], "needs_approval");
    assert_eq!(approval_envelope["approval"]["role"], "finance_approver");
    assert_eq!(approval_envelope["approval"]["reasonRequired"], true);

    let description = patch_tool["description"].as_str().unwrap();
    assert!(
        description.contains("toolOperation=patch"),
        "description should contain tool operation: {description}"
    );
    for expected in [
        "write:small-amount-autonomous=autonomous",
        "write:large-amount-needs-approval=needs_approval role=finance_approver reasonRequired=true",
    ] {
        assert!(
            description.contains(expected),
            "description should contain {expected:?}: {description}"
        );
        assert!(
            policy["envelopeSummary"].as_str().unwrap().contains(expected),
            "input policy metadata should contain {expected:?}: {policy}"
        );
    }

    let small_patch = explain_policy_as(
        &server,
        "finance-agent",
        r#"{
            operation: "patch",
            collection: "policy_invoice",
            entityId: "inv-1",
            expectedVersion: 1,
            patch: { amount_cents: 9000 }
        }"#,
    )
    .await;
    assert_eq!(small_patch["operation"], policy["toolOperation"]);
    assert_eq!(small_patch["policyVersion"], policy["policyVersion"]);
    assert_eq!(small_patch["decision"], "allow");

    let large_patch = explain_policy_as(
        &server,
        "finance-agent",
        r#"{
            operation: "patch",
            collection: "policy_invoice",
            entityId: "inv-1",
            expectedVersion: 1,
            patch: { amount_cents: 20000 }
        }"#,
    )
    .await;
    assert_eq!(large_patch["operation"], policy["toolOperation"]);
    assert_eq!(large_patch["policyVersion"], policy["policyVersion"]);
    assert_eq!(large_patch["decision"], "needs_approval");
    assert_eq!(large_patch["approval"]["name"], approval_envelope["name"]);
    assert_eq!(
        large_patch["approval"]["decision"],
        approval_envelope["decision"]
    );
    assert_eq!(
        large_patch["approval"]["role"],
        approval_envelope["approval"]["role"]
    );
    assert_eq!(
        large_patch["approval"]["reasonRequired"],
        approval_envelope["approval"]["reasonRequired"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_tools_list_refreshes_policy_metadata_after_schema_update() {
    let server = test_server();
    seed_policy_collection(&server).await;

    let first_response = server
        .post("/mcp")
        .add_header("x-axon-actor", "contractor")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .await;
    first_response.assert_status_ok();
    let session_id = first_response
        .headers()
        .get("x-axon-mcp-session")
        .and_then(|value| value.to_str().ok())
        .expect("tools/list should issue a session id")
        .to_string();
    let first = first_response.json::<Value>();
    let first_tools = first["result"]["tools"].as_array().unwrap();
    let first_patch = first_tools
        .iter()
        .find(|tool| tool["name"] == "policy_item.patch")
        .expect("policy_item.patch should be listed before schema update");
    assert_eq!(first_patch["policy"]["policyVersion"], 1);
    assert_eq!(first_patch["policy"]["capabilities"]["canCreate"], false);

    server
        .put("/tenants/default/databases/default/collections/policy_item/schema")
        .json(&json!({
            "version": 2,
            "entity_schema": {
                "type": "object",
                "properties": {
                    "label": { "type": "string" },
                    "secret": { "type": "string" },
                    "amount_cents": { "type": "integer" }
                }
            },
            "access_control": {
                "read": {
                    "allow": [{ "name": "all-read" }]
                },
                "create": {
                    "allow": [{ "name": "all-create" }]
                },
                "update": {
                    "allow": [{ "name": "all-update" }]
                },
                "delete": {
                    "allow": [{
                        "name": "admins-delete",
                        "when": { "subject": "user_id", "eq": "admin" }
                    }]
                },
                "fields": {
                    "secret": {
                        "read": {
                            "deny": [{
                                "name": "contractors-cannot-read-secret",
                                "when": { "subject": "user_id", "eq": "contractor" },
                                "redact_as": null
                            }]
                        },
                        "write": {
                            "deny": [{
                                "name": "contractors-cannot-write-secret",
                                "when": { "subject": "user_id", "eq": "contractor" }
                            }]
                        }
                    }
                },
                "envelopes": {
                    "write": [{
                        "name": "large-amount-needs-approval-v2",
                        "when": { "field": "amount_cents", "gt": 5000 },
                        "decision": "needs_approval",
                        "approval": {
                            "role": "finance_approver",
                            "reason_required": true
                        }
                    }]
                }
            },
            "actor": "schema-admin"
        }))
        .await
        .assert_status_ok();

    let effective = gql_as(
        &server,
        "contractor",
        r#"{
            effectivePolicy(collection: "policy_item") {
                canCreate
                policyVersion
            }
        }"#,
    )
    .await;
    assert!(
        effective["errors"].is_null(),
        "unexpected GraphQL effectivePolicy error: {effective}"
    );
    let effective = &effective["data"]["effectivePolicy"];

    let refreshed = mcp_as_with_session(
        &server,
        "contractor",
        &session_id,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await;
    assert!(
        refreshed["error"].is_null(),
        "unexpected MCP tools/list error after schema update: {refreshed}"
    );
    let refreshed_tools = refreshed["result"]["tools"].as_array().unwrap();
    let refreshed_patch = refreshed_tools
        .iter()
        .find(|tool| tool["name"] == "policy_item.patch")
        .expect("policy_item.patch should still be listed after schema update");
    let policy = &refreshed_patch["policy"];

    assert_eq!(policy["policyVersion"], effective["policyVersion"]);
    assert_eq!(policy["capabilities"]["canCreate"], effective["canCreate"]);
    assert_eq!(policy["policyVersion"], 2);
    assert_eq!(policy["capabilities"]["canCreate"], true);
    assert_eq!(
        policy["envelopes"][0]["name"],
        "large-amount-needs-approval-v2"
    );
    assert_eq!(&refreshed_patch["inputSchema"]["x-axon-policy"], policy);
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_mcp_policy_parity_matrix_matches_expected_decisions() {
    let server = test_server();
    seed_policy_collection(&server).await;
    server
        .post("/tenants/default/databases/default/entities/policy_item/p1")
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "label": "policy target",
                "secret": "classified",
                "amount_cents": 100
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

    let effective = gql_as(
        &server,
        "contractor",
        r#"{
            effectivePolicy(collection: "policy_item") {
                policyVersion
                redactedFields
                deniedFields
            }
        }"#,
    )
    .await;
    assert!(
        effective["errors"].is_null(),
        "unexpected GraphQL effectivePolicy error: {effective}"
    );
    let effective = &effective["data"]["effectivePolicy"];

    let tools = mcp_as(
        &server,
        "contractor",
        &json!({"jsonrpc":"2.0","id":"tools","method":"tools/list"}),
    )
    .await;
    assert!(
        tools["error"].is_null(),
        "unexpected MCP tools/list error: {tools}"
    );
    let patch_tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "policy_item.patch")
        .expect("policy_item.patch should be listed");
    let tool_policy = &patch_tool["policy"];
    assert_eq!(tool_policy["policyVersion"], effective["policyVersion"]);
    assert_eq!(tool_policy["redactedFields"], effective["redactedFields"]);
    assert_eq!(tool_policy["deniedFields"], effective["deniedFields"]);
    assert_eq!(tool_policy["redactedFields"], json!(["secret"]));
    assert_eq!(tool_policy["deniedFields"], json!(["secret"]));

    let envelope = &tool_policy["envelopes"][0];
    assert_eq!(envelope["name"], "large-amount-needs-approval");
    assert_eq!(envelope["decision"], "needs_approval");
    assert_eq!(envelope["approval"]["role"], "finance_approver");
    assert_eq!(envelope["approval"]["reasonRequired"], true);

    let matrix = [
        (
            "contractor-denied-secret-patch",
            "contractor",
            r#"{
                operation: "patch",
                collection: "policy_item",
                entityId: "p1",
                expectedVersion: 1,
                patch: { secret: "leaked" }
            }"#,
            json!({
                "jsonrpc": "2.0",
                "id": "contractor-denied-secret-patch",
                "method": "tools/call",
                "params": {
                    "name": "policy_item.patch",
                    "arguments": {
                        "id": "p1",
                        "data": { "secret": "leaked" },
                        "expected_version": 1
                    }
                }
            }),
            "deny",
            "denied",
            "field_write_denied",
            json!(["secret"]),
            Value::Null,
        ),
        (
            "admin-approval-large-amount-patch",
            "admin",
            r#"{
                operation: "patch",
                collection: "policy_item",
                entityId: "p1",
                expectedVersion: 1,
                patch: { amount_cents: 20000 }
            }"#,
            json!({
                "jsonrpc": "2.0",
                "id": "admin-approval-large-amount-patch",
                "method": "tools/call",
                "params": {
                    "name": "policy_item.patch",
                    "arguments": {
                        "id": "p1",
                        "data": { "amount_cents": 20000 },
                        "expected_version": 1
                    }
                }
            }),
            "needs_approval",
            "needs_approval",
            "needs_approval",
            json!([]),
            json!({
                "name": "large-amount-needs-approval",
                "decision": "needs_approval",
                "role": "finance_approver",
                "reasonRequired": true
            }),
        ),
    ];

    for (
        case_name,
        actor,
        explain_input,
        mcp_request,
        graph_decision,
        mcp_outcome,
        reason,
        field_paths,
        expected_approval,
    ) in matrix
    {
        let graph = explain_policy_as(&server, actor, explain_input).await;
        assert_eq!(
            graph["policyVersion"], effective["policyVersion"],
            "{case_name}"
        );
        assert_eq!(graph["decision"], graph_decision, "{case_name}");
        assert_eq!(graph["reason"], reason, "{case_name}");
        assert_eq!(graph["fieldPaths"], field_paths, "{case_name}");

        let mcp = mcp_as(&server, actor, &mcp_request).await;
        assert!(
            mcp["error"].is_null(),
            "unexpected MCP protocol error for {case_name}: {mcp}"
        );
        assert_eq!(mcp["result"]["isError"], true, "{case_name}");
        let structured = &mcp["result"]["structuredContent"];
        assert_eq!(
            structured["policyVersion"], graph["policyVersion"],
            "{case_name}"
        );
        assert_eq!(structured["outcome"], mcp_outcome, "{case_name}");
        assert_eq!(structured["reason"], graph["reason"], "{case_name}");
        let empty_field_paths = json!([]);
        let structured_field_paths = structured.get("fieldPaths").unwrap_or(&empty_field_paths);
        assert_eq!(structured_field_paths, &graph["fieldPaths"], "{case_name}");
        assert_eq!(structured["collection"], graph["collection"], "{case_name}");
        assert_eq!(structured["entityId"], graph["entityId"], "{case_name}");

        if expected_approval.is_null() {
            assert!(
                structured["approval"].is_null(),
                "unexpected MCP approval for {case_name}: {structured}"
            );
            let graph_approval_absent = graph["approval"].is_null()
                || graph["approval"]
                    .as_object()
                    .is_some_and(|approval| approval.values().all(Value::is_null));
            assert!(
                graph_approval_absent,
                "unexpected GraphQL approval for {case_name}: {graph}"
            );
        } else {
            assert_eq!(graph["approval"], expected_approval, "{case_name}");
            assert_eq!(
                structured["approval"]["name"], graph["approval"]["name"],
                "{case_name}"
            );
            assert_eq!(
                structured["approval"]["decision"], graph["approval"]["decision"],
                "{case_name}"
            );
            assert_eq!(
                structured["approval"]["role"], graph["approval"]["role"],
                "{case_name}"
            );
            assert_eq!(
                structured["approval"]["reason_required"], graph["approval"]["reasonRequired"],
                "{case_name}"
            );
            assert_eq!(envelope["name"], graph["approval"]["name"], "{case_name}");
            assert_eq!(
                envelope["decision"], graph["approval"]["decision"],
                "{case_name}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_crud_policy_denials_return_structured_outcomes() {
    let server = test_server();
    seed_policy_collection(&server).await;
    server
        .post("/tenants/default/databases/default/entities/policy_item/p1")
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "label": "policy target",
                "secret": "classified",
                "amount_cents": 100
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

    let denied = mcp_as(
        &server,
        "contractor",
        &json!({
            "jsonrpc": "2.0",
            "id": "denied",
            "method": "tools/call",
            "params": {
                "name": "policy_item.patch",
                "arguments": {
                    "id": "p1",
                    "data": { "secret": "leaked" },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        denied["error"].is_null(),
        "unexpected MCP protocol error: {denied}"
    );
    assert_eq!(denied["result"]["isError"], true);
    let structured = &denied["result"]["structuredContent"];
    assert_eq!(structured["outcome"], "denied");
    assert_eq!(structured["errorCode"], "denied_policy");
    assert_eq!(structured["operation"], "patch");
    assert_eq!(structured["reason"], "field_write_denied");
    assert_eq!(structured["collection"], "policy_item");
    assert_eq!(structured["entityId"], "p1");
    assert_eq!(structured["fieldPath"], "secret");
    assert_eq!(structured["ruleId"], "contractors-cannot-write-secret");
    assert_eq!(structured["deniedFields"], json!(["secret"]));
    assert_eq!(structured["fieldPaths"], json!(["secret"]));
    let rule_names: Vec<_> = structured["rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|rule| rule["name"].as_str())
        .collect();
    assert!(
        rule_names.contains(&"contractors-cannot-write-secret"),
        "structured denial should name the matching rule: {denied}"
    );

    let approval = mcp_as(
        &server,
        "admin",
        &json!({
            "jsonrpc": "2.0",
            "id": "approval",
            "method": "tools/call",
            "params": {
                "name": "policy_item.patch",
                "arguments": {
                    "id": "p1",
                    "data": { "amount_cents": 20000 },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        approval["error"].is_null(),
        "unexpected MCP protocol error: {approval}"
    );
    assert_eq!(approval["result"]["isError"], true);
    let structured = &approval["result"]["structuredContent"];
    assert_eq!(structured["outcome"], "needs_approval");
    assert_eq!(structured["errorCode"], "approval_required");
    assert_eq!(structured["reason"], "needs_approval");
    assert_eq!(structured["collection"], "policy_item");
    assert_eq!(structured["entityId"], "p1");
    assert_eq!(
        structured["approval"]["name"],
        "large-amount-needs-approval"
    );
    assert_eq!(structured["approval"]["role"], "finance_approver");
    assert_eq!(structured["approval"]["reason_required"], true);
    assert!(
        structured.get("intentToken").is_none(),
        "approval-routed direct CRUD must not mint FEAT-030 tokens: {approval}"
    );

    let allowed = mcp_as(
        &server,
        "admin",
        &json!({
            "jsonrpc": "2.0",
            "id": "allowed",
            "method": "tools/call",
            "params": {
                "name": "policy_item.patch",
                "arguments": {
                    "id": "p1",
                    "data": { "label": "updated" },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        allowed["error"].is_null(),
        "unexpected MCP protocol error: {allowed}"
    );
    assert!(allowed["result"]["isError"].is_null() || allowed["result"]["isError"] == false);
    assert!(
        allowed["result"]["structuredContent"].is_null(),
        "allowed CRUD should keep the normal payload shape: {allowed}"
    );
    let text = allowed["result"]["content"][0]["text"].as_str().unwrap();
    let entity: Value = serde_json::from_str(text).unwrap();
    assert_eq!(entity["data"]["label"], "updated");
    assert_eq!(entity["version"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_axon_query_matches_graphql_policy_semantics() {
    let server = test_server();
    seed_query_policy_fixture(&server).await;

    let first_page_query = r#"
        fragment EntityBits on Entity {
            id
            data
        }

        {
            hidden: entity(collection: "policy_task", id: "task-b") {
                ...EntityBits
            }
            page: entities(collection: "policy_task", limit: 1) {
                totalCount
                edges {
                    cursor
                    node { ...EntityBits }
                }
                pageInfo {
                    hasNextPage
                    hasPreviousPage
                    startCursor
                    endCursor
                }
            }
            related: neighbors(
                collection: "policy_user",
                id: "u1",
                direction: "outbound",
                linkType: "assigned-to",
                limit: 10
            ) {
                totalCount
                groups {
                    linkType
                    direction
                    totalCount
                    edges {
                        linkType
                        direction
                        node { ...EntityBits }
                    }
                }
            }
        }
    "#;
    let gql = gql_as(&server, "requester", first_page_query).await;
    let mcp = mcp_query_as(&server, "requester", first_page_query).await;
    assert!(gql["errors"].is_null(), "unexpected GraphQL errors: {gql}");
    assert!(
        mcp["errors"].is_null(),
        "unexpected MCP GraphQL errors: {mcp}"
    );
    assert_eq!(mcp["data"], gql["data"]);
    assert_eq!(mcp["data"]["hidden"], Value::Null);
    assert_eq!(mcp["data"]["page"]["totalCount"], 2);
    assert_eq!(mcp["data"]["page"]["edges"][0]["node"]["id"], "task-a");
    assert_eq!(mcp["data"]["page"]["pageInfo"]["hasNextPage"], true);
    assert_eq!(mcp["data"]["related"]["totalCount"], 1);
    assert_eq!(
        mcp["data"]["related"]["groups"][0]["edges"][0]["node"]["id"],
        "task-a"
    );

    let after = gql["data"]["page"]["pageInfo"]["endCursor"]
        .as_str()
        .expect("first page should include endCursor");
    let second_page_query = format!(
        r#"{{
            page: entities(collection: "policy_task", limit: 1, after: "{after}") {{
                totalCount
                edges {{ node {{ id data }} }}
                pageInfo {{ hasNextPage hasPreviousPage startCursor endCursor }}
            }}
        }}"#
    );
    let gql = gql_as(&server, "requester", &second_page_query).await;
    let mcp = mcp_query_as(&server, "requester", &second_page_query).await;
    assert!(gql["errors"].is_null(), "unexpected GraphQL errors: {gql}");
    assert!(
        mcp["errors"].is_null(),
        "unexpected MCP GraphQL errors: {mcp}"
    );
    assert_eq!(mcp["data"], gql["data"]);
    assert_eq!(mcp["data"]["page"]["totalCount"], 2);
    assert_eq!(mcp["data"]["page"]["edges"][0]["node"]["id"], "task-c");
    assert_eq!(mcp["data"]["page"]["pageInfo"]["hasPreviousPage"], true);

    let redaction_query = r#"{
        redacted: entity(collection: "policy_task", id: "task-contractor") {
            id
            data
        }
    }"#;
    let gql = gql_as(&server, "contractor", redaction_query).await;
    let mcp = mcp_query_as(&server, "contractor", redaction_query).await;
    assert!(gql["errors"].is_null(), "unexpected GraphQL errors: {gql}");
    assert!(
        mcp["errors"].is_null(),
        "unexpected MCP GraphQL errors: {mcp}"
    );
    assert_eq!(mcp["data"], gql["data"]);
    assert_eq!(mcp["data"]["redacted"]["data"]["secret"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_nexiq_reference_policy_queries_match_graphql() {
    let (server, handler) = test_server_with_handler();
    let fixture = seed_nexiq_fixture(&handler).await;

    let consultant_query = format!(
        r#"{{
            visibleEngagement: entity(collection: "{engagements}", id: "{engagement_alpha}") {{ id data }}
            hiddenEngagement: entity(collection: "{engagements}", id: "{engagement_beta}") {{ id data }}
            engagements: entities(collection: "{engagements}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
            visibleContract: entity(collection: "{contracts}", id: "{contract_alpha}") {{ id data }}
            hiddenContract: entity(collection: "{contracts}", id: "{contract_beta}") {{ id data }}
            contracts: entities(collection: "{contracts}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
            visibleTask: entity(collection: "{tasks}", id: "{task_alpha}") {{ id data }}
            hiddenTask: entity(collection: "{tasks}", id: "{task_beta}") {{ id data }}
            tasks: entities(collection: "{tasks}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
            invoice: entity(collection: "{invoices}", id: "{invoice_alpha}") {{ id data }}
            invoices: entities(collection: "{invoices}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
        }}"#,
        engagements = fixture.collections.engagements.as_str(),
        engagement_alpha = fixture.ids.engagement_alpha.as_str(),
        engagement_beta = fixture.ids.engagement_beta.as_str(),
        contracts = fixture.collections.contracts.as_str(),
        contract_alpha = fixture.ids.contract_alpha.as_str(),
        contract_beta = fixture.ids.contract_beta.as_str(),
        tasks = fixture.collections.tasks.as_str(),
        task_alpha = fixture.ids.task_alpha.as_str(),
        task_beta = fixture.ids.task_beta.as_str(),
        invoices = fixture.collections.invoices.as_str(),
        invoice_alpha = fixture.ids.invoice_alpha.as_str(),
    );
    let consultant_gql = gql_as(&server, fixture.subjects.consultant, &consultant_query).await;
    let consultant_mcp =
        mcp_query_as(&server, fixture.subjects.consultant, &consultant_query).await;
    assert!(
        consultant_gql["errors"].is_null(),
        "unexpected GraphQL errors: {consultant_gql}"
    );
    assert!(
        consultant_mcp["errors"].is_null(),
        "unexpected MCP GraphQL errors: {consultant_mcp}"
    );
    assert_eq!(consultant_mcp["data"], consultant_gql["data"]);
    assert_eq!(consultant_mcp["data"]["hiddenEngagement"], Value::Null);
    assert_eq!(consultant_mcp["data"]["engagements"]["totalCount"], 1);
    assert_eq!(
        consultant_mcp["data"]["contracts"]["edges"][0]["node"]["id"],
        fixture.ids.contract_alpha.as_str()
    );
    assert_eq!(consultant_mcp["data"]["hiddenTask"], Value::Null);
    assert_eq!(consultant_mcp["data"]["invoices"]["totalCount"], 0);

    let contractor_query = format!(
        r#"{{
            engagement: entity(collection: "{engagements}", id: "{engagement_alpha}") {{ id data }}
            invoice: entity(collection: "{invoices}", id: "{invoice_alpha}") {{ id data }}
            invoices: entities(collection: "{invoices}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
        }}"#,
        engagements = fixture.collections.engagements.as_str(),
        engagement_alpha = fixture.ids.engagement_alpha.as_str(),
        invoices = fixture.collections.invoices.as_str(),
        invoice_alpha = fixture.ids.invoice_alpha.as_str(),
    );
    let contractor_gql = gql_as(&server, fixture.subjects.contractor, &contractor_query).await;
    let contractor_mcp =
        mcp_query_as(&server, fixture.subjects.contractor, &contractor_query).await;
    assert!(
        contractor_gql["errors"].is_null(),
        "unexpected GraphQL errors: {contractor_gql}"
    );
    assert!(
        contractor_mcp["errors"].is_null(),
        "unexpected MCP GraphQL errors: {contractor_mcp}"
    );
    assert_eq!(contractor_mcp["data"], contractor_gql["data"]);
    assert_eq!(
        contractor_mcp["data"]["engagement"]["data"]["budget_cents"],
        Value::Null
    );
    assert_eq!(
        contractor_mcp["data"]["engagement"]["data"]["rate_card_id"],
        Value::Null
    );
    assert_eq!(contractor_mcp["data"]["invoice"], Value::Null);

    let ops_manager_query = format!(
        r#"{{
            contract: entity(collection: "{contracts}", id: "{contract_alpha}") {{ id data }}
            invoice: entity(collection: "{invoices}", id: "{invoice_alpha}") {{ id data }}
            timeVisible: entity(collection: "{time_entries}", id: "{time_entry_alpha}") {{ id data }}
            timeHidden: entity(collection: "{time_entries}", id: "{time_entry_beta}") {{ id data }}
            timeEntries: entities(collection: "{time_entries}", limit: 10) {{
                totalCount
                edges {{ node {{ id data }} }}
            }}
        }}"#,
        contracts = fixture.collections.contracts.as_str(),
        contract_alpha = fixture.ids.contract_alpha.as_str(),
        invoices = fixture.collections.invoices.as_str(),
        invoice_alpha = fixture.ids.invoice_alpha.as_str(),
        time_entries = fixture.collections.time_entries.as_str(),
        time_entry_alpha = fixture.ids.time_entry_alpha.as_str(),
        time_entry_beta = fixture.ids.time_entry_beta.as_str(),
    );
    let ops_manager_gql = gql_as(&server, fixture.subjects.ops_manager, &ops_manager_query).await;
    let ops_manager_mcp =
        mcp_query_as(&server, fixture.subjects.ops_manager, &ops_manager_query).await;
    assert!(
        ops_manager_gql["errors"].is_null(),
        "unexpected GraphQL errors: {ops_manager_gql}"
    );
    assert!(
        ops_manager_mcp["errors"].is_null(),
        "unexpected MCP GraphQL errors: {ops_manager_mcp}"
    );
    assert_eq!(ops_manager_mcp["data"], ops_manager_gql["data"]);
    assert_eq!(
        ops_manager_mcp["data"]["contract"]["data"]["rate_card_entries"],
        Value::Null
    );
    assert_eq!(
        ops_manager_mcp["data"]["invoice"]["id"],
        fixture.ids.invoice_alpha.as_str()
    );
    assert_eq!(ops_manager_mcp["data"]["timeHidden"], Value::Null);
    assert_eq!(ops_manager_mcp["data"]["timeEntries"]["totalCount"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_nexiq_reference_policy_writes_return_structured_denials() {
    let (server, handler) = test_server_with_handler();
    let fixture = seed_nexiq_fixture(&handler).await;

    let engagement_denial = mcp_as(
        &server,
        fixture.subjects.consultant,
        &json!({
            "jsonrpc": "2.0",
            "id": "engagement-status-denied",
            "method": "tools/call",
            "params": {
                "name": "engagements.patch",
                "arguments": {
                    "id": fixture.ids.engagement_alpha.as_str(),
                    "data": { "status": "closed" },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        engagement_denial["error"].is_null(),
        "unexpected MCP protocol error: {engagement_denial}"
    );
    assert_eq!(engagement_denial["result"]["isError"], true);
    let structured = &engagement_denial["result"]["structuredContent"];
    assert_eq!(structured["outcome"], "denied");
    assert_eq!(structured["errorCode"], "denied_policy");
    assert_eq!(structured["reason"], "field_write_denied");
    assert_eq!(
        structured["collection"],
        fixture.collections.engagements.as_str()
    );
    assert_eq!(
        structured["entityId"],
        fixture.ids.engagement_alpha.as_str()
    );
    assert_eq!(structured["fieldPath"], "status");
    assert_eq!(structured["fieldPaths"], json!(["status"]));
    assert_eq!(structured["deniedFields"], json!(["status"]));

    let time_entry_denial = mcp_as(
        &server,
        fixture.subjects.ops_manager,
        &json!({
            "jsonrpc": "2.0",
            "id": "time-approval-denied",
            "method": "tools/call",
            "params": {
                "name": "time_entries.patch",
                "arguments": {
                    "id": fixture.ids.time_entry_alpha.as_str(),
                    "data": { "status": "approved" },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        time_entry_denial["error"].is_null(),
        "unexpected MCP protocol error: {time_entry_denial}"
    );
    assert_eq!(time_entry_denial["result"]["isError"], true);
    let structured = &time_entry_denial["result"]["structuredContent"];
    assert_eq!(structured["outcome"], "denied");
    assert_eq!(structured["errorCode"], "denied_policy");
    assert_eq!(structured["reason"], "row_write_denied");
    assert_eq!(
        structured["collection"],
        fixture.collections.time_entries.as_str()
    );
    assert_eq!(
        structured["entityId"],
        fixture.ids.time_entry_alpha.as_str()
    );
    assert!(
        structured["fieldPath"].is_null(),
        "row denial should not name a field path: {time_entry_denial}"
    );
}

// ── tools/call: CRUD ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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
    assert!(
        body["error"].is_null(),
        "should be tool-level error: {body}"
    );
    assert_eq!(
        body["result"]["isError"], true,
        "unknown tool should return isError: {body}"
    );
}

// ── tools/call: aggregate ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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
    assert!(
        uris.contains(&"axon://_collections"),
        "missing _collections: {uris:?}"
    );
    assert!(
        uris.contains(&"axon://_schemas"),
        "missing _schemas: {uris:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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
    assert!(
        !body["error"].is_null(),
        "expected error for missing entity: {body}"
    );
    assert_eq!(body["error"]["code"], -32602_i64);
}

// ── resources/subscribe + unsubscribe ─────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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

#[tokio::test(flavor = "multi_thread")]
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
    assert!(
        names.contains(&"axon.explore_collection"),
        "missing explore_collection: {names:?}"
    );
    assert!(
        names.contains(&"axon.schema_review"),
        "missing schema_review: {names:?}"
    );
    assert!(
        names.contains(&"axon.audit_review"),
        "missing audit_review: {names:?}"
    );
    assert!(
        names.contains(&"axon.dependency_analysis"),
        "missing dependency_analysis: {names:?}"
    );
}

// ── prompts/get ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
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
    assert!(
        !messages.is_empty(),
        "prompt should produce at least one message"
    );
    let role = messages[0]["role"].as_str().unwrap();
    assert_eq!(role, "user");
    let content_type = messages[0]["content"]["type"].as_str().unwrap();
    assert_eq!(content_type, "text");
}

#[tokio::test(flavor = "multi_thread")]
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

    assert!(
        !body["error"].is_null(),
        "expected error for unknown prompt: {body}"
    );
    assert_eq!(body["error"]["code"], -32602_i64);
}

// ── SSE endpoint ──────────────────────────────────────────────────────────────

/// Verify the SSE endpoint is wired correctly by checking that a GET request
/// to /mcp/sse starts an event-stream. Uses a real HTTP transport so the
/// connection can be established without the in-process transport short-circuiting
/// the streaming response.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_sse_endpoint_delivers_ready_event() {
    use axon_storage::SqliteStorageAdapter as Sqlite;

    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(Sqlite::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    let server = axum_test::TestServer::builder().http_transport().build(app);

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
