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
        .post("/tenants/default/databases/default/collections/users")
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
                    "identity": {
                        "user_id": "subject.user_id",
                        "role": "subject.attributes.approval_role",
                        "attributes": {
                            "approval_role": {
                                "from": "collection",
                                "collection": "users",
                                "key_field": "user_id",
                                "key_subject": "user_id",
                                "value_field": "approval_role"
                            }
                        }
                    },
                    "read": {
                        "allow": [{ "name": "fixture-read" }]
                    },
                    "create": {
                        "allow": [{ "name": "fixture-create" }]
                    },
                    "update": {
                        "allow": [{
                            "name": "fixture-update",
                            "when": {
                                "subject": "user_id",
                                "in": ["finance-agent", "finance-approver"]
                            }
                        }]
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

    for (id, approval_role) in [
        ("finance-agent", "finance_agent"),
        ("finance-approver", "finance_approver"),
        ("contractor", "contractor"),
    ] {
        server
            .post(&format!(
                "/tenants/default/databases/default/entities/users/{id}"
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

async fn approve_intent(server: &axum_test::TestServer, actor: &str, intent_id: &str) -> Value {
    gql_as(
        server,
        actor,
        &format!(
            r#"mutation {{
                approveMutationIntent(input: {{
                    intentId: "{intent_id}"
                    reason: "approved"
                }}) {{ id approvalState decision }}
            }}"#
        ),
    )
    .await
}

async fn update_approval_role(server: &axum_test::TestServer, user_id: &str, approval_role: &str) {
    server
        .put(&format!(
            "/tenants/default/databases/default/entities/users/{user_id}"
        ))
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "user_id": user_id,
                "approval_role": approval_role
            },
            "expected_version": 1,
            "actor": "admin"
        }))
        .await
        .assert_status_ok();
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

