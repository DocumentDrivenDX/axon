//! GraphQL FEAT-030 mutation intent commit contract tests.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use axum::http::StatusCode;
use serde_json::{json, Value};
use tokio::sync::Mutex;

type TestStorage = Box<dyn StorageAdapter + Send + Sync>;

fn test_server() -> axum_test::TestServer {
    let storage: TestStorage =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(Arc::clone(&handler)));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

async fn gql_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .add_header("x-axon-actor", actor)
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

async fn seed_intent_fixture(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/task")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "budget_cents": { "type": "integer" },
                        "secret": { "type": "string" },
                        "status": { "type": "string" }
                    }
                },
                "access_control": {
                    "read": {
                        "allow": [{ "name": "fixture-read" }]
                    },
                    "create": {
                        "allow": [{ "name": "fixture-create" }]
                    },
                    "update": {
                        "allow": [{ "name": "fixture-update" }]
                    },
                    "fields": {
                        "secret": {
                            "write": {
                                "deny": [{
                                    "name": "finance-cannot-write-secret",
                                    "when": { "subject": "user_id", "eq": "finance-agent" }
                                }]
                            }
                        }
                    },
                    "envelopes": {
                        "write": [{
                            "name": "large-budget-needs-finance-approval",
                            "when": {
                                "all": [
                                    { "operation": "update" },
                                    { "field": "budget_cents", "gt": 10000 }
                                ]
                            },
                            "decision": "needs_approval",
                            "approval": {
                                "role": "finance_approver",
                                "reason_required": true,
                                "deadline_seconds": 86400,
                                "separation_of_duties": true
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
        .post("/tenants/default/databases/default/entities/task/task-a")
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "title": "Budget request",
                "budget_cents": 5000,
                "secret": "alpha",
                "status": "draft"
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);
}

async fn preview_budget_patch(
    server: &axum_test::TestServer,
    budget_cents: u64,
    expires_in_seconds: u64,
) -> Value {
    gql_as(
        server,
        "finance-agent",
        &format!(
            r#"mutation {{
                previewMutation(input: {{
                    operation: {{
                        operationKind: "patch_entity"
                        operation: {{
                            collection: "task"
                            id: "task-a"
                            expected_version: 1
                            patch: {{ budget_cents: {budget_cents} }}
                        }}
                    }}
                    expiresInSeconds: {expires_in_seconds}
                }}) {{
                    decision
                    intentToken
                    intent {{ id approvalState decision }}
                }}
            }}"#
        ),
    )
    .await
}

async fn commit_token(server: &axum_test::TestServer, actor: &str, token: &str) -> Value {
    gql_as(
        server,
        actor,
        &format!(
            r#"mutation {{
                commitMutationIntent(input: {{ intentToken: "{token}" }}) {{
                    committed
                    transactionId
                    errorCode
                    stale {{ dimension expected actual path }}
                    intent {{ id approvalState decision }}
                }}
            }}"#
        ),
    )
    .await
}

async fn budget_cents(server: &axum_test::TestServer) -> Value {
    let body = gql_as(
        server,
        "finance-agent",
        r#"{ entity(collection: "task", id: "task-a") { data } }"#,
    )
    .await;
    body["data"]["entity"]["data"]["budget_cents"].clone()
}

async fn audit_by_intent(
    server: &axum_test::TestServer,
    intent_id: &str,
    operation: &str,
) -> Value {
    server
        .get(&format!(
            "/tenants/default/databases/default/audit/query?intent_id={intent_id}&operation={operation}"
        ))
        .await
        .json::<Value>()
}

fn assert_no_errors(body: &Value, context: &str) {
    assert!(
        body["errors"].is_null(),
        "unexpected {context} errors: {body}"
    );
}

fn assert_error_code(body: &Value, expected: &str) {
    assert_eq!(body["errors"][0]["extensions"]["code"], expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn under_threshold_allow_commit_and_replay_rejects() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 6000, 600).await;
    assert_no_errors(&preview, "preview");
    let result = &preview["data"]["previewMutation"];
    assert_eq!(result["decision"], "allow");
    let token = result["intentToken"].as_str().unwrap().to_string();

    let committed = commit_token(&server, "finance-agent", &token).await;
    assert_no_errors(&committed, "commit");
    let commit = &committed["data"]["commitMutationIntent"];
    assert_eq!(commit["committed"], true);
    assert_eq!(commit["intent"]["approvalState"], "committed");
    assert!(commit["transactionId"].as_str().is_some());
    assert_eq!(budget_cents(&server).await, json!(6000));

    let replay = commit_token(&server, "finance-agent", &token).await;
    assert_error_code(&replay, "intent_already_committed");
}

