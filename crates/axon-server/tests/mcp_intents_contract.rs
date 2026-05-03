//! MCP mutation intent contract tests for generated tools.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_api::intent::MutationIntentCommitValidationError;
use axon_mcp::McpMutationIntentOutcome;
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

async fn gql_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .add_header("x-axon-actor", actor)
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
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

fn assert_no_graphql_errors(body: &Value, context: &str) {
    assert!(
        body["errors"].is_null(),
        "unexpected {context} GraphQL errors: {body}"
    );
}

async fn seed_intent_collection(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/intent_users")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "required": ["user_id", "approval_role"],
                    "properties": {
                        "user_id": { "type": "string" },
                        "approval_role": { "type": "string" }
                    }
                },
                "indexes": [
                    { "field": "user_id", "type": "string", "unique": true }
                ]
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

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
                    "identity": {
                        "user_id": "subject.user_id",
                        "role": "subject.attributes.approval_role",
                        "attributes": {
                            "approval_role": {
                                "from": "collection",
                                "collection": "intent_users",
                                "key_field": "user_id",
                                "key_subject": "user_id",
                                "value_field": "approval_role"
                            }
                        }
                    },
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

    for (id, approval_role) in [
        ("finance-agent", "finance_agent"),
        ("finance-approver", "finance_approver"),
    ] {
        server
            .post(&format!(
                "/tenants/default/databases/default/entities/intent_users/{id}"
            ))
            .add_header("x-axon-actor", "admin")
            .json(&json!({
                "data": {
                    "user_id": id,
                    "approval_role": approval_role
                },
                "actor": "setup"
            }))
            .await
            .assert_status(StatusCode::CREATED);
    }

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

async fn update_task_amount(server: &axum_test::TestServer, expected_version: u64, amount: u64) {
    server
        .put("/tenants/default/databases/default/entities/intent_task/task-a")
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "title": "Tracked task",
                "amount_cents": amount,
                "secret": "alpha"
            },
            "expected_version": expected_version,
            "actor": "setup"
        }))
        .await
        .assert_status_ok();
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

