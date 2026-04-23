//! MCP mutation intent contract tests for generated tools.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use axum::http::StatusCode;
use serde_json::{json, Value};
use tokio::sync::Mutex;

fn test_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    axum_test::TestServer::new(build_router(tenant_router, "memory", None))
}

async fn mcp_as(server: &axum_test::TestServer, actor: &str, request: &Value) -> Value {
    server
        .post("/mcp")
        .add_header("x-axon-actor", actor)
        .json(request)
        .await
        .json::<Value>()
}

async fn seed_intent_collection(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/intent_task")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "amount_cents": { "type": "integer" },
                        "secret": { "type": "string" }
                    }
                },
                "access_control": {
                    "read": { "allow": [{ "name": "all-read" }] },
                    "create": { "allow": [{ "name": "all-create" }] },
                    "update": { "allow": [{ "name": "all-update" }] },
                    "fields": {
                        "secret": {
                            "write": {
                                "deny": [{
                                    "name": "finance-agent-cannot-write-secret",
                                    "when": { "subject": "user_id", "eq": "finance-agent" }
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

    server
        .post("/tenants/default/databases/default/entities/intent_task/task-a")
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "title": "Tracked task",
                "amount_cents": 5000,
                "secret": "alpha"
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);
}

async fn get_task(server: &axum_test::TestServer) -> Value {
    server
        .get("/tenants/default/databases/default/entities/intent_task/task-a")
        .await
        .json::<Value>()["entity"]
        .clone()
}

async fn patch_tool(server: &axum_test::TestServer, id: &str, arguments: Value) -> Value {
    mcp_as(
        server,
        "finance-agent",
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "intent_task.patch",
                "arguments": arguments
            }
        }),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_mcp_tools_preview_commit_and_block_approval_bypass() {
    let server = test_server();
    seed_intent_collection(&server).await;

    let tools = mcp_as(
        &server,
        "finance-agent",
        &json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "method": "tools/list"
        }),
    )
    .await;
    assert!(
        tools["error"].is_null(),
        "unexpected tools/list error: {tools}"
    );
    let patch_tool_info = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "intent_task.patch")
        .expect("generated patch tool should exist");
    let properties = &patch_tool_info["inputSchema"]["properties"];
    assert_eq!(
        properties["intent_mode"]["enum"],
        json!(["direct", "preview", "commit"])
    );
    assert!(properties["intent_token"].is_object());
    assert!(properties["preview"].is_object());

    let allowed_preview = patch_tool(
        &server,
        "allowed-preview",
        json!({
            "intent_mode": "preview",
            "id": "task-a",
            "data": { "amount_cents": 6000 },
            "expected_version": 1,
            "expires_in_seconds": 600
        }),
    )
    .await;
    assert!(
        allowed_preview["error"].is_null(),
        "unexpected allowed preview protocol error: {allowed_preview}"
    );
    let allowed = &allowed_preview["result"]["structuredContent"];
    assert_eq!(allowed["outcome"], "allowed");
    let token = allowed["intent_token"].as_str().unwrap().to_string();
    let before_commit = get_task(&server).await;
    assert_eq!(before_commit["data"]["amount_cents"], 5000);
    assert_eq!(before_commit["version"], 1);

    let commit = patch_tool(
        &server,
        "commit",
        json!({
            "intent_mode": "commit",
            "intent_token": token
        }),
    )
    .await;
    assert!(
        commit["error"].is_null(),
        "unexpected commit protocol error: {commit}"
    );
    let committed = &commit["result"]["structuredContent"];
    assert_eq!(committed["outcome"], "committed");
    assert_eq!(committed["intent_id"], allowed["intent_id"]);
    assert!(committed["transaction_id"].as_str().is_some());
    let after_commit = get_task(&server).await;
    assert_eq!(after_commit["data"]["amount_cents"], 6000);
    assert_eq!(after_commit["version"], 2);

    let needs_approval = patch_tool(
        &server,
        "needs-approval",
        json!({
            "intent_mode": "preview",
            "id": "task-a",
            "data": { "amount_cents": 20000 },
            "expected_version": 2
        }),
    )
    .await;
    assert!(
        needs_approval["error"].is_null(),
        "unexpected approval preview protocol error: {needs_approval}"
    );
    let approval = &needs_approval["result"]["structuredContent"];
    assert_eq!(approval["outcome"], "needs_approval");
    assert!(approval["intent_token"].as_str().is_some());
    assert_eq!(approval["approval_route"]["role"], "finance_approver");

    let bypass = patch_tool(
        &server,
        "bypass",
        json!({
            "id": "task-a",
            "data": { "amount_cents": 20000 },
            "expected_version": 2
        }),
    )
    .await;
    assert!(
        bypass["error"].is_null(),
        "unexpected bypass protocol error: {bypass}"
    );
    assert_eq!(bypass["result"]["isError"], true);
    assert_eq!(
        bypass["result"]["structuredContent"]["outcome"],
        "needs_approval"
    );
    assert_eq!(
        bypass["result"]["structuredContent"]["errorCode"],
        "approval_required"
    );
    let after_bypass = get_task(&server).await;
    assert_eq!(after_bypass["data"]["amount_cents"], 6000);
    assert_eq!(after_bypass["version"], 2);

    let denied_preview = patch_tool(
        &server,
        "denied-preview",
        json!({
            "preview": true,
            "id": "task-a",
            "data": { "secret": "leaked" },
            "expected_version": 2
        }),
    )
    .await;
    assert!(
        denied_preview["error"].is_null(),
        "unexpected denied preview protocol error: {denied_preview}"
    );
    let denied = &denied_preview["result"]["structuredContent"];
    assert_eq!(denied["outcome"], "denied");
    assert_eq!(denied["error_code"], "denied_policy");
    assert!(denied.get("intent_token").is_none());
    let after_denied = get_task(&server).await;
    assert_eq!(after_denied["data"]["secret"], "alpha");
    assert_eq!(after_denied["version"], 2);
}
