//! GraphQL consumer parity matrix.
//!
//! These tests exercise the GraphQL surface as a consumer would: live HTTP/WS
//! requests against an in-process gateway, no mocked handler calls.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_graphql::BroadcastBroker;
use axon_server::actor_scope::ActorScopeGuard;
use axon_server::auth::{AuthContext, Role};
use axon_server::cors_config::CorsStore;
use axon_server::gateway::{build_router_with_auth, build_router_with_broker};
use axon_server::rate_limit::RateLimitConfig;
use axon_server::tenant_router::{TenantHandler, TenantRouter};
use axon_storage::adapter::StorageAdapter;
use axon_storage::MemoryStorageAdapter;
use axum::http::{header, HeaderName, HeaderValue, Method};
use serde_json::{json, Value};
use tokio::sync::Mutex;

fn memory_handler() -> TenantHandler {
    let storage: Box<dyn StorageAdapter + Send + Sync> = Box::new(MemoryStorageAdapter::default());
    Arc::new(Mutex::new(AxonHandler::new(storage)))
}

fn memory_server_with_broker() -> (axum_test::TestServer, BroadcastBroker) {
    let tenant_router = Arc::new(TenantRouter::single(memory_handler()));
    let broker = BroadcastBroker::default();
    let app = build_router_with_broker(tenant_router, "memory", None, broker.clone());
    let server = axum_test::TestServer::builder().http_transport().build(app);
    (server, broker)
}

fn memory_server_with_auth(auth: AuthContext, cors: CorsStore) -> axum_test::TestServer {
    let tenant_router = Arc::new(TenantRouter::single(memory_handler()));
    let app = build_router_with_auth(
        tenant_router,
        "memory",
        None,
        auth,
        RateLimitConfig::default(),
        ActorScopeGuard::default(),
        None,
        cors,
    );
    axum_test::TestServer::new(app)
}