async fn audit_entity(server: &axum_test::TestServer) -> Value {
    server
        .get("/tenants/default/databases/default/audit/entity/task/task-a")
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
    let approval_audit = audit_by_intent(&server, &intent_id, Some("intent.approve")).await;
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

    let lineage_audit = audit_by_intent(&server, &intent_id, None).await;
    let lineage_entries = lineage_audit["entries"].as_array().unwrap();
    assert_eq!(lineage_entries.len(), 2);
    assert!(
        lineage_entries
            .iter()
            .any(|entry| entry["mutation"] == "intent.approve"
                && entry["actor"] == "finance-approver")
    );
    let committed_entry = lineage_entries
        .iter()
        .find(|entry| entry["mutation"] == "entity.update")
        .expect("intent lineage query should include committed mutation audit");
    assert_eq!(committed_entry["actor"], "finance-agent");
    assert_eq!(
        committed_entry["intent_lineage"]["subject_snapshot"]["user_id"],
        "finance-agent"
    );
    assert_eq!(committed_entry["intent_lineage"]["policy_version"], 1);
    assert_eq!(committed_entry["diff"]["budget_cents"]["before"], 5000);
    assert_eq!(committed_entry["diff"]["budget_cents"]["after"], 20_000);

    let entity_audit = audit_entity(&server).await;
    let entity_entries = entity_audit["entries"].as_array().unwrap();
    let entity_update = entity_entries
        .iter()
        .find(|entry| {
            entry["mutation"] == "entity.update"
                && entry["intent_lineage"]["intent_id"] == intent_id
        })
        .expect("entity audit should carry committed intent lineage");
    assert_eq!(entity_update["diff"]["budget_cents"]["before"], 5000);
    assert_eq!(entity_update["diff"]["budget_cents"]["after"], 20_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn pending_intent_queries_return_pending_reviews() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 20_250, 600).await;
    assert_no_errors(&preview, "preview");
    let intent_id = preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let queried = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"{{
                mutationIntent(id: "{intent_id}") {{
                    id
                    approvalState
                    decision
                    approvalRoute {{ role separationOfDuties }}
                }}
                pendingMutationIntents(
                    filter: {{ status: "pending", decision: "needs_approval" }}
                    limit: 10
                ) {{
                    totalCount
                    edges {{
                        cursor
                        node {{
                            id
                            approvalState
                            decision
                            approvalRoute {{ role separationOfDuties }}
                        }}
                    }}
                    pageInfo {{ hasNextPage hasPreviousPage startCursor endCursor }}
                }}
            }}"#
        ),
    )
    .await;
    assert_no_errors(&queried, "pending intent queries");

    let found = &queried["data"]["mutationIntent"];
    assert_eq!(found["id"], intent_id);
    assert_eq!(found["approvalState"], "pending");
    assert_eq!(found["decision"], "needs_approval");
    assert_eq!(found["approvalRoute"]["role"], "finance_approver");
    assert_eq!(found["approvalRoute"]["separationOfDuties"], true);

    let pending = &queried["data"]["pendingMutationIntents"];
    assert_eq!(pending["totalCount"], 1);
    assert_eq!(pending["edges"].as_array().unwrap().len(), 1);
    let edge = &pending["edges"][0];
    assert_eq!(edge["node"]["id"], intent_id);
    assert_eq!(edge["node"]["approvalState"], "pending");
    assert_eq!(edge["node"]["decision"], "needs_approval");
    assert!(edge["cursor"].as_str().is_some());
    assert_eq!(pending["pageInfo"]["hasNextPage"], false);
    assert_eq!(pending["pageInfo"]["hasPreviousPage"], false);
    assert!(pending["pageInfo"]["startCursor"].as_str().is_some());
    assert!(pending["pageInfo"]["endCursor"].as_str().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_requires_current_approver_role() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 20_500, 600).await;
    assert_no_errors(&preview, "preview");
    let intent_id = preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let denied = approve_intent(&server, "finance-agent", &intent_id).await;
    assert_error_code(&denied, "intent_authorization_failed");
    assert_eq!(budget_cents(&server).await, json!(5000));

    let approved = approve_intent(&server, "finance-approver", &intent_id).await;
    assert_no_errors(&approved, "approval after denied actor");
    assert_eq!(
        approved["data"]["approveMutationIntent"]["approvalState"],
        "approved"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_rechecks_role_after_preview() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 21_500, 600).await;
    assert_no_errors(&preview, "preview");
    let intent_id = preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    update_approval_role(&server, "finance-approver", "contractor").await;
    let denied = approve_intent(&server, "finance-approver", &intent_id).await;
    assert_error_code(&denied, "intent_authorization_failed");
    assert_eq!(budget_cents(&server).await, json!(5000));
}

#[tokio::test(flavor = "multi_thread")]
async fn separation_of_duties_blocks_self_approval() {
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = gql_as(
        &server,
        "finance-approver",
        r#"mutation {
            previewMutation(input: {
                operation: {
                    operationKind: "patch_entity"
                    operation: {
                        collection: "task"
                        id: "task-a"
                        expected_version: 1
                        patch: { budget_cents: 22500 }
                    }
                }
                expiresInSeconds: 600
            }) {
                decision
                intent { id approvalState decision subject approvalRoute { role separationOfDuties } }
            }
        }"#,
    )
    .await;
    assert_no_errors(&preview, "self preview");
    assert_eq!(
        preview["data"]["previewMutation"]["decision"],
        "needs_approval"
    );
    assert_eq!(
        preview["data"]["previewMutation"]["intent"]["approvalRoute"]["role"],
        "finance_approver"
    );
    let intent_id = preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let denied = approve_intent(&server, "finance-approver", &intent_id).await;
    assert_error_code(&denied, "intent_authorization_failed");
    assert_eq!(budget_cents(&server).await, json!(5000));
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
    let rejection_audit = audit_by_intent(&server, &intent_id, Some("intent.reject")).await;
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
