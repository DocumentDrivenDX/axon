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
                        "owner_id": { "type": "string" },
                        "secret": { "type": "string" }
                    }
                },
                "indexes": [
                    { "field": "owner_id", "type": "string" }
                ],
                "access_control": {
                    "read": {
                        "allow": [{
                            "name": "owners-read-tasks",
                            "where": { "field": "owner_id", "eq_subject": "user_id" }
                        }]
                    },
                    "create": { "allow": [{ "name": "seed-tasks" }] },
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
        ("user", "u1", json!({ "name": "Ada" })),
        (
            "task",
            "task-a",
            json!({ "title": "Visible A", "owner_id": "alice", "secret": "alpha" }),
        ),
        (
            "task",
            "task-b",
            json!({ "title": "Hidden B", "owner_id": "bob", "secret": "beta" }),
        ),
        (
            "task",
            "task-c",
            json!({ "title": "Visible C", "owner_id": "alice", "secret": "gamma" }),
        ),
        (
            "task",
            "task-contractor",
            json!({ "title": "Contractor visible", "owner_id": "contractor", "secret": "classified" }),
        ),
    ] {
        server
            .post(&format!(
                "/tenants/default/databases/default/entities/{collection}/{id}"
            ))
            .json(&json!({ "data": data, "actor": "setup" }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    for target in ["task-a", "task-b"] {
        server
            .post("/tenants/default/databases/default/links")
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
        "alice",
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
        "alice",
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
        "alice",
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
        "alice",
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