async fn gql(server: &axum_test::TestServer, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

async fn gql_at(
    server: &axum_test::TestServer,
    tenant: &str,
    database: &str,
    query: &str,
) -> Value {
    server
        .post(&format!("/tenants/{tenant}/databases/{database}/graphql"))
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

async fn seed_consumer_collections(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/user")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "status": { "type": "string" }
                    }
                },
                "link_types": {
                    "assigned-to": {
                        "target_collection": "task",
                        "cardinality": "many-to-many",
                        "metadata_schema": {
                            "type": "object",
                            "properties": {
                                "role": { "type": "string" }
                            }
                        }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/tenants/default/databases/default/collections/task")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "minLength": 3 },
                        "status": { "type": "string" },
                        "hours": { "type": "number" }
                    },
                    "required": ["title"]
                },
                "lifecycles": {
                    "status": {
                        "field": "status",
                        "initial": "open",
                        "transitions": {
                            "open": ["review"],
                            "review": ["done"],
                            "done": []
                        }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

async fn receive_ws_json(websocket: &mut axum_test::TestWebSocket) -> Value {
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        websocket.receive_json::<Value>(),
    )
    .await
    .expect("websocket message arrives within the timeout")
}

async fn wait_for_receivers(broker: &BroadcastBroker, count: usize) {
    for _ in 0..50 {
        if broker.receiver_count() >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "expected at least {count} subscription receiver(s), got {}",
        broker.receiver_count()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_consumer_canary_crud_query_relationship_subscription_delete() {
    let (server, broker) = memory_server_with_broker();
    seed_consumer_collections(&server).await;

    let mut websocket = server
        .get_websocket("/tenants/default/databases/default/graphql/ws")
        .add_header("sec-websocket-protocol", "graphql-transport-ws")
        .await
        .into_websocket()
        .await;
    websocket
        .send_json(&json!({"type": "connection_init"}))
        .await;
    assert_eq!(
        receive_ws_json(&mut websocket).await["type"],
        "connection_ack"
    );
    websocket
        .send_json(&json!({
            "id": "consumer-sub",
            "type": "subscribe",
            "payload": {
                "query": r#"subscription {
                    taskChanged {
                        auditId
                        collection
                        entityId
                        operation
                        actor
                        data
                    }
                }"#
            }
        }))
        .await;
    wait_for_receivers(&broker, 1).await;

    let create = gql(
        &server,
        r#"mutation {
            createUser(id: "u-canary", input: { name: "Ada", status: "active" }) { id version name }
            createTask(id: "task-canary", input: { title: "Draft plan", status: "open", hours: 4 }) {
                id
                version
                title
                status
                validTransitions(lifecycleName: "status")
            }
        }"#,
    )
    .await;
    assert!(
        create["errors"].is_null(),
        "unexpected create errors: {create}"
    );
    assert_eq!(create["data"]["createTask"]["version"], 1);
    assert_eq!(
        create["data"]["createTask"]["validTransitions"],
        json!(["review"])
    );

    let filtered = gql(
        &server,
        r#"{
            typed: tasks(
                filter: { status: { eq: "open" } }
                sort: [{ field: hours, direction: "desc" }]
                limit: 10
            ) { id title status hours }
            generic: entities(
                collection: "task"
                filter: { field: "status", op: "eq", value: "open" }
                sort: [{ field: "hours", direction: "desc" }]
                limit: 10
            ) {
                totalCount
                edges { cursor node { id collection version data } }
                pageInfo { hasNextPage hasPreviousPage }
            }
            aggregate: taskAggregate(
                filter: { status: { eq: "open" } }
                aggregations: [{ function: COUNT }, { function: SUM, field: hours }]
            ) {
                totalCount
                groups { count values { function field value count } }
            }
            collection(name: "task") { name entityCount schemaVersion schema }
        }"#,
    )
    .await;
    assert!(
        filtered["errors"].is_null(),
        "unexpected filter/query errors: {filtered}"
    );
    assert_eq!(filtered["data"]["typed"][0]["id"], "task-canary");
    assert_eq!(filtered["data"]["generic"]["totalCount"], 1);
    assert_eq!(
        filtered["data"]["generic"]["edges"][0]["node"]["data"]["title"],
        "Draft plan"
    );
    assert_eq!(filtered["data"]["aggregate"]["totalCount"], 1);
    assert_eq!(filtered["data"]["collection"]["entityCount"], 1);

    let update_and_link = gql(
        &server,
        r#"mutation {
            updateTask(
                id: "task-canary"
                version: 1
                input: { title: "Review plan", status: "review", hours: 5 }
            ) { id version title status validTransitions(lifecycleName: "status") }
            createLink(
                sourceCollection: "user"
                sourceId: "u-canary"
                targetCollection: "task"
                targetId: "task-canary"
                linkType: "assigned-to"
                metadata: "{\"role\":\"owner\"}"
            )
        }"#,
    )
    .await;
    assert!(
        update_and_link["errors"].is_null(),
        "unexpected update/link errors: {update_and_link}"
    );
    assert_eq!(update_and_link["data"]["updateTask"]["version"], 2);
    assert_eq!(
        update_and_link["data"]["updateTask"]["validTransitions"],
        json!(["done"])
    );
    assert_eq!(update_and_link["data"]["createLink"], true);

    let relationship = gql(
        &server,
        r#"{
            user(id: "u-canary") {
                assignedTo {
                    totalCount
                    edges { metadata node { id title status } }
                }
            }
            inbound: task(id: "task-canary") {
                assignedToInbound {
                    totalCount
                    edges { metadata node { id name } }
                }
            }
            candidates: linkCandidates(
                sourceCollection: "user"
                sourceId: "u-canary"
                linkType: "assigned-to"
                search: "Review"
            ) {
                existingLinkCount
                candidates { alreadyLinked entity { id data } }
            }
            neighbors(collection: "task", id: "task-canary", direction: "inbound") {
                totalCount
                groups { linkType direction edges { metadata node { id collection data } } }
            }
        }"#,
    )
    .await;
    assert!(
        relationship["errors"].is_null(),
        "unexpected relationship errors: {relationship}"
    );
    assert_eq!(relationship["data"]["user"]["assignedTo"]["totalCount"], 1);
    assert_eq!(
        relationship["data"]["user"]["assignedTo"]["edges"][0]["metadata"]["role"],
        "owner"
    );
    assert_eq!(
        relationship["data"]["inbound"]["assignedToInbound"]["edges"][0]["node"]["id"],
        "u-canary"
    );
    assert_eq!(relationship["data"]["candidates"]["existingLinkCount"], 1);
    assert_eq!(
        relationship["data"]["candidates"]["candidates"][0]["alreadyLinked"],
        true
    );
    assert_eq!(relationship["data"]["neighbors"]["totalCount"], 1);

    let audit = gql(
        &server,
        r#"{
            auditLog(collection: "task", entityId: "task-canary") {
                totalCount
                edges { node { mutation version transactionId dataAfter actor } }
            }
        }"#,
    )
    .await;
    assert!(
        audit["errors"].is_null(),
        "unexpected audit errors: {audit}"
    );
    assert_eq!(audit["data"]["auditLog"]["totalCount"], 2);
    assert!(audit["data"]["auditLog"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["node"]["mutation"] == "entity.update" && edge["node"]["version"] == 2));

    server
        .post("/tenants/default/databases/default/entities/task/subscribed-rest-write")
        .add_header("x-axon-actor", "subscription-probe")
        .json(&json!({"data": {"title": "Subscription probe", "status": "open", "hours": 1}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    let next = receive_ws_json(&mut websocket).await;
    assert_eq!(next["type"], "next");
    assert_eq!(
        next["payload"]["data"]["taskChanged"]["entityId"],
        "subscribed-rest-write"
    );
    assert_eq!(
        next["payload"]["data"]["taskChanged"]["actor"],
        "subscription-probe"
    );
    assert!(!next["payload"]["data"]["taskChanged"]["auditId"]
        .as_str()
        .unwrap()
        .is_empty());

    let delete = gql(
        &server,
        r#"mutation {
            deleteLink(
                sourceCollection: "user"
                sourceId: "u-canary"
                targetCollection: "task"
                targetId: "task-canary"
                linkType: "assigned-to"
            )
            deleteTask(id: "task-canary") { deleted }
        }"#,
    )
    .await;
    assert!(
        delete["errors"].is_null(),
        "unexpected delete errors: {delete}"
    );
    assert_eq!(delete["data"]["deleteLink"], true);
    assert_eq!(delete["data"]["deleteTask"]["deleted"], true);

    let final_state = gql(
        &server,
        r#"{ task(id: "task-canary") { id } user(id: "u-canary") { assignedTo { totalCount } } }"#,
    )
    .await;
    assert!(
        final_state["errors"].is_null(),
        "unexpected final state errors: {final_state}"
    );
    assert!(final_state["data"]["task"].is_null());
    assert_eq!(final_state["data"]["user"]["assignedTo"]["totalCount"], 0);

    websocket
        .send_json(&json!({"id": "consumer-sub", "type": "complete"}))
        .await;
    websocket.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_bulk_transaction_replay_preserves_postconditions() {
    let (server, _broker) = memory_server_with_broker();
    seed_consumer_collections(&server).await;

    let seed = gql(
        &server,
        r#"mutation {
            createUser(id: "bulk-user", input: { name: "Bulk Owner", status: "active" }) { id }
            existing: createTask(id: "bulk-existing", input: { title: "Before bulk", status: "open", hours: 1 }) { id version }
            target: createTask(id: "bulk-target", input: { title: "Linked target", status: "open", hours: 4 }) { id version }
        }"#,
    )
    .await;
    assert!(seed["errors"].is_null(), "unexpected seed errors: {seed}");

    let bulk = gql(
        &server,
        r#"mutation {
            commitTransaction(input: {
                idempotencyKey: "consumer-bulk-1"
                operations: [
                    { createEntity: {
                        collection: "task"
                        id: "bulk-new"
                        data: { title: "Created in bulk", status: "open", hours: 3 }
                    }}
                    { updateEntity: {
                        collection: "task"
                        id: "bulk-existing"
                        expectedVersion: 1
                        data: { title: "Updated in bulk", status: "review", hours: 2 }
                    }}
                    { createLink: {
                        sourceCollection: "user"
                        sourceId: "bulk-user"
                        targetCollection: "task"
                        targetId: "bulk-target"
                        linkType: "assigned-to"
                        metadata: { role: "bulk-owner" }
                    }}
                ]
            }) {
                transactionId
                replayHit
                results { index success collection id entity { id version data } }
            }
        }"#,
    )
    .await;
    assert!(bulk["errors"].is_null(), "unexpected bulk errors: {bulk}");
    let tx_id = bulk["data"]["commitTransaction"]["transactionId"]
        .as_str()
        .expect("transaction id");
    assert!(!tx_id.is_empty());
    assert_eq!(bulk["data"]["commitTransaction"]["replayHit"], false);
    assert_eq!(
        bulk["data"]["commitTransaction"]["results"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let replay = gql(
        &server,
        r#"mutation {
            commitTransaction(input: {
                idempotencyKey: "consumer-bulk-1"
                operations: [
                    { createEntity: {
                        collection: "task"
                        id: "bulk-new"
                        data: { title: "Different payload", status: "done", hours: 99 }
                    }}
                ]
            }) {
                transactionId
                replayHit
                results { entity { id data } }
            }
        }"#,
    )
    .await;
    assert!(
        replay["errors"].is_null(),
        "unexpected replay errors: {replay}"
    );
    assert_eq!(replay["data"]["commitTransaction"]["transactionId"], tx_id);
    assert_eq!(replay["data"]["commitTransaction"]["replayHit"], true);

    let state = gql(
        &server,
        r#"{
            created: task(id: "bulk-new") { id version title status hours }
            updated: task(id: "bulk-existing") { id version title status hours }
            owner: user(id: "bulk-user") {
                assignedTo { totalCount edges { metadata node { id title } } }
            }
            auditLog(collection: "task") {
                edges { node { mutation entityId transactionId version } }
            }
        }"#,
    )
    .await;
    assert!(
        state["errors"].is_null(),
        "unexpected state errors: {state}"
    );
    assert_eq!(state["data"]["created"]["title"], "Created in bulk");
    assert_eq!(state["data"]["updated"]["version"], 2);
    assert_eq!(
        state["data"]["owner"]["assignedTo"]["edges"][0]["metadata"]["role"],
        "bulk-owner"
    );
    let audit_edges = state["data"]["auditLog"]["edges"].as_array().unwrap();
    assert!(audit_edges.iter().any(|edge| {
        edge["node"]["entityId"] == "bulk-new" && edge["node"]["transactionId"] == tx_id
    }));
    assert!(audit_edges.iter().any(|edge| {
        edge["node"]["entityId"] == "bulk-existing" && edge["node"]["transactionId"] == tx_id
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_error_rbac_and_cors_matrix() {
    let read_only = memory_server_with_auth(AuthContext::guest(Role::Read), CorsStore::default());
    let denied = gql(
        &read_only,
        r#"mutation { createCollection(input: { name: "blocked", schema: { version: 1 } }) { name } }"#,
    )
    .await;
    assert_eq!(
        denied["errors"][0]["extensions"]["code"], "INVALID_OPERATION",
        "read role should not perform admin GraphQL mutations: {denied}"
    );

    let (server, _broker) = memory_server_with_broker();
    seed_consumer_collections(&server).await;
    let invalid = gql(
        &server,
        r#"mutation { createTask(id: "bad-schema", input: { title: "no" }) { id } }"#,
    )
    .await;
    assert_eq!(
        invalid["errors"][0]["extensions"]["code"], "SCHEMA_VALIDATION",
        "schema validation should expose field details: {invalid}"
    );
    assert!(invalid["errors"][0]["extensions"]["fieldErrors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty()));

    let create = gql(
        &server,
        r#"mutation { createTask(id: "conflict-task", input: { title: "Conflict task", status: "open", hours: 1 }) { id } }"#,
    )
    .await;
    assert!(
        create["errors"].is_null(),
        "unexpected create errors: {create}"
    );
    let conflict = gql(
        &server,
        r#"mutation {
            updateTask(
                id: "conflict-task"
                version: 99
                input: { title: "Conflict task", status: "review", hours: 1 }
            ) { id }
        }"#,
    )
    .await;
    assert_eq!(
        conflict["errors"][0]["extensions"]["code"],
        "VERSION_CONFLICT"
    );
    assert_eq!(
        conflict["errors"][0]["extensions"]["currentEntity"]["id"],
        "conflict-task"
    );

    let invalid_transition = gql(
        &server,
        r#"mutation {
            transitionTaskLifecycle(
                id: "conflict-task"
                lifecycleName: "status"
                targetState: "done"
                expectedVersion: 1
            ) { id }
        }"#,
    )
    .await;
    assert_eq!(
        invalid_transition["errors"][0]["extensions"]["code"],
        "INVALID_TRANSITION"
    );
    assert_eq!(
        invalid_transition["errors"][0]["extensions"]["validTransitions"],
        json!(["review"])
    );

    let unsupported = gql(
        &server,
        r#"{ auditLog(metadataPath: "kind", metadataEq: "manual") { totalCount } }"#,
    )
    .await;
    assert_eq!(
        unsupported["errors"][0]["extensions"]["code"],
        "UNSUPPORTED_AUDIT_FILTER"
    );

    let cors = CorsStore::default();
    cors.add_cached("https://ui.example.test");
    let cors_server = memory_server_with_auth(AuthContext::no_auth(), cors);
    let preflight = cors_server
        .method(
            Method::OPTIONS,
            "/tenants/default/databases/default/graphql",
        )
        .add_header(
            header::ORIGIN,
            HeaderValue::from_static("https://ui.example.test"),
        )
        .add_header(
            HeaderName::from_static("access-control-request-method"),
            HeaderValue::from_static("POST"),
        )
        .await;
    preflight.assert_status_ok();
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("https://ui.example.test")
    );
    let allow_headers = preflight
        .headers()
        .get("access-control-allow-headers")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(allow_headers.contains("content-type"));
    assert!(allow_headers.contains("x-axon-actor"));
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_tenant_database_isolation_matrix() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tenant_router = Arc::new(TenantRouter::new(
        tmp.path().to_path_buf(),
        memory_handler(),
    ));
    let app = build_router_with_broker(tenant_router, "memory", None, BroadcastBroker::default());
    let server = axum_test::TestServer::new(app);

    for tenant in ["tenant-a", "tenant-b"] {
        server
            .post(&format!(
                "/tenants/{tenant}/databases/default/collections/task"
            ))
            .json(&json!({
                "schema": {
                    "version": 1,
                    "entity_schema": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "tenant": { "type": "string" }
                        }
                    }
                },
                "actor": "setup"
            }))
            .await
            .assert_status(axum::http::StatusCode::CREATED);
    }

    let create_a = gql_at(
        &server,
        "tenant-a",
        "default",
        r#"mutation { createTask(id: "shared-id", input: { title: "A only", tenant: "a" }) { id tenant } }"#,
    )
    .await;
    assert!(
        create_a["errors"].is_null(),
        "unexpected tenant A errors: {create_a}"
    );
    let create_b = gql_at(
        &server,
        "tenant-b",
        "default",
        r#"mutation { createTask(id: "shared-id", input: { title: "B only", tenant: "b" }) { id tenant } }"#,
    )
    .await;
    assert!(
        create_b["errors"].is_null(),
        "unexpected tenant B errors: {create_b}"
    );

    let read_a = gql_at(
        &server,
        "tenant-a",
        "default",
        r#"{ task(id: "shared-id") { id title tenant } entities(collection: "task") { totalCount edges { node { id data } } } }"#,
    )
    .await;
    let read_b = gql_at(
        &server,
        "tenant-b",
        "default",
        r#"{ task(id: "shared-id") { id title tenant } entities(collection: "task") { totalCount edges { node { id data } } } }"#,
    )
    .await;
    assert!(
        read_a["errors"].is_null(),
        "unexpected read A errors: {read_a}"
    );
    assert!(
        read_b["errors"].is_null(),
        "unexpected read B errors: {read_b}"
    );
    assert_eq!(read_a["data"]["task"]["title"], "A only");
    assert_eq!(read_b["data"]["task"]["title"], "B only");
    assert_eq!(read_a["data"]["entities"]["totalCount"], 1);
    assert_eq!(read_b["data"]["entities"]["totalCount"], 1);

    let tenant_b_audit = gql_at(
        &server,
        "tenant-b",
        "default",
        r#"{ auditLog(collection: "task") { edges { node { entityId dataAfter } } } }"#,
    )
    .await;
    assert!(
        tenant_b_audit["errors"].is_null(),
        "unexpected audit errors: {tenant_b_audit}"
    );
    let audit_edges = tenant_b_audit["data"]["auditLog"]["edges"]
        .as_array()
        .unwrap();
    assert_eq!(
        audit_edges.len(),
        2,
        "collection create + entity create should be visible"
    );
    assert!(audit_edges.iter().all(|edge| {
        edge["node"]["dataAfter"].is_null() || edge["node"]["dataAfter"]["title"] == "B only"
    }));
}
