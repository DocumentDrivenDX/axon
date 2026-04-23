//! GraphQL FEAT-029 policy contract tests.
//!
//! These tests exercise the public `/graphql` endpoint so policy-safe read
//! semantics are verified across HTTP caller resolution, dynamic schema
//! generation, and `AxonHandler` policy enforcement.

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

async fn effective_policy_as(
    server: &axum_test::TestServer,
    actor: &str,
    entity_id: Option<&str>,
) -> Value {
    let entity_arg = entity_id
        .map(|id| format!(r#", entityId: "{id}""#))
        .unwrap_or_default();
    let body = gql_as(
        server,
        actor,
        &format!(
            r#"{{
                effectivePolicy(collection: "task"{entity_arg}) {{
                    collection
                    canRead
                    canCreate
                    canUpdate
                    canDelete
                    redactedFields
                    deniedFields
                    policyVersion
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        body["errors"].is_null(),
        "unexpected effectivePolicy errors for {actor}: {body}"
    );
    body["data"]["effectivePolicy"].clone()
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
                    operationIndex
                    decision
                    reason
                    policyVersion
                    ruleIds
                    policyIds
                    fieldPaths
                    deniedFields
                    rules {{ ruleId name kind fieldPath }}
                    approval {{
                        policyId
                        name
                        decision
                        role
                        reasonRequired
                        deadlineSeconds
                        separationOfDuties
                    }}
                    operations {{
                        operation
                        operationIndex
                        decision
                        reason
                        policyVersion
                        deniedFields
                        approval {{
                            policyId
                            name
                            decision
                            role
                            reasonRequired
                            deadlineSeconds
                            separationOfDuties
                        }}
                    }}
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        body["errors"].is_null(),
        "unexpected explainPolicy errors for {actor}: {body}"
    );
    body["data"]["explainPolicy"].clone()
}

async fn seed_policy_fixture(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/user")
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
                        "target_collection": "task",
                        "cardinality": "many-to-many"
                    }
                },
                "access_control": {
                    "read": { "allow": [{ "name": "users-visible" }] },
                    "create": { "allow": [{ "name": "seed-users" }] }
                }
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
                        "requester_id": { "type": "string" },
                        "assigned_contractor_id": { "type": "string" },
                        "budget_cents": { "type": "integer" },
                        "secret": { "type": "string" }
                    }
                },
                "indexes": [
                    { "field": "requester_id", "type": "string" },
                    { "field": "assigned_contractor_id", "type": "string" }
                ],
                "access_control": {
                    "read": {
                        "allow": [
                            {
                                "name": "admins-and-finance-read-tasks",
                                "when": { "subject": "user_id", "in": ["admin", "finance-agent"] }
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
                            "name": "admins-and-finance-create-tasks",
                            "when": { "subject": "user_id", "in": ["admin", "finance-agent"] }
                        }]
                    },
                    "update": {
                        "allow": [{
                            "name": "admins-and-finance-update-tasks",
                            "when": { "subject": "user_id", "in": ["admin", "finance-agent"] }
                        }]
                    },
                    "delete": {
                        "allow": [{
                            "name": "admins-delete-tasks",
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
                                "deny": [
                                    {
                                        "name": "contractors-cannot-write-secret",
                                        "when": { "subject": "user_id", "eq": "contractor" }
                                    },
                                    {
                                        "name": "finance-cannot-write-secret",
                                        "when": { "subject": "user_id", "eq": "finance-agent" }
                                    }
                                ]
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

    for (collection, id, data) in [
        ("user", "u1", json!({ "name": "Ada" })),
        (
            "task",
            "task-a",
            json!({
                "title": "Visible A",
                "requester_id": "requester",
                "assigned_contractor_id": "contractor",
                "budget_cents": 5000,
                "secret": "alpha"
            }),
        ),
        (
            "task",
            "task-b",
            json!({
                "title": "Hidden B",
                "requester_id": "other-requester",
                "assigned_contractor_id": "other-contractor",
                "budget_cents": 4000,
                "secret": "beta"
            }),
        ),
        (
            "task",
            "task-c",
            json!({
                "title": "Visible C",
                "requester_id": "requester",
                "assigned_contractor_id": "other-contractor",
                "budget_cents": 3000,
                "secret": "gamma"
            }),
        ),
        (
            "task",
            "task-contractor",
            json!({
                "title": "Contractor visible",
                "requester_id": "other-requester",
                "assigned_contractor_id": "contractor",
                "budget_cents": 2500,
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
                "source_collection": "user",
                "source_id": "u1",
                "target_collection": "task",
                "target_id": target,
                "link_type": "assigned-to"
            }))
            .await
            .assert_status(StatusCode::CREATED);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_policy_read_semantics_are_safe() {
    let server = test_server();
    seed_policy_fixture(&server).await;

    let point = gql_as(
        &server,
        "requester",
        r#"{
            hiddenTyped: task(id: "task-b") { id title }
            hiddenGeneric: entity(collection: "task", id: "task-b") { id data }
            visibleTyped: task(id: "task-a") { id title secret }
        }"#,
    )
    .await;
    assert!(
        point["errors"].is_null(),
        "unexpected point errors: {point}"
    );
    assert_eq!(point["data"]["hiddenTyped"], Value::Null);
    assert_eq!(point["data"]["hiddenGeneric"], Value::Null);
    assert_eq!(point["data"]["visibleTyped"]["id"], "task-a");
    assert_eq!(point["data"]["visibleTyped"]["secret"], "alpha");

    let first_page = gql_as(
        &server,
        "requester",
        r#"{
            tasksConnection(limit: 1) {
                totalCount
                edges { node { id title } }
                pageInfo { hasNextPage hasPreviousPage endCursor }
            }
        }"#,
    )
    .await;
    assert!(
        first_page["errors"].is_null(),
        "unexpected first page errors: {first_page}"
    );
    let connection = &first_page["data"]["tasksConnection"];
    assert_eq!(connection["totalCount"], 2);
    assert_eq!(connection["edges"].as_array().unwrap().len(), 1);
    assert_eq!(connection["edges"][0]["node"]["id"], "task-a");
    assert_eq!(connection["pageInfo"]["hasNextPage"], true);
    assert_eq!(connection["pageInfo"]["hasPreviousPage"], false);

    let after = connection["pageInfo"]["endCursor"].as_str().unwrap();
    let second_page = gql_as(
        &server,
        "requester",
        &format!(
            r#"{{
                tasksConnection(limit: 1, afterId: "{after}") {{
                    totalCount
                    edges {{ node {{ id title }} }}
                    pageInfo {{ hasNextPage hasPreviousPage }}
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        second_page["errors"].is_null(),
        "unexpected second page errors: {second_page}"
    );
    let connection = &second_page["data"]["tasksConnection"];
    assert_eq!(connection["totalCount"], 2);
    assert_eq!(connection["edges"].as_array().unwrap().len(), 1);
    assert_eq!(connection["edges"][0]["node"]["id"], "task-c");
    assert_eq!(connection["pageInfo"]["hasNextPage"], false);
    assert_eq!(connection["pageInfo"]["hasPreviousPage"], true);

    let related = gql_as(
        &server,
        "requester",
        r#"{
            user(id: "u1") {
                assignedTo {
                    totalCount
                    edges { node { id title } }
                }
            }
        }"#,
    )
    .await;
    assert!(
        related["errors"].is_null(),
        "unexpected relationship errors: {related}"
    );
    let assigned = &related["data"]["user"]["assignedTo"];
    assert_eq!(assigned["totalCount"], 1);
    assert_eq!(assigned["edges"].as_array().unwrap().len(), 1);
    assert_eq!(assigned["edges"][0]["node"]["id"], "task-a");

    let redacted = gql_as(
        &server,
        "contractor",
        r#"{
            typed: task(id: "task-contractor") { id title secret }
            generic: entity(collection: "task", id: "task-contractor") { id data }
        }"#,
    )
    .await;
    assert!(
        redacted["errors"].is_null(),
        "unexpected redaction errors: {redacted}"
    );
    assert_eq!(redacted["data"]["typed"]["id"], "task-contractor");
    assert_eq!(redacted["data"]["typed"]["secret"], Value::Null);
    assert_eq!(
        redacted["data"]["generic"]["data"]["secret"],
        Value::Null,
        "generic entity payloads must use the same field redaction"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_effective_policy_reports_subject_capabilities() {
    let server = test_server();
    seed_policy_fixture(&server).await;

    let admin = effective_policy_as(&server, "admin", None).await;
    assert_eq!(admin["collection"], "task");
    assert_eq!(admin["canRead"], true);
    assert_eq!(admin["canCreate"], true);
    assert_eq!(admin["canUpdate"], true);
    assert_eq!(admin["canDelete"], true);
    assert_eq!(admin["redactedFields"], json!([]));
    assert_eq!(admin["deniedFields"], json!([]));
    assert_eq!(admin["policyVersion"], 1);

    let admin_entity = effective_policy_as(&server, "admin", Some("task-b")).await;
    assert_eq!(admin_entity["canRead"], true);
    assert_eq!(admin_entity["canCreate"], true);
    assert_eq!(admin_entity["canUpdate"], true);
    assert_eq!(admin_entity["canDelete"], true);

    let finance = effective_policy_as(&server, "finance-agent", None).await;
    assert_eq!(finance["canRead"], true);
    assert_eq!(finance["canCreate"], true);
    assert_eq!(finance["canUpdate"], true);
    assert_eq!(finance["canDelete"], false);
    assert_eq!(finance["redactedFields"], json!([]));
    assert_eq!(finance["deniedFields"], json!(["secret"]));

    let finance_entity = effective_policy_as(&server, "finance-agent", Some("task-b")).await;
    assert_eq!(finance_entity["canRead"], true);
    assert_eq!(finance_entity["canCreate"], true);
    assert_eq!(finance_entity["canUpdate"], true);
    assert_eq!(finance_entity["canDelete"], false);
    assert_eq!(finance_entity["deniedFields"], json!(["secret"]));

    let requester_collection = effective_policy_as(&server, "requester", None).await;
    assert_eq!(requester_collection["canRead"], true);
    assert_eq!(requester_collection["canCreate"], false);
    assert_eq!(requester_collection["canUpdate"], false);
    assert_eq!(requester_collection["canDelete"], false);
    assert_eq!(requester_collection["redactedFields"], json!([]));
    assert_eq!(requester_collection["deniedFields"], json!([]));

    let requester_entity = effective_policy_as(&server, "requester", Some("task-a")).await;
    assert_eq!(requester_entity["canRead"], true);
    assert_eq!(requester_entity["canCreate"], false);
    assert_eq!(requester_entity["canUpdate"], false);
    assert_eq!(requester_entity["canDelete"], false);

    let contractor_collection = effective_policy_as(&server, "contractor", None).await;
    assert_eq!(contractor_collection["canRead"], true);
    assert_eq!(contractor_collection["canCreate"], false);
    assert_eq!(contractor_collection["canUpdate"], false);
    assert_eq!(contractor_collection["canDelete"], false);
    assert_eq!(contractor_collection["redactedFields"], json!(["secret"]));
    assert_eq!(contractor_collection["deniedFields"], json!(["secret"]));

    let contractor = effective_policy_as(&server, "contractor", Some("task-contractor")).await;
    assert_eq!(contractor["canRead"], true);
    assert_eq!(contractor["canCreate"], false);
    assert_eq!(contractor["canUpdate"], false);
    assert_eq!(contractor["canDelete"], false);
    assert_eq!(contractor["redactedFields"], json!(["secret"]));
    assert_eq!(contractor["deniedFields"], json!(["secret"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_explain_policy_reports_rules_denials_and_approval_envelopes() {
    let server = test_server();
    seed_policy_fixture(&server).await;

    let read = explain_policy_as(
        &server,
        "requester",
        r#"{ operation: "read", collection: "task", entityId: "task-a" }"#,
    )
    .await;
    assert_eq!(read["decision"], "allow");
    assert_eq!(read["reason"], "allowed");
    assert_eq!(read["policyVersion"], 1);
    let read_rule_names: Vec<_> = read["rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|rule| rule["name"].as_str())
        .collect();
    assert!(
        read_rule_names.contains(&"requesters-read-own-tasks"),
        "read explanation should name the matching requester rule: {read}"
    );

    let field_denial = explain_policy_as(
        &server,
        "finance-agent",
        r#"{
            operation: "update",
            collection: "task",
            entityId: "task-a",
            data: {
                title: "Secret change",
                requester_id: "requester",
                assigned_contractor_id: "contractor",
                budget_cents: 5000,
                secret: "changed"
            }
        }"#,
    )
    .await;
    assert_eq!(field_denial["decision"], "deny");
    assert_eq!(field_denial["reason"], "field_write_denied");
    assert_eq!(field_denial["deniedFields"], json!(["secret"]));
    assert_eq!(field_denial["fieldPaths"], json!(["secret"]));
    let denial_rule_names: Vec<_> = field_denial["rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|rule| rule["name"].as_str())
        .collect();
    assert!(
        denial_rule_names.contains(&"finance-cannot-write-secret"),
        "field denial should name the matching rule: {field_denial}"
    );

    let approval = explain_policy_as(
        &server,
        "finance-agent",
        r#"{
            operation: "update",
            collection: "task",
            entityId: "task-a",
            data: {
                title: "Large budget",
                requester_id: "requester",
                assigned_contractor_id: "contractor",
                budget_cents: 20000
            }
        }"#,
    )
    .await;
    assert_eq!(approval["decision"], "needs_approval");
    assert_eq!(approval["reason"], "needs_approval");
    assert_eq!(
        approval["approval"]["name"],
        "large-budget-needs-finance-approval"
    );
    assert_eq!(approval["approval"]["decision"], "needs_approval");
    assert_eq!(approval["approval"]["role"], "finance_approver");
    assert_eq!(approval["approval"]["reasonRequired"], true);
    assert_eq!(approval["approval"]["deadlineSeconds"], 86400);
    assert_eq!(approval["approval"]["separationOfDuties"], true);

    let transaction = explain_policy_as(
        &server,
        "finance-agent",
        r#"{
            operation: "transaction",
            operations: [{
                updateEntity: {
                    collection: "task",
                    id: "task-a",
                    expectedVersion: 1,
                    data: {
                        title: "Bulk budget",
                        requester_id: "requester",
                        assigned_contractor_id: "contractor",
                        budget_cents: 20000
                    }
                }
            }]
        }"#,
    )
    .await;
    assert_eq!(transaction["decision"], "needs_approval");
    assert_eq!(transaction["operations"][0]["operation"], "update");
    assert_eq!(transaction["operations"][0]["operationIndex"], 0);
    assert_eq!(
        transaction["operations"][0]["approval"]["name"],
        "large-budget-needs-finance-approval"
    );

    let direct_mutation = gql_as(
        &server,
        "finance-agent",
        r#"mutation {
            updateTask(
                id: "task-a",
                version: 1,
                input: {
                    title: "Large budget",
                    requester_id: "requester",
                    assigned_contractor_id: "contractor",
                    budget_cents: 20000
                }
            ) { id version title }
        }"#,
    )
    .await;
    assert_eq!(
        direct_mutation["errors"][0]["extensions"]["code"],
        "forbidden"
    );
    assert_eq!(
        direct_mutation["errors"][0]["extensions"]["detail"]["reason"],
        "needs_approval"
    );

    let stored = gql_as(
        &server,
        "admin",
        r#"{ entity(collection: "task", id: "task-a") { id data } }"#,
    )
    .await;
    assert_eq!(stored["data"]["entity"]["data"]["budget_cents"], 5000);
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_put_schema_exposes_policy_compile_reports_and_errors() {
    let server = test_server();

    server
        .post("/tenants/default/databases/default/collections/policy_report")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "required": ["title", "secret"],
                    "properties": {
                        "title": { "type": "string" },
                        "secret": { "type": "string" },
                        "amount_cents": { "type": "integer" }
                    }
                }
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);

    let dry_run = gql_as(
        &server,
        "admin",
        r#"mutation {
            putSchema(input: {
                collection: "policy_report",
                dryRun: true,
                schema: {
                    version: 2,
                    entitySchema: {
                        type: "object",
                        required: ["title", "secret"],
                        properties: {
                            title: { type: "string" },
                            secret: { type: "string" },
                            amount_cents: { type: "integer" }
                        }
                    },
                    accessControl: {
                        read: { allow: [{ name: "admins-read", when: { subject: "user_id", eq: "admin" } }] },
                        update: { allow: [{ name: "admins-update", when: { subject: "user_id", eq: "admin" } }] },
                        fields: {
                            secret: {
                                read: {
                                    deny: [{
                                        name: "mask-secret-for-contractors",
                                        when: { subject: "user_id", eq: "contractor" },
                                        redact_as: null
                                    }]
                                },
                                write: {
                                    deny: [{
                                        name: "contractors-cannot-write-secret",
                                        when: { subject: "user_id", eq: "contractor" }
                                    }]
                                }
                            }
                        },
                        envelopes: {
                            write: [{
                                name: "large-amount-needs-approval",
                                when: {
                                    all: [
                                        { operation: "update" },
                                        { field: "amount_cents", gt: 10000 }
                                    ]
                                },
                                decision: "needs_approval",
                                approval: {
                                    role: "finance_approver",
                                    reason_required: true,
                                    deadline_seconds: 86400,
                                    separation_of_duties: true
                                }
                            }]
                        }
                    }
                }
            }) {
                dryRun
                schema
                policyCompileReport
            }
        }"#,
    )
    .await;
    assert!(dry_run["errors"].is_null(), "unexpected errors: {dry_run}");
    let report = &dry_run["data"]["putSchema"]["policyCompileReport"];
    assert_eq!(dry_run["data"]["putSchema"]["dryRun"], true);
    assert_eq!(dry_run["data"]["putSchema"]["schema"]["version"], 2);
    assert_eq!(report["nullable_fields"][0]["field"], "secret");
    assert_eq!(report["nullable_fields"][0]["required_by_schema"], true);
    assert_eq!(report["denied_write_fields"][0]["field"], "secret");
    assert_eq!(
        report["envelope_summaries"][0]["name"],
        "large-amount-needs-approval"
    );
    assert_eq!(
        report["envelope_summaries"][0]["decision"],
        "needs_approval"
    );

    let active = gql_as(
        &server,
        "admin",
        r#"{ collection(name: "policy_report") { name schemaVersion } }"#,
    )
    .await;
    assert_eq!(active["data"]["collection"]["schemaVersion"], 1);

    let invalid_path = gql_as(
        &server,
        "admin",
        r#"mutation {
            putSchema(input: {
                collection: "policy_report",
                dryRun: true,
                schema: {
                    version: 2,
                    entitySchema: {
                        type: "object",
                        properties: { title: { type: "string" } }
                    },
                    accessControl: {
                        read: {
                            allow: [{
                                name: "bad-path",
                                where: { field: "missing_field", eq: "x" }
                            }]
                        }
                    }
                }
            }) { dryRun policyCompileReport }
        }"#,
    )
    .await;
    assert_eq!(
        invalid_path["errors"][0]["extensions"]["code"],
        "SCHEMA_VALIDATION"
    );
    assert!(
        invalid_path["errors"][0]["extensions"]["detail"]
            .as_str()
            .unwrap()
            .contains("unknown field path 'missing_field'"),
        "invalid path error should report the bad path: {invalid_path}"
    );

    server
        .post("/tenants/default/databases/default/collections/a")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": { "name": { "type": "string" } }
                },
                "link_types": {
                    "to_b": {
                        "target_collection": "b",
                        "cardinality": "many-to-one"
                    }
                },
                "access_control": {
                    "read": { "allow": [{ "name": "a-visible" }] }
                }
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);
    server
        .post("/tenants/default/databases/default/collections/b")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": { "name": { "type": "string" } }
                },
                "link_types": {
                    "to_a": {
                        "target_collection": "a",
                        "cardinality": "many-to-one"
                    }
                },
                "access_control": {
                    "read": {
                        "allow": [{
                            "name": "b-through-a",
                            "where": {
                                "related": {
                                    "link_type": "to_a",
                                    "target_collection": "a",
                                    "target_policy": "read"
                                }
                            }
                        }]
                    }
                }
            },
            "actor": "setup"
        }))
        .await
        .assert_status(StatusCode::CREATED);
    let cycle = gql_as(
        &server,
        "admin",
        r#"mutation {
            putSchema(input: {
                collection: "a",
                dryRun: true,
                schema: {
                    version: 2,
                    entitySchema: {
                        type: "object",
                        properties: { name: { type: "string" } }
                    },
                    linkTypes: {
                        to_b: { target_collection: "b", cardinality: "many-to-one" }
                    },
                    accessControl: {
                        read: {
                            allow: [{
                                name: "a-through-b",
                                where: {
                                    related: {
                                        link_type: "to_b",
                                        target_collection: "b",
                                        target_policy: "read"
                                    }
                                }
                            }]
                        }
                    }
                }
            }) { dryRun policyCompileReport }
        }"#,
    )
    .await;
    assert_eq!(
        cycle["errors"][0]["extensions"]["code"],
        "SCHEMA_VALIDATION"
    );
    assert!(
        cycle["errors"][0]["extensions"]["detail"]
            .as_str()
            .unwrap()
            .contains("relationship target_policy cycle detected"),
        "cycle error should report the target_policy cycle: {cycle}"
    );

    let applied = gql_as(
        &server,
        "admin",
        r#"mutation {
            putSchema(input: {
                collection: "policy_report",
                schema: {
                    version: 2,
                    entitySchema: {
                        type: "object",
                        required: ["title", "secret"],
                        properties: {
                            title: { type: "string" },
                            secret: { type: "string" },
                            amount_cents: { type: "integer" }
                        }
                    },
                    accessControl: {
                        read: { allow: [{ name: "admins-read", when: { subject: "user_id", eq: "admin" } }] },
                        update: { allow: [{ name: "admins-update", when: { subject: "user_id", eq: "admin" } }] },
                        envelopes: {
                            write: [{
                                name: "large-amount-needs-approval",
                                when: {
                                    all: [
                                        { operation: "update" },
                                        { field: "amount_cents", gt: 10000 }
                                    ]
                                },
                                decision: "needs_approval",
                                approval: { role: "finance_approver", reason_required: true }
                            }]
                        }
                    }
                }
            }) { dryRun schema policyCompileReport }
        }"#,
    )
    .await;
    assert!(applied["errors"].is_null(), "unexpected errors: {applied}");
    assert_eq!(applied["data"]["putSchema"]["dryRun"], false);
    assert_eq!(applied["data"]["putSchema"]["schema"]["version"], 2);

    let audit = gql_as(
        &server,
        "admin",
        r#"{
            auditLog(collection: "policy_report", operation: "schema.update") {
                totalCount
                edges { node { metadata } }
            }
        }"#,
    )
    .await;
    let metadata = &audit["data"]["auditLog"]["edges"][0]["node"]["metadata"];
    assert_eq!(metadata["old_schema_version"], "1");
    assert_eq!(metadata["new_schema_version"], "2");
    assert_eq!(metadata["old_policy_version"], "none");
    assert_eq!(metadata["new_policy_version"], "2");
    assert_eq!(metadata["policy_envelopes"], "1");
}