#[tokio::test(flavor = "multi_thread")]
async fn over_threshold_intent_can_be_approved_and_committed() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 20_000, 600).await;
    assert_no_errors(&preview, "preview");
    let result = &preview["data"]["previewMutation"];
    assert_eq!(result["decision"], "needs_approval");
    assert_eq!(result["intent"]["approvalState"], "pending");
    let token = result["intentToken"].as_str().unwrap().to_string();
    let intent_id = result["intent"]["id"].as_str().unwrap().to_string();

    let approved = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"mutation {{
                approveMutationIntent(input: {{
                    intentId: "{intent_id}"
                    reason: "approved"
                }}) {{ id approvalState decision }}
            }}"#
        ),
    )
    .await;
    assert_no_errors(&approved, "approval");
    assert_eq!(
        approved["data"]["approveMutationIntent"]["approvalState"],
        "approved"
    );
    let approval_audit = audit_by_intent(&server, &intent_id, "intent.approve").await;
    let approval_entries = approval_audit["entries"].as_array().unwrap();
    assert_eq!(approval_entries.len(), 1);
    assert_eq!(approval_entries[0]["actor"], "finance-approver");
    assert_eq!(approval_entries[0]["metadata"]["reason"], "approved");
    assert_eq!(
        approval_entries[0]["intent_lineage"]["intent_id"],
        intent_id
    );
    assert_eq!(approval_entries[0]["intent_lineage"]["policy_version"], 1);

    let committed = commit_token(&server, "finance-agent", &token).await;
    assert_no_errors(&committed, "commit");
    assert_eq!(
        committed["data"]["commitMutationIntent"]["intent"]["approvalState"],
        "committed"
    );
    assert_eq!(budget_cents(&server).await, json!(20_000));
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_intent_cannot_commit() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 21_000, 600).await;
    assert_no_errors(&preview, "preview");
    let result = &preview["data"]["previewMutation"];
    let token = result["intentToken"].as_str().unwrap().to_string();
    let intent_id = result["intent"]["id"].as_str().unwrap().to_string();

    let rejected = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"mutation {{
                rejectMutationIntent(input: {{
                    intentId: "{intent_id}"
                    reason: "rejected"
                }}) {{ id approvalState decision }}
            }}"#
        ),
    )
    .await;
    assert_no_errors(&rejected, "rejection");
    assert_eq!(
        rejected["data"]["rejectMutationIntent"]["approvalState"],
        "rejected"
    );
    let rejection_audit = audit_by_intent(&server, &intent_id, "intent.reject").await;
    let rejection_entries = rejection_audit["entries"].as_array().unwrap();
    assert_eq!(rejection_entries.len(), 1);
    assert_eq!(rejection_entries[0]["actor"], "finance-approver");
    assert_eq!(rejection_entries[0]["metadata"]["reason"], "rejected");
    assert_eq!(
        rejection_entries[0]["intent_lineage"]["intent_id"],
        intent_id
    );

    let committed = commit_token(&server, "finance-agent", &token).await;
    assert_error_code(&committed, "intent_rejected");
    assert_eq!(budget_cents(&server).await, json!(5000));
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_intent_cannot_commit() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 6000, 0).await;
    assert_no_errors(&preview, "preview");
    let token = preview["data"]["previewMutation"]["intentToken"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::sleep(Duration::from_millis(1)).await;

    let committed = commit_token(&server, "finance-agent", &token).await;
    assert_error_code(&committed, "intent_expired");
    assert_eq!(budget_cents(&server).await, json!(5000));
}

#[tokio::test(flavor = "multi_thread")]
async fn denied_preview_has_no_executable_token() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let denied = gql_as(
        &server,
        "finance-agent",
        r#"mutation {
            previewMutation(input: {
                operation: {
                    operationKind: "patch_entity"
                    operation: {
                        collection: "task"
                        id: "task-a"
                        expected_version: 1
                        patch: { secret: "changed" }
                    }
                }
                expiresInSeconds: 600
            }) {
                decision
                intentToken
                intent { approvalState decision }
            }
        }"#,
    )
    .await;

    assert_no_errors(&denied, "denied preview");
    let result = &denied["data"]["previewMutation"];
    assert_eq!(result["decision"], "deny");
    assert!(result["intentToken"].is_null());
    assert_eq!(budget_cents(&server).await, json!(5000));
}