async fn audit_by_intent(
    server: &axum_test::TestServer,
    intent_id: &str,
    operation: Option<&str>,
) -> Value {
    let operation_query = operation
        .map(|operation| format!("&operation={operation}"))
        .unwrap_or_default();
    server
        .get(&format!(
            "/tenants/default/databases/default/audit/query?intent_id={intent_id}{operation_query}"
        ))
        .await
        .json::<Value>()
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
    let allowed_intent_id = allowed["intent_id"].as_str().unwrap();
    let preview_audit = audit_by_intent(&server, allowed_intent_id, Some("intent.preview")).await;
    let preview_entries = preview_audit["entries"].as_array().unwrap();
    assert_eq!(preview_entries.len(), 1);
    assert_eq!(preview_entries[0]["actor"], "finance-agent");
    assert_eq!(
        preview_entries[0]["intent_lineage"]["intent_id"],
        allowed_intent_id
    );
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

#[tokio::test(flavor = "multi_thread")]
async fn generated_mcp_tools_report_stale_commit_conflict() {
    let server = test_server();
    seed_intent_collection(&server).await;

    let preview = patch_tool(
        &server,
        "stale-preview",
        json!({
            "intent_mode": "preview",
            "id": "task-a",
            "data": { "amount_cents": 9000 },
            "expected_version": 1,
            "expires_in_seconds": 600
        }),
    )
    .await;
    assert!(
        preview["error"].is_null(),
        "unexpected stale preview protocol error: {preview}"
    );
    let allowed = &preview["result"]["structuredContent"];
    assert_eq!(allowed["outcome"], "allowed");
    let token = allowed["intent_token"].as_str().unwrap().to_string();
    let intent_id = allowed["intent_id"].as_str().unwrap().to_string();

    update_task_amount(&server, 1, 7000).await;

    let stale = patch_tool(
        &server,
        "stale-commit",
        json!({
            "intent_mode": "commit",
            "intent_token": token
        }),
    )
    .await;
    assert!(
        stale["error"].is_null(),
        "unexpected stale commit protocol error: {stale}"
    );
    assert_eq!(stale["result"]["isError"], true);
    let conflict = &stale["result"]["structuredContent"];
    assert_eq!(conflict["outcome"], "conflict");
    assert_eq!(conflict["error_code"], "intent_stale");
    assert_eq!(conflict["intent_id"], intent_id);
    assert_eq!(conflict["details"][0]["dimension"], "pre_image");
    assert_eq!(conflict["details"][0]["expected"], "1");
    assert_eq!(conflict["details"][0]["actual"], "2");
    assert_eq!(
        conflict["details"][0]["detail"],
        "entity:intent_task/task-a"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn axon_query_intent_mutations_match_graphql_review_flow() {
    let server = test_server();
    seed_intent_collection(&server).await;

    let preview = mcp_query_as(
        &server,
        "finance-agent",
        r#"mutation {
            previewMutation(input: {
                operation: {
                    operationKind: "patch_entity"
                    operation: {
                        collection: "intent_task"
                        id: "task-a"
                        expected_version: 1
                        patch: { amount_cents: 20000 }
                    }
                }
                expiresInSeconds: 600
            }) {
                decision
                intentToken
                intent {
                    id
                    approvalState
                    decision
                    approvalRoute { role }
                    reviewSummary
                }
            }
        }"#,
    )
    .await;
    assert_no_graphql_errors(&preview, "MCP preview");
    let preview_result = &preview["data"]["previewMutation"];
    assert_eq!(preview_result["decision"], "needs_approval");
    let intent_id = preview_result["intent"]["id"].as_str().unwrap().to_string();
    let intent_token = preview_result["intentToken"].as_str().unwrap().to_string();
    assert_eq!(preview_result["intent"]["approvalState"], "pending");
    assert_eq!(
        preview_result["intent"]["approvalRoute"]["role"],
        "finance_approver"
    );
    assert!(
        !preview_result["intent"]["reviewSummary"]["policy_explanation"]
            .as_array()
            .unwrap()
            .is_empty(),
        "preview should expose a policy explanation: {preview_result}"
    );

    let lookup = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"{{
                mutationIntent(id: "{intent_id}") {{
                    id
                    approvalState
                    decision
                    approvalRoute {{ role }}
                    reviewSummary
                }}
            }}"#
        ),
    )
    .await;
    assert_no_graphql_errors(&lookup, "GraphQL lookup after MCP preview");
    assert_eq!(lookup["data"]["mutationIntent"]["id"], intent_id);
    assert_eq!(
        lookup["data"]["mutationIntent"]["decision"],
        preview_result["intent"]["decision"]
    );
    assert_eq!(
        lookup["data"]["mutationIntent"]["reviewSummary"]["policy_explanation"],
        preview_result["intent"]["reviewSummary"]["policy_explanation"]
    );

    let approved = mcp_query_as(
        &server,
        "finance-approver",
        &format!(
            r#"mutation {{
                approveMutationIntent(input: {{
                    intentId: "{intent_id}"
                    reason: "approved through MCP"
                }}) {{
                    id
                    approvalState
                    decision
                    reviewSummary
                }}
            }}"#
        ),
    )
    .await;
    assert_no_graphql_errors(&approved, "MCP approval");
    assert_eq!(approved["data"]["approveMutationIntent"]["id"], intent_id);
    assert_eq!(
        approved["data"]["approveMutationIntent"]["approvalState"],
        "approved"
    );

    let committed = mcp_query_as(
        &server,
        "finance-agent",
        &format!(
            r#"mutation {{
                commitMutationIntent(input: {{
                    intentToken: "{intent_token}"
                    intentId: "{intent_id}"
                }}) {{
                    committed
                    errorCode
                    stale {{ dimension expected actual path }}
                    intent {{ id approvalState decision }}
                }}
            }}"#
        ),
    )
    .await;
    assert_no_graphql_errors(&committed, "MCP commit");
    assert_eq!(committed["data"]["commitMutationIntent"]["committed"], true);
    assert_eq!(
        committed["data"]["commitMutationIntent"]["errorCode"],
        Value::Null
    );
    assert_eq!(
        committed["data"]["commitMutationIntent"]["intent"]["id"],
        intent_id
    );
    assert_eq!(
        committed["data"]["commitMutationIntent"]["intent"]["approvalState"],
        "committed"
    );
    assert_eq!(get_task(&server).await["data"]["amount_cents"], 20000);

    let reject_preview = mcp_query_as(
        &server,
        "finance-agent",
        r#"mutation {
            previewMutation(input: {
                operation: {
                    operationKind: "patch_entity"
                    operation: {
                        collection: "intent_task"
                        id: "task-a"
                        expected_version: 2
                        patch: { amount_cents: 30000 }
                    }
                }
                expiresInSeconds: 600
            }) {
                decision
                intent { id approvalState decision }
            }
        }"#,
    )
    .await;
    assert_no_graphql_errors(&reject_preview, "MCP reject preview");
    let reject_intent_id = reject_preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let rejected = mcp_query_as(
        &server,
        "finance-approver",
        &format!(
            r#"mutation {{
                rejectMutationIntent(input: {{
                    intentId: "{reject_intent_id}"
                    reason: "not approved"
                }}) {{
                    id
                    approvalState
                    decision
                }}
            }}"#
        ),
    )
    .await;
    assert_no_graphql_errors(&rejected, "MCP rejection");
    assert_eq!(
        rejected["data"]["rejectMutationIntent"]["id"],
        reject_intent_id
    );
    assert_eq!(
        rejected["data"]["rejectMutationIntent"]["approvalState"],
        "rejected"
    );

    let rejected_lookup = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"{{
                mutationIntent(id: "{reject_intent_id}") {{ id approvalState decision }}
            }}"#
        ),
    )
    .await;
    assert_no_graphql_errors(&rejected_lookup, "GraphQL lookup after MCP rejection");
    assert_eq!(
        rejected_lookup["data"]["mutationIntent"]["approvalState"],
        "rejected"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn axon_query_intent_commit_conflict_matches_graphql_error_extensions() {
    let graph_server = test_server();
    seed_intent_collection(&graph_server).await;
    let mcp_server = test_server();
    seed_intent_collection(&mcp_server).await;

    let graph_preview = gql_as(
        &graph_server,
        "finance-agent",
        r#"mutation {
            previewMutation(input: {
                operation: {
                    operationKind: "patch_entity"
                    operation: {
                        collection: "intent_task"
                        id: "task-a"
                        expected_version: 1
                        patch: { amount_cents: 9000 }
                    }
                }
                expiresInSeconds: 600
            }) {
                decision
                intentToken
                intent { id reviewSummary }
            }
        }"#,
    )
    .await;
    assert_no_graphql_errors(&graph_preview, "GraphQL stale preview");
    let graph_token = graph_preview["data"]["previewMutation"]["intentToken"]
        .as_str()
        .unwrap()
        .to_string();
    let graph_intent_id = graph_preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let mcp_preview = mcp_query_as(
        &mcp_server,
        "finance-agent",
        r#"mutation {
            previewMutation(input: {
                operation: {
                    operationKind: "patch_entity"
                    operation: {
                        collection: "intent_task"
                        id: "task-a"
                        expected_version: 1
                        patch: { amount_cents: 9000 }
                    }
                }
                expiresInSeconds: 600
            }) {
                decision
                intentToken
                intent { id reviewSummary }
            }
        }"#,
    )
    .await;
    assert_no_graphql_errors(&mcp_preview, "MCP stale preview");
    let mcp_token = mcp_preview["data"]["previewMutation"]["intentToken"]
        .as_str()
        .unwrap()
        .to_string();
    let mcp_intent_id = mcp_preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        mcp_preview["data"]["previewMutation"]["decision"],
        graph_preview["data"]["previewMutation"]["decision"]
    );
    assert_eq!(
        mcp_preview["data"]["previewMutation"]["intent"]["reviewSummary"]["policy_explanation"],
        graph_preview["data"]["previewMutation"]["intent"]["reviewSummary"]["policy_explanation"]
    );

    update_task_amount(&graph_server, 1, 7000).await;
    update_task_amount(&mcp_server, 1, 7000).await;

    let graph_commit = gql_as(
        &graph_server,
        "finance-agent",
        &format!(
            r#"mutation {{
                commitMutationIntent(input: {{
                    intentToken: "{graph_token}"
                    intentId: "{graph_intent_id}"
                }}) {{
                    committed
                    stale {{ dimension expected actual path }}
                    errorCode
                }}
            }}"#
        ),
    )
    .await;
    let mcp_commit = mcp_query_as(
        &mcp_server,
        "finance-agent",
        &format!(
            r#"mutation {{
                commitMutationIntent(input: {{
                    intentToken: "{mcp_token}"
                    intentId: "{mcp_intent_id}"
                }}) {{
                    committed
                    stale {{ dimension expected actual path }}
                    errorCode
                }}
            }}"#
        ),
    )
    .await;

    assert_eq!(
        mcp_commit["errors"][0]["extensions"]["code"],
        graph_commit["errors"][0]["extensions"]["code"]
    );
    assert_eq!(
        mcp_commit["errors"][0]["extensions"]["code"],
        "intent_stale"
    );
    assert_eq!(
        mcp_commit["errors"][0]["extensions"]["stale"],
        graph_commit["errors"][0]["extensions"]["stale"]
    );
    assert_eq!(
        mcp_commit["errors"][0]["extensions"]["stale"][0]["dimension"],
        "pre_image"
    );
}

#[test]
fn mcp_intent_authorization_denial_uses_graphql_stable_code() {
    let outcome = McpMutationIntentOutcome::from_commit_validation_error(
        MutationIntentCommitValidationError::AuthorizationFailed {
            intent_id: "mint_authz".into(),
            reason: "current grants do not authorize intent commit".into(),
        },
    );
    let value = serde_json::to_value(outcome).expect("MCP outcome should serialize");

    assert_eq!(value["outcome"], "denied");
    assert_eq!(value["error_code"], "intent_authorization_failed");
    assert_eq!(value["intent_id"], "mint_authz");
}
