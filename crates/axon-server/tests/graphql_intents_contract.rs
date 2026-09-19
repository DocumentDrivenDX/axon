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
                    canonicalOperation {{ operationHash }}
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
    // @covers US-106-AC1
    // @covers US-107-AC5
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
    // @covers US-106-AC1
    // @covers US-106-AC2
    // @covers US-106-AC4
    // @covers US-106-AC5
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 20_000, 600).await;
    assert_no_errors(&preview, "preview");
    let result = &preview["data"]["previewMutation"];
    assert_eq!(result["decision"], "needs_approval");
    assert_eq!(result["intent"]["approvalState"], "pending");
    let token = result["intentToken"].as_str().unwrap().to_string();
    let intent_id = result["intent"]["id"].as_str().unwrap().to_string();
    let preview_audit = audit_by_intent(&server, &intent_id, Some("mutation_intent.preview")).await;
    let preview_entries = preview_audit["entries"].as_array().unwrap();
    assert_eq!(preview_entries.len(), 1);
    assert_eq!(preview_entries[0]["actor"], "finance-agent");
    assert_eq!(preview_entries[0]["collection"], "__mutation_intents");
    assert_eq!(preview_entries[0]["entity_id"], intent_id);
    assert!(preview_entries[0]["data_before"].is_null());
    assert_eq!(preview_entries[0]["intent_lineage"]["intent_id"], intent_id);
    assert_eq!(
        preview_entries[0]["intent_lineage"]["subject_snapshot"]["user_id"],
        "finance-agent"
    );
    assert_eq!(preview_entries[0]["intent_lineage"]["policy_version"], 1);
    assert_eq!(
        preview_entries[0]["intent_lineage"]["origin"]["surface"],
        "graphql"
    );
    assert_eq!(
        preview_entries[0]["intent_lineage"]["origin"]["operation_hash"],
        result["canonicalOperation"]["operationHash"]
    );
    assert_eq!(preview_entries[0]["metadata"]["decision"], "needs_approval");
    assert_eq!(preview_entries[0]["metadata"]["schema_version"], "1");
    assert_eq!(preview_entries[0]["metadata"]["policy_version"], "1");
    assert_eq!(
        preview_entries[0]["metadata"]["operation_hash"],
        result["canonicalOperation"]["operationHash"]
    );
    assert!(preview_entries[0]["metadata"]["expires_at"]
        .as_str()
        .is_some());
    assert_eq!(
        preview_entries[0]["data_after"]["diff"]["budget_cents"]["after"],
        20_000
    );
    assert!(
        preview_entries[0]["data_after"].get("operation").is_none(),
        "preview after payload should contain the review summary only"
    );

    let graphql_preview_audit = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"{{
                auditLog(collection: "__mutation_intents", entityId: "{intent_id}") {{
                    totalCount
                    edges {{
                        node {{
                            operation
                            actor
                            collection
                            entityId
                            metadata
                            dataBefore
                            dataAfter
                        }}
                    }}
                }}
            }}"#
        ),
    )
    .await;
    assert_eq!(
        graphql_preview_audit["errors"][0]["extensions"]["code"],
        "INVALID_ARGUMENT"
    );
    let message = graphql_preview_audit["errors"][0]["message"]
        .as_str()
        .unwrap();
    assert!(message.contains("\"code\":\"reserved_namespace\""));
    assert!(message.contains("\"reason\":\"generic_access_forbidden\""));
    assert!(message.contains("\"name\":\"__mutation_intents\""));
    assert!(message.contains("\"operation\":\"audit\""));

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
    assert_eq!(
        approval_entries[0]["intent_lineage"]["origin"]["surface"],
        "graphql"
    );
    assert_eq!(
        approval_entries[0]["intent_lineage"]["approver"]["actor"],
        "finance-approver"
    );
    assert!(
        approval_entries[0]["intent_lineage"]["approver"]["tenant_role"]
            .as_str()
            .is_some()
    );

    let committed = commit_token(&server, "finance-agent", &token).await;
    assert_no_errors(&committed, "commit");
    assert_eq!(
        committed["data"]["commitMutationIntent"]["intent"]["approvalState"],
        "committed"
    );
    assert_eq!(budget_cents(&server).await, json!(20_000));

    let lineage_audit = audit_by_intent(&server, &intent_id, None).await;
    let lineage_entries = lineage_audit["entries"].as_array().unwrap();
    assert_eq!(lineage_entries.len(), 3);
    assert!(lineage_entries
        .iter()
        .any(|entry| entry["mutation"] == "mutation_intent.preview"
            && entry["actor"] == "finance-agent"));
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

    let graphql_lineage_audit = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"{{
                auditLog(intentId: "{intent_id}") {{
                    totalCount
                    edges {{
                        node {{
                            mutation
                            actor
                            collection
                            entityId
                            intentLineage {{ intentId policyVersion origin }}
                        }}
                    }}
                }}
            }}"#
        ),
    )
    .await;
    assert_no_errors(&graphql_lineage_audit, "GraphQL auditLog intent lookup");
    let graphql_lineage_entries = graphql_lineage_audit["data"]["auditLog"]["edges"]
        .as_array()
        .unwrap();
    assert_eq!(graphql_lineage_audit["data"]["auditLog"]["totalCount"], 3);
    assert!(graphql_lineage_entries.iter().any(|edge| {
        edge["node"]["mutation"] == "mutation_intent.preview"
            && edge["node"]["intentLineage"]["intentId"] == intent_id
    }));
    assert!(graphql_lineage_entries.iter().any(|edge| {
        edge["node"]["mutation"] == "intent.approve" && edge["node"]["actor"] == "finance-approver"
    }));
    assert!(graphql_lineage_entries.iter().any(|edge| {
        edge["node"]["mutation"] == "entity.update"
            && edge["node"]["collection"] == "task"
            && edge["node"]["entityId"] == "task-a"
    }));

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
    // @covers US-106-AC2
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
    // @covers US-106-AC4
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
    // @covers US-106-AC4
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
    // @covers US-107-AC5
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
async fn pending_query_materializes_and_audits_expired_intent_lineage() {
    // @covers US-107-AC5
    let server = test_server();
    seed_intent_fixture(&server).await;

    let preview = preview_budget_patch(&server, 22_000, 0).await;
    assert_no_errors(&preview, "preview");
    let intent_id = preview["data"]["previewMutation"]["intent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::sleep(Duration::from_millis(1)).await;

    let queried = gql_as(
        &server,
        "finance-approver",
        &format!(
            r#"{{
                pendingMutationIntents(filter: {{ status: "expired" }}, limit: 10) {{
                    totalCount
                    edges {{ node {{ id approvalState decision }} }}
                }}
                mutationIntent(id: "{intent_id}") {{ id approvalState decision }}
            }}"#
        ),
    )
    .await;
    assert_no_errors(&queried, "expired intent query");
    assert_eq!(
        queried["data"]["mutationIntent"]["approvalState"],
        "expired"
    );
    assert_eq!(
        queried["data"]["pendingMutationIntents"]["edges"][0]["node"]["id"],
        intent_id
    );
    assert_eq!(
        queried["data"]["pendingMutationIntents"]["edges"][0]["node"]["approvalState"],
        "expired"
    );

    let expiry_audit = audit_by_intent(&server, &intent_id, Some("intent.expire")).await;
    let expiry_entries = expiry_audit["entries"].as_array().unwrap();
    assert_eq!(expiry_entries.len(), 1);
    assert_eq!(expiry_entries[0]["actor"], "system");
    assert_eq!(expiry_entries[0]["collection"], "__mutation_intents");
    assert_eq!(expiry_entries[0]["entity_id"], intent_id);
    assert_eq!(
        expiry_entries[0]["data_before"]["approval_state"],
        "pending"
    );
    assert_eq!(expiry_entries[0]["data_after"]["approval_state"], "expired");
    assert_eq!(expiry_entries[0]["intent_lineage"]["intent_id"], intent_id);
    assert_eq!(expiry_entries[0]["intent_lineage"]["policy_version"], 1);
    assert_eq!(
        expiry_entries[0]["intent_lineage"]["origin"]["surface"],
        "system"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn denied_preview_has_no_executable_token() {
    // @covers US-105-AC2
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

// Bump the task collection schema to version 2 (identical access_control, new version number).
async fn bump_task_schema(server: &axum_test::TestServer) {
    server
        .put("/tenants/default/databases/default/collections/task/schema")
        .json(&json!({
            "version": 2,
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
                "read": { "allow": [{ "name": "fixture-read" }] },
                "create": { "allow": [{ "name": "fixture-create" }] },
                "update": {
                    "allow": [{
                        "name": "fixture-update",
                        "when": { "subject": "user_id", "in": ["finance-agent", "finance-approver"] }
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
        }))
        .await
        .assert_status_ok();
}

async fn create_task_b(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/entities/task/task-b")
        .add_header("x-axon-actor", "admin")
        .json(&json!({
            "data": {
                "title": "Secondary task",
                "budget_cents": 1000,
                "secret": "beta",
                "status": "draft"
            },
            "actor": "admin"
        }))
        .await
        .assert_status(StatusCode::CREATED);
}

async fn touch_task_b(server: &axum_test::TestServer) {
    server
        .put("/tenants/default/databases/default/entities/task/task-b")
        .add_header("x-axon-actor", "finance-approver")
        .json(&json!({
            "data": {
                "title": "Secondary task - out-of-band update",
                "budget_cents": 1000,
                "status": "draft"
            },
            "expected_version": 1,
            "actor": "finance-approver"
        }))
        .await
        .assert_status_ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn preview_decision_determinism_matches_commit_time_evaluation() {
    // @covers US-105-AC4
    let server = test_server();
    seed_intent_fixture(&server).await;

    // Two previews of the same operation with the same entity state must yield the same decision.
    let preview_a = preview_budget_patch(&server, 6000, 600).await;
    let preview_b = preview_budget_patch(&server, 6000, 600).await;
    assert_no_errors(&preview_a, "preview_a");
    assert_no_errors(&preview_b, "preview_b");

    let result_a = &preview_a["data"]["previewMutation"];
    let result_b = &preview_b["data"]["previewMutation"];

    // Same state → same decision (deterministic policy evaluation).
    assert_eq!(result_a["decision"], "allow");
    assert_eq!(result_b["decision"], "allow");
    // Same operation text → identical canonical hash (canonicalization is deterministic).
    assert_eq!(
        result_a["canonicalOperation"]["operationHash"],
        result_b["canonicalOperation"]["operationHash"],
        "preview of the same operation must produce the same operation hash"
    );

    let token_a = result_a["intentToken"].as_str().unwrap().to_string();
    let token_b = result_b["intentToken"].as_str().unwrap().to_string();

    // Commit token_a — the commit-time evaluation must agree with the preview decision.
    let committed = commit_token(&server, "finance-agent", &token_a).await;
    assert_no_errors(&committed, "commit_a");
    assert_eq!(committed["data"]["commitMutationIntent"]["committed"], true);
    assert_eq!(budget_cents(&server).await, json!(6000));

    // Token_b is now stale: entity pre-image version changed (v1 → v2 after commit_a).
    // This proves commit validates pre-image bindings using the same rules as preview.
    let stale = commit_token(&server, "finance-agent", &token_b).await;
    assert_error_code(&stale, "intent_stale");
    let stale_dims = stale["errors"][0]["extensions"]["stale"]
        .as_array()
        .unwrap();
    assert!(
        stale_dims.iter().any(|d| d["dimension"] == "pre_image"),
        "stale must name pre_image dimension: {stale_dims:?}"
    );
    // Entity remains at the committed value (token_b commit was rejected, not rolled back).
    assert_eq!(budget_cents(&server).await, json!(6000));
}

#[tokio::test(flavor = "multi_thread")]
async fn direct_write_intercepted_no_mutation_no_audit() {
    // @covers US-106-AC3
    let server = test_server();
    seed_intent_fixture(&server).await;

    // Capture audit baseline: only the creation entry should exist.
    let before = audit_entity(&server).await;
    let update_entries_before: Vec<_> = before["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["mutation"] == "entity.update")
        .collect();
    assert!(
        update_entries_before.is_empty(),
        "no entity.update entries expected before direct write attempt"
    );

    // Direct write via the dynamically-generated GraphQL mutation, budget > 10000
    // triggers the approval envelope and must be intercepted.
    let direct = gql_as(
        &server,
        "finance-agent",
        r#"mutation {
            patchTask(id: "task-a", version: 1, patch: "{\"budget_cents\": 20000}") {
                id version
            }
        }"#,
    )
    .await;

    // Write is intercepted: approval required.
    assert_eq!(direct["errors"][0]["extensions"]["code"], "forbidden");
    assert_eq!(
        direct["errors"][0]["extensions"]["detail"]["reason"],
        "needs_approval"
    );

    // Entity state unchanged.
    assert_eq!(budget_cents(&server).await, json!(5000));

    // No entity.update audit entry was produced by the rejected write.
    let after = audit_entity(&server).await;
    let update_entries_after: Vec<_> = after["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["mutation"] == "entity.update")
        .collect();
    assert!(
        update_entries_after.is_empty(),
        "direct write intercepted by approval envelope must not produce an entity.update audit entry"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn policy_version_drift_before_commit_rejects_as_stale() {
    // @covers US-107-AC2
    let server = test_server();
    seed_intent_fixture(&server).await;

    // Preview under threshold — intent is bound to schema_version=1, policy_version=1.
    let preview = preview_budget_patch(&server, 6000, 600).await;
    assert_no_errors(&preview, "preview");
    let result = &preview["data"]["previewMutation"];
    assert_eq!(result["decision"], "allow");
    let token = result["intentToken"].as_str().unwrap().to_string();

    // Advance the collection schema to version 2, bumping the live policy version.
    bump_task_schema(&server).await;

    // Commit now observes schema_version=2 but the intent was bound to version 1.
    let committed = commit_token(&server, "finance-agent", &token).await;
    assert_error_code(&committed, "intent_stale");

    let stale_dims = committed["errors"][0]["extensions"]["stale"]
        .as_array()
        .unwrap();
    assert!(
        stale_dims
            .iter()
            .any(|d| d["dimension"] == "policy_version"),
        "stale rejection must name the policy_version dimension: {stale_dims:?}"
    );

    // Entity state unchanged — no partial commit.
    assert_eq!(budget_cents(&server).await, json!(5000));
}

#[tokio::test(flavor = "multi_thread")]
async fn operation_hash_mismatch_rejects_commit() {
    // @covers US-107-AC3
    let server = test_server();
    seed_intent_fixture(&server).await;

    // Preview patching budget to 6000.
    let preview = preview_budget_patch(&server, 6000, 600).await;
    assert_no_errors(&preview, "preview");
    let token = preview["data"]["previewMutation"]["intentToken"]
        .as_str()
        .unwrap()
        .to_string();

    // Commit with a different operation payload (budget 7777 ≠ 6000).
    // The canonical hash of the supplied operation will not match the stored intent hash.
    let mismatched = gql_as(
        &server,
        "finance-agent",
        &format!(
            r#"mutation {{
                commitMutationIntent(input: {{
                    intentToken: "{token}"
                    operation: {{
                        operationKind: "patch_entity"
                        operation: {{
                            collection: "task"
                            id: "task-a"
                            expected_version: 1
                            patch: {{ budget_cents: 7777 }}
                        }}
                    }}
                }}) {{
                    committed transactionId errorCode
                    stale {{ dimension expected actual path }}
                }}
            }}"#
        ),
    )
    .await;

    assert_error_code(&mismatched, "intent_mismatch");
    // Entity state unchanged — mismatched operation was never applied.
    assert_eq!(budget_cents(&server).await, json!(5000));
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_entity_intent_one_stale_entity_invalidates_whole_intent() {
    // @covers US-107-AC4
    let server = test_server();
    seed_intent_fixture(&server).await;
    create_task_b(&server).await;

    // Preview a transaction touching both task-a and task-b (both under the approval threshold).
    let preview = gql_as(
        &server,
        "finance-agent",
        r#"mutation {
            previewMutation(input: {
                operation: {
                    operationKind: "transaction"
                    operation: {
                        operations: [
                            {
                                updateEntity: {
                                    collection: "task"
                                    id: "task-a"
                                    expectedVersion: 1
                                    data: {
                                        title: "Budget request"
                                        budget_cents: 6000
                                        status: "draft"
                                    }
                                }
                            }
                            {
                                updateEntity: {
                                    collection: "task"
                                    id: "task-b"
                                    expectedVersion: 1
                                    data: {
                                        title: "Secondary task"
                                        budget_cents: 2000
                                        status: "draft"
                                    }
                                }
                            }
                        ]
                    }
                }
                expiresInSeconds: 600
            }) {
                decision
                intentToken
                intent { id approvalState decision }
            }
        }"#,
    )
    .await;
    assert_no_errors(&preview, "multi-entity preview");
    let result = &preview["data"]["previewMutation"];
    assert_eq!(result["decision"], "allow");
    let token = result["intentToken"].as_str().unwrap().to_string();

    // Mutate task-b out-of-band: version 1 → 2, making its pre-image stale.
    touch_task_b(&server).await;

    // Commit must fail because task-b's pre-image is stale; no partial write allowed.
    let committed = commit_token(&server, "finance-agent", &token).await;
    assert_error_code(&committed, "intent_stale");

    let stale_dims = committed["errors"][0]["extensions"]["stale"]
        .as_array()
        .unwrap();
    assert!(
        stale_dims.iter().any(|d| {
            d["dimension"] == "pre_image"
                && d["path"].as_str().map_or(false, |p| p.contains("task-b"))
        }),
        "stale rejection must name the task-b pre_image dimension: {stale_dims:?}"
    );

    // task-a must also be unchanged — the intent was rejected atomically.
    assert_eq!(
        budget_cents(&server).await,
        json!(5000),
        "task-a must not be partially committed when the intent is rejected as stale"
    );
}
