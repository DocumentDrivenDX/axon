//! L6 API contract tests: gRPC client tests and HTTP/gRPC parity verification.
//!
//! These tests spin up both a gRPC server (on a random port) and an HTTP test
//! server, then verify that:
//! 1. All gRPC RPCs match protobuf contract expectations.
//! 2. HTTP gateway returns identical results to gRPC for the same operations.

#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::service::{AxonServiceImpl, AxonServiceServer};
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;
use axon_storage::SqliteStorageAdapter;

// Use the proto types from the server crate.
use axon_server::service::proto;
use proto::axon_service_client::AxonServiceClient;

/// Start a gRPC server on a random port and return the address.
async fn start_grpc_server() -> (SocketAddr, Arc<Mutex<AxonHandler<MemoryStorageAdapter>>>) {
    let handler = Arc::new(Mutex::new(
        AxonHandler::new(MemoryStorageAdapter::default()),
    ));
    let svc = AxonServiceImpl::from_handler(AxonHandler::new(MemoryStorageAdapter::default()));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(AxonServiceServer::new(svc))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    // Give the server a moment to start.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, handler)
}

async fn grpc_client(addr: SocketAddr) -> AxonServiceClient<tonic::transport::Channel> {
    AxonServiceClient::connect(format!("http://{addr}"))
        .await
        .unwrap()
}

// ── gRPC Contract Tests ──────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn grpc_create_then_get_entity() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    // Create entity.
    let resp = client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"hello"}"#.into(),
            actor: "test".into(),
        })
        .await
        .unwrap();

    let entity = resp.into_inner().entity.unwrap();
    assert_eq!(entity.collection, "tasks");
    assert_eq!(entity.id, "t-001");
    assert_eq!(entity.version, 1);

    // Get entity.
    let resp = client
        .get_entity(proto::GetEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
        })
        .await
        .unwrap();

    let entity = resp.into_inner().entity.unwrap();
    assert_eq!(entity.id, "t-001");
    let data: Value = serde_json::from_str(&entity.data_json).unwrap();
    assert_eq!(data["title"], "hello");
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_get_missing_returns_not_found() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    let err = client
        .get_entity(proto::GetEntityRequest {
            collection: "tasks".into(),
            id: "ghost".into(),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_update_entity() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"v1"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let resp = client
        .update_entity(proto::UpdateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"v2"}"#.into(),
            expected_version: 1,
            actor: String::new(),
        })
        .await
        .unwrap();

    let entity = resp.into_inner().entity.unwrap();
    assert_eq!(entity.version, 2);
    let data: Value = serde_json::from_str(&entity.data_json).unwrap();
    assert_eq!(data["title"], "v2");
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_update_version_conflict() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"v1"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let err = client
        .update_entity(proto::UpdateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"v2"}"#.into(),
            expected_version: 99,
            actor: String::new(),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(err.message().contains("version_conflict"));
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_delete_entity() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"bye"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let resp = client
        .delete_entity(proto::DeleteEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let inner = resp.into_inner();
    assert_eq!(inner.collection, "tasks");
    assert_eq!(inner.id, "t-001");

    // Verify entity is gone.
    let err = client
        .get_entity(proto::GetEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_create_link_and_traverse() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    // Create two entities.
    client
        .create_entity(proto::CreateEntityRequest {
            collection: "users".into(),
            id: "u-001".into(),
            data_json: r#"{"name":"Alice"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();
    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"Task 1"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    // Create link.
    let resp = client
        .create_link(proto::CreateLinkRequest {
            source_collection: "users".into(),
            source_id: "u-001".into(),
            target_collection: "tasks".into(),
            target_id: "t-001".into(),
            link_type: "owns".into(),
            metadata_json: String::new(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let link = resp.into_inner().link.unwrap();
    assert_eq!(link.link_type, "owns");
    assert_eq!(link.source_id, "u-001");
    assert_eq!(link.target_id, "t-001");

    // Traverse.
    let resp = client
        .traverse(proto::TraverseRequest {
            collection: "users".into(),
            id: "u-001".into(),
            link_type: "owns".into(),
            max_depth: 1,
        })
        .await
        .unwrap();

    let entities = resp.into_inner().entities;
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].id, "t-001");
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_delete_link() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "users".into(),
            id: "u-001".into(),
            data_json: r#"{"name":"Alice"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();
    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"Task 1"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();
    client
        .create_link(proto::CreateLinkRequest {
            source_collection: "users".into(),
            source_id: "u-001".into(),
            target_collection: "tasks".into(),
            target_id: "t-001".into(),
            link_type: "owns".into(),
            metadata_json: String::new(),
            actor: String::new(),
        })
        .await
        .unwrap();

    // Delete link.
    let resp = client
        .delete_link(proto::DeleteLinkRequest {
            source_collection: "users".into(),
            source_id: "u-001".into(),
            target_collection: "tasks".into(),
            target_id: "t-001".into(),
            link_type: "owns".into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let inner = resp.into_inner();
    assert_eq!(inner.link_type, "owns");

    // Traverse should return empty.
    let resp = client
        .traverse(proto::TraverseRequest {
            collection: "users".into(),
            id: "u-001".into(),
            link_type: "owns".into(),
            max_depth: 1,
        })
        .await
        .unwrap();

    assert!(resp.into_inner().entities.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_query_audit_by_entity() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"hello"}"#.into(),
            actor: "agent-1".into(),
        })
        .await
        .unwrap();

    let resp = client
        .query_audit_by_entity(proto::QueryAuditByEntityRequest {
            collection: "tasks".into(),
            entity_id: "t-001".into(),
        })
        .await
        .unwrap();

    let entries = resp.into_inner().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor, "anonymous");
    assert_eq!(entries[0].mutation, "EntityCreate");
}

// ── HTTP/gRPC Parity Tests ──────────────────────────────────────────────────

/// Create both an HTTP test server and a gRPC server sharing the same handler,
/// then verify that the same operations produce matching results.
///
/// Since axum_test and tonic test servers don't share state easily, we test
/// parity by running the same sequence independently and comparing outputs.

#[tokio::test(flavor = "multi_thread")]
async fn parity_create_get_entity() {
    // HTTP
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    let http_create = http
        .post("/tenants/default/databases/default/entities/tasks/t-001")
        .json(&json!({"data": {"title": "hello"}, "actor": "test"}))
        .await;
    http_create.assert_status(axum::http::StatusCode::CREATED);
    let http_body: Value = http_create.json();

    let http_get = http
        .get("/tenants/default/databases/default/entities/tasks/t-001")
        .await;
    http_get.assert_status_ok();
    let http_get_body: Value = http_get.json();

    // gRPC
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    let grpc_create = client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"hello"}"#.into(),
            actor: "test".into(),
        })
        .await
        .unwrap();
    let grpc_entity = grpc_create.into_inner().entity.unwrap();

    let grpc_get = client
        .get_entity(proto::GetEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
        })
        .await
        .unwrap();
    let grpc_get_entity = grpc_get.into_inner().entity.unwrap();

    // Parity checks: same entity data from both transports.
    assert_eq!(http_body["entity"]["collection"], grpc_entity.collection);
    assert_eq!(http_body["entity"]["id"], grpc_entity.id);
    assert_eq!(http_body["entity"]["version"], grpc_entity.version);

    assert_eq!(
        http_get_body["entity"]["collection"],
        grpc_get_entity.collection
    );
    assert_eq!(http_get_body["entity"]["id"], grpc_get_entity.id);
    assert_eq!(http_get_body["entity"]["version"], grpc_get_entity.version);

    // Data content parity.
    let http_data = &http_get_body["entity"]["data"];
    let grpc_data: Value = serde_json::from_str(&grpc_get_entity.data_json).unwrap();
    assert_eq!(http_data, &grpc_data);
}

#[tokio::test(flavor = "multi_thread")]
async fn parity_update_entity() {
    // HTTP
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    http.post("/tenants/default/databases/default/entities/tasks/t-001")
        .json(&json!({"data": {"title": "v1"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let http_update = http
        .put("/tenants/default/databases/default/entities/tasks/t-001")
        .json(&json!({"data": {"title": "v2"}, "expected_version": 1}))
        .await;
    http_update.assert_status_ok();
    let http_body: Value = http_update.json();

    // gRPC
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"v1"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let grpc_update = client
        .update_entity(proto::UpdateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"v2"}"#.into(),
            expected_version: 1,
            actor: String::new(),
        })
        .await
        .unwrap();
    let grpc_entity = grpc_update.into_inner().entity.unwrap();

    // Parity.
    assert_eq!(http_body["entity"]["version"], grpc_entity.version);
    let http_data = &http_body["entity"]["data"];
    let grpc_data: Value = serde_json::from_str(&grpc_entity.data_json).unwrap();
    assert_eq!(http_data, &grpc_data);
}

#[tokio::test(flavor = "multi_thread")]
async fn parity_link_traverse() {
    // HTTP
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    http.post("/tenants/default/databases/default/entities/users/u-001")
        .json(&json!({"data": {"name": "Alice"}}))
        .await;
    http.post("/tenants/default/databases/default/entities/tasks/t-001")
        .json(&json!({"data": {"title": "Task 1"}}))
        .await;
    http.post("/tenants/default/databases/default/links")
        .json(&json!({
            "source_collection": "users",
            "source_id": "u-001",
            "target_collection": "tasks",
            "target_id": "t-001",
            "link_type": "owns"
        }))
        .await;

    let http_traverse = http
        .get("/tenants/default/databases/default/traverse/users/u-001?link_type=owns")
        .await;
    http_traverse.assert_status_ok();
    let http_body: Value = http_traverse.json();

    // gRPC
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "users".into(),
            id: "u-001".into(),
            data_json: r#"{"name":"Alice"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();
    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: r#"{"title":"Task 1"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();
    client
        .create_link(proto::CreateLinkRequest {
            source_collection: "users".into(),
            source_id: "u-001".into(),
            target_collection: "tasks".into(),
            target_id: "t-001".into(),
            link_type: "owns".into(),
            metadata_json: String::new(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let grpc_traverse = client
        .traverse(proto::TraverseRequest {
            collection: "users".into(),
            id: "u-001".into(),
            link_type: "owns".into(),
            max_depth: 1,
        })
        .await
        .unwrap();

    let grpc_entities = grpc_traverse.into_inner().entities;
    let http_entities = http_body["entities"].as_array().unwrap();

    // Parity: same number of entities, same IDs.
    assert_eq!(http_entities.len(), grpc_entities.len());
    assert_eq!(http_entities[0]["id"], grpc_entities[0].id);

    // Data parity.
    let http_data = &http_entities[0]["data"];
    let grpc_data: Value = serde_json::from_str(&grpc_entities[0].data_json).unwrap();
    assert_eq!(http_data, &grpc_data);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_commit_transaction_atomic() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    // Seed an entity to delete in the transaction.
    client
        .create_entity(proto::CreateEntityRequest {
            collection: "items".into(),
            id: "del-me".into(),
            data_json: r#"{"x":1}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    // Commit a transaction: create one entity, delete another.
    let resp = client
        .commit_transaction(proto::CommitTransactionRequest {
            operations: vec![
                proto::TransactionOp {
                    op: "create".into(),
                    collection: "items".into(),
                    id: "new-one".into(),
                    data_json: r#"{"y":2}"#.into(),
                    expected_version: 0,
                },
                proto::TransactionOp {
                    op: "delete".into(),
                    collection: "items".into(),
                    id: "del-me".into(),
                    data_json: String::new(),
                    expected_version: 1,
                },
            ],
            actor: "tx-agent".into(),
        })
        .await
        .unwrap();

    let inner = resp.into_inner();
    assert!(!inner.transaction_id.is_empty());
    assert_eq!(inner.entities.len(), 2);

    // Verify: new-one exists.
    let resp = client
        .get_entity(proto::GetEntityRequest {
            collection: "items".into(),
            id: "new-one".into(),
        })
        .await
        .unwrap();
    assert_eq!(resp.into_inner().entity.unwrap().id, "new-one");

    // Verify: del-me is gone.
    let err = client
        .get_entity(proto::GetEntityRequest {
            collection: "items".into(),
            id: "del-me".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

// ── New RPC contract tests ────────────────────────────────────────────────────

/// Minimal schema JSON for tests.
fn minimal_schema_json(collection: &str) -> String {
    serde_json::json!({
        "collection": collection,
        "version": 1
    })
    .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_query_entities_basic() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    for i in 1..=3_u32 {
        client
            .create_entity(proto::CreateEntityRequest {
                collection: "items".into(),
                id: format!("item-{i:03}"),
                data_json: serde_json::json!({"n": i}).to_string(),
                actor: String::new(),
            })
            .await
            .unwrap();
    }

    let resp = client
        .query_entities(proto::QueryEntitiesRequest {
            collection: "items".into(),
            filter_json: String::new(),
            limit: 0,
            after_id: String::new(),
        })
        .await
        .unwrap();
    let inner = resp.into_inner();
    assert_eq!(inner.entities.len(), 3);
    assert_eq!(inner.total_count, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_query_entities_with_filter() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    for i in 1..=5_u32 {
        client
            .create_entity(proto::CreateEntityRequest {
                collection: "things".into(),
                id: format!("t-{i:03}"),
                data_json: serde_json::json!({"v": i}).to_string(),
                actor: String::new(),
            })
            .await
            .unwrap();
    }

    let filter = serde_json::json!({
        "type": "field",
        "field": "v",
        "op": "gte",
        "value": 3
    });
    let resp = client
        .query_entities(proto::QueryEntitiesRequest {
            collection: "things".into(),
            filter_json: filter.to_string(),
            limit: 0,
            after_id: String::new(),
        })
        .await
        .unwrap();
    let inner = resp.into_inner();
    assert_eq!(inner.entities.len(), 3, "expected v=3,4,5");
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_query_entities_with_pagination() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    for i in 1..=5_u32 {
        client
            .create_entity(proto::CreateEntityRequest {
                collection: "pages".into(),
                id: format!("p-{i:03}"),
                data_json: serde_json::json!({"i": i}).to_string(),
                actor: String::new(),
            })
            .await
            .unwrap();
    }

    let resp = client
        .query_entities(proto::QueryEntitiesRequest {
            collection: "pages".into(),
            filter_json: String::new(),
            limit: 2,
            after_id: String::new(),
        })
        .await
        .unwrap();
    let inner = resp.into_inner();
    assert_eq!(inner.entities.len(), 2);
    assert!(!inner.next_cursor.is_empty(), "cursor should be set");

    let resp = client
        .query_entities(proto::QueryEntitiesRequest {
            collection: "pages".into(),
            filter_json: String::new(),
            limit: 2,
            after_id: inner.next_cursor.clone(),
        })
        .await
        .unwrap();
    let inner2 = resp.into_inner();
    assert_eq!(inner2.entities.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_put_and_get_schema() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    let schema_json = serde_json::json!({
        "collection": "docs",
        "version": 1,
        "entity_schema": {
            "type": "object",
            "properties": {
                "title": {"type": "string"}
            }
        }
    })
    .to_string();

    let resp = client
        .put_schema(proto::PutSchemaRequest {
            schema_json: schema_json.clone(),
            actor: "admin".into(),
            force: false,
            dry_run: false,
        })
        .await
        .unwrap();
    let inner = resp.into_inner();
    assert!(!inner.schema_json.is_empty());
    assert!(!inner.dry_run);

    let resp = client
        .get_schema(proto::GetSchemaRequest {
            collection: "docs".into(),
        })
        .await
        .unwrap();
    let returned: Value = serde_json::from_str(&resp.into_inner().schema_json).unwrap();
    assert_eq!(returned["collection"], "docs");
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_get_schema_not_found() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    let err = client
        .get_schema(proto::GetSchemaRequest {
            collection: "nonexistent".into(),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_put_schema_dry_run() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    let resp = client
        .put_schema(proto::PutSchemaRequest {
            schema_json: minimal_schema_json("myc"),
            actor: String::new(),
            force: false,
            dry_run: true,
        })
        .await
        .unwrap();
    assert!(
        resp.into_inner().dry_run,
        "dry_run flag should be reflected"
    );

    let err = client
        .get_schema(proto::GetSchemaRequest {
            collection: "myc".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_collection_lifecycle() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    let schema_json = serde_json::json!({
        "collection": "books",
        "version": 1
    })
    .to_string();

    // Create.
    let resp = client
        .create_collection(proto::CreateCollectionRequest {
            name: "books".into(),
            schema_json: schema_json.clone(),
            actor: "admin".into(),
        })
        .await
        .unwrap();
    assert_eq!(resp.into_inner().name, "books");

    // Add an entity.
    client
        .create_entity(proto::CreateEntityRequest {
            collection: "books".into(),
            id: "b-001".into(),
            data_json: r#"{"title":"Rust Programming"}"#.into(),
            actor: String::new(),
        })
        .await
        .unwrap();

    // Describe.
    let resp = client
        .describe_collection(proto::DescribeCollectionRequest {
            name: "books".into(),
        })
        .await
        .unwrap();
    let inner = resp.into_inner();
    assert_eq!(inner.name, "books");
    assert_eq!(inner.entity_count, 1);
    assert!(!inner.schema_json.is_empty());

    // List.
    let resp = client
        .list_collections(proto::ListCollectionsRequest {})
        .await
        .unwrap();
    let inner = resp.into_inner();
    assert!(
        inner.collections.iter().any(|c| c.name == "books"),
        "books should be in list"
    );

    // Drop.
    let resp = client
        .drop_collection(proto::DropCollectionRequest {
            name: "books".into(),
            actor: "admin".into(),
            confirm: true,
        })
        .await
        .unwrap();
    let inner = resp.into_inner();
    assert_eq!(inner.name, "books");
    assert_eq!(inner.entities_removed, 1);

    // describe should now return not-found.
    let err = client
        .describe_collection(proto::DescribeCollectionRequest {
            name: "books".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_drop_collection_requires_confirm() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_collection(proto::CreateCollectionRequest {
            name: "tmp".into(),
            schema_json: minimal_schema_json("tmp"),
            actor: String::new(),
        })
        .await
        .unwrap();

    let err = client
        .drop_collection(proto::DropCollectionRequest {
            name: "tmp".into(),
            actor: String::new(),
            confirm: false,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

/// HTTP contract test: `direction=reverse` on the traverse endpoint.
///
/// Graph: A --owns--> B
///
/// Forward traversal from A should return B.
/// Reverse traversal from B should return A (A links TO B).
#[tokio::test(flavor = "multi_thread")]
async fn http_traverse_direction_reverse() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    // Create entities A and B.
    http.post("/tenants/default/databases/default/entities/nodes/a")
        .json(&json!({"data": {"name": "A"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.post("/tenants/default/databases/default/entities/nodes/b")
        .json(&json!({"data": {"name": "B"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Create forward link: A -> B.
    http.post("/tenants/default/databases/default/links")
        .json(&json!({
            "source_collection": "nodes",
            "source_id": "a",
            "target_collection": "nodes",
            "target_id": "b",
            "link_type": "owns"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Forward traversal from A should return B.
    let fwd = http
        .get("/tenants/default/databases/default/traverse/nodes/a?link_type=owns&direction=forward")
        .await;
    fwd.assert_status_ok();
    let fwd_body: Value = fwd.json();
    let fwd_entities = fwd_body["entities"].as_array().unwrap();
    assert_eq!(
        fwd_entities.len(),
        1,
        "forward from A should return 1 entity"
    );
    assert_eq!(fwd_entities[0]["id"], "b", "forward from A should return B");

    // Reverse traversal from B should return A (A links TO B).
    let rev = http
        .get("/tenants/default/databases/default/traverse/nodes/b?link_type=owns&direction=reverse")
        .await;
    rev.assert_status_ok();
    let rev_body: Value = rev.json();
    let rev_entities = rev_body["entities"].as_array().unwrap();
    assert_eq!(
        rev_entities.len(),
        1,
        "reverse from B should return 1 entity"
    );
    assert_eq!(rev_entities[0]["id"], "a", "reverse from B should return A");
}

#[tokio::test(flavor = "multi_thread")]
async fn http_traverse_post_accepts_hop_filter_and_returns_link_metadata() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    for (id, status) in [("a", "root"), ("b", "active"), ("c", "archived")] {
        http.post(&format!(
            "/tenants/default/databases/default/entities/nodes/{id}"
        ))
        .json(&json!({"data": {"name": id, "status": status}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    }

    for (target, weight) in [("b", 1), ("c", 2)] {
        http.post("/tenants/default/databases/default/links")
            .json(&json!({
                "source_collection": "nodes",
                "source_id": "a",
                "target_collection": "nodes",
                "target_id": target,
                "link_type": "owns",
                "metadata": {"weight": weight}
            }))
            .await
            .assert_status(axum::http::StatusCode::CREATED);
    }

    let resp = http
        .post("/tenants/default/databases/default/traverse/nodes/a")
        .json(&json!({
            "link_type": "owns",
            "max_depth": 1,
            "hop_filter": {
                "type": "field",
                "field": "status",
                "op": "eq",
                "value": "active"
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();

    let entities = body["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 1, "hop filter should exclude archived node");
    assert_eq!(entities[0]["id"], "b");

    let paths = body["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0]["target_id"], "b");
    assert_eq!(paths[0]["metadata"]["weight"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_schema_manifest_supports_static_client_handshake() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    http.post("/tenants/default/databases/default/collections/time_entries")
        .json(&json!({
            "schema": {
                "description": "Time entry records used by browser clients",
                "version": 7,
                "entity_schema": {
                    "type": "object",
                    "description": "A time entry payload",
                    "properties": {
                        "status": {
                            "type": "string",
                            "description": "Approval status label"
                        },
                        "hours": {
                            "type": "number",
                            "description": "Billable hours for the entry"
                        }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let ok = http.get("/tenants/default/databases/default/schema").await;
    ok.assert_status_ok();
    let body: Value = ok.json();
    let schema_hash = body["schema_hash"]
        .as_str()
        .expect("schema manifest exposes schema_hash");
    assert!(
        schema_hash.starts_with("fnv64:"),
        "schema_hash should be stable and named: {schema_hash}"
    );
    assert_eq!(body["database"], "default");
    assert_eq!(body["expected_header"], "x-axon-schema-hash");
    assert_eq!(body["collections"][0]["name"], "time_entries");
    assert_eq!(body["collections"][0]["version"], 1);
    assert_eq!(
        body["collections"][0]["schema"]["description"],
        "Time entry records used by browser clients"
    );
    assert_eq!(
        body["collections"][0]["schema"]["entity_schema"]["description"],
        "A time entry payload"
    );
    assert_eq!(
        body["collections"][0]["schema"]["entity_schema"]["properties"]["status"]["description"],
        "Approval status label"
    );
    assert_eq!(
        body["collections"][0]["schema"]["entity_schema"]["properties"]["hours"]["description"],
        "Billable hours for the entry"
    );

    let matched = http
        .get("/tenants/default/databases/default/schema")
        .add_header("x-axon-schema-hash", schema_hash)
        .await;
    matched.assert_status_ok();

    let mismatch = http
        .get("/tenants/default/databases/default/schema")
        .add_header("x-axon-schema-hash", "fnv64:stale")
        .await;
    mismatch.assert_status(axum::http::StatusCode::CONFLICT);
    let mismatch_body: Value = mismatch.json();
    assert_eq!(mismatch_body["code"], "schema_mismatch");
    assert_eq!(mismatch_body["detail"]["expected"], "fnv64:stale");
    assert_eq!(mismatch_body["detail"]["actual"], schema_hash);
}

#[tokio::test(flavor = "multi_thread")]
async fn http_dev_database_reset_requires_force_and_allows_reseed() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    http.post("/databases/dev")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.post("/tenants/default/databases/dev/collections/tasks")
        .json(&json!({
            "schema": {
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"}
                    }
                }
            }
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.post("/tenants/default/databases/dev/entities/tasks/t-001")
        .json(&json!({"data": {"title": "before reset"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let guarded = http.delete("/databases/dev").await;
    guarded.assert_status(axum::http::StatusCode::BAD_REQUEST);
    let guarded_body: Value = guarded.json();
    assert_eq!(guarded_body["code"], "invalid_operation");
    assert!(guarded_body["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("Use force=true to drop"));

    let reset = http.delete("/databases/dev?force=true").await;
    reset.assert_status_ok();
    let reset_body: Value = reset.json();
    assert_eq!(reset_body["name"], "dev");
    assert_eq!(reset_body["collections_removed"], 1);

    http.post("/databases/dev")
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.post("/tenants/default/databases/dev/collections/tasks")
        .json(&json!({
            "schema": {
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"}
                    }
                }
            }
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.post("/tenants/default/databases/dev/entities/tasks/t-001")
        .json(&json!({"data": {"title": "after reset"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let reseeded = http
        .get("/tenants/default/databases/dev/entities/tasks/t-001")
        .await;
    reseeded.assert_status_ok();
    let reseeded_body: Value = reseeded.json();
    assert_eq!(reseeded_body["entity"]["data"]["title"], "after reset");
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_create_collection_duplicate_fails() {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    let schema_json = minimal_schema_json("dupes");

    client
        .create_collection(proto::CreateCollectionRequest {
            name: "dupes".into(),
            schema_json: schema_json.clone(),
            actor: String::new(),
        })
        .await
        .unwrap();

    let err = client
        .create_collection(proto::CreateCollectionRequest {
            name: "dupes".into(),
            schema_json,
            actor: String::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::AlreadyExists);
}

// ── gRPC Lifecycle Tests (FEAT-015) ─────────────────────────────────────────

/// JSON schema literal with a `status` lifecycle matching the HTTP tests:
/// `draft -> submitted -> approved`.
fn lifecycle_schema_json(collection: &str) -> String {
    serde_json::json!({
        "collection": collection,
        "version": 1,
        "lifecycles": {
            "status": {
                "field": "status",
                "initial": "draft",
                "transitions": {
                    "draft": ["submitted"],
                    "submitted": ["approved"],
                    "approved": []
                }
            }
        }
    })
    .to_string()
}

/// Provision the `tasks` collection (with the `status` lifecycle) and seed
/// entity `t-001` in `status: "draft"` on a fresh gRPC server. Returns the
/// connected client.
async fn setup_lifecycle_client(
) -> proto::axon_service_client::AxonServiceClient<tonic::transport::Channel> {
    let (addr, _) = start_grpc_server().await;
    let mut client = grpc_client(addr).await;

    client
        .create_collection(proto::CreateCollectionRequest {
            name: "tasks".into(),
            schema_json: lifecycle_schema_json("tasks"),
            actor: "test-setup".into(),
        })
        .await
        .unwrap();

    client
        .create_entity(proto::CreateEntityRequest {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: serde_json::json!({
                "status": "draft",
                "title": "design the thing"
            })
            .to_string(),
            actor: "test-setup".into(),
        })
        .await
        .unwrap();

    client
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_transition_lifecycle_happy_path() {
    let mut client = setup_lifecycle_client().await;

    let resp = client
        .transition_lifecycle(proto::TransitionLifecycleRequest {
            collection: "tasks".into(),
            entity_id: "t-001".into(),
            lifecycle_name: "status".into(),
            target_state: "submitted".into(),
            expected_version: 1,
            actor: "alice".into(),
        })
        .await
        .unwrap();

    let entity = resp.into_inner().entity.unwrap();
    assert_eq!(entity.collection, "tasks");
    assert_eq!(entity.id, "t-001");
    assert_eq!(entity.version, 2);
    let data: Value = serde_json::from_str(&entity.data_json).unwrap();
    assert_eq!(data["status"], "submitted");
    assert_eq!(data["title"], "design the thing");
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_transition_lifecycle_invalid_transition() {
    let mut client = setup_lifecycle_client().await;

    let err = client
        .transition_lifecycle(proto::TransitionLifecycleRequest {
            collection: "tasks".into(),
            entity_id: "t-001".into(),
            lifecycle_name: "status".into(),
            target_state: "approved".into(),
            expected_version: 1,
            actor: String::new(),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    let msg = err.message();
    assert!(
        msg.contains("invalid_transition"),
        "message should include invalid_transition code: {msg}"
    );
    assert!(
        msg.contains("submitted"),
        "message should include valid_transitions list: {msg}"
    );
    assert!(
        msg.contains("\"draft\""),
        "message should include current_state: {msg}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_transition_lifecycle_not_found() {
    let mut client = setup_lifecycle_client().await;

    let err = client
        .transition_lifecycle(proto::TransitionLifecycleRequest {
            collection: "tasks".into(),
            entity_id: "t-001".into(),
            lifecycle_name: "does_not_exist".into(),
            target_state: "whatever".into(),
            expected_version: 1,
            actor: String::new(),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), tonic::Code::NotFound);
    assert!(err.message().contains("does_not_exist"));
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_transition_lifecycle_version_conflict() {
    let mut client = setup_lifecycle_client().await;

    let err = client
        .transition_lifecycle(proto::TransitionLifecycleRequest {
            collection: "tasks".into(),
            entity_id: "t-001".into(),
            lifecycle_name: "status".into(),
            target_state: "submitted".into(),
            expected_version: 99,
            actor: String::new(),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
    assert!(err.message().contains("version_conflict"));
}

// ── Caller identity propagation (FEAT-012, salvage of axon-81966bf9) ────────
//
// End-to-end verification that the `x-axon-actor` header flows from the HTTP
// and gRPC transports into audit entry provenance. These tests start a real
// HTTP test server (axum_test) and a real gRPC server (tonic) and inspect
// the audit log via the public `/audit/entity/...` endpoint or the
// `query_audit_by_entity` RPC.

fn caller_identity_http_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(http_app)
}

#[tokio::test(flavor = "multi_thread")]
async fn http_caller_identity_from_header_recorded_in_audit() {
    let server = caller_identity_http_server();

    server
        .post("/tenants/default/databases/default/entities/tasks/t-001")
        .add_header("x-axon-actor", "agent-1")
        .json(&json!({"data": {"title": "hello"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .get("/tenants/default/databases/default/audit/entity/tasks/t-001")
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["actor"], "agent-1",
        "x-axon-actor header value must be recorded as the audit actor"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_missing_caller_identity_records_anonymous_in_audit() {
    let server = caller_identity_http_server();

    server
        .post("/tenants/default/databases/default/entities/tasks/t-002")
        .json(&json!({"data": {"title": "no-header"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .get("/tenants/default/databases/default/audit/entity/tasks/t-002")
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["actor"], "anonymous",
        "missing identity header falls back to the anonymous caller"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_caller_identity_body_actor_is_ignored() {
    let server = caller_identity_http_server();

    // Body carries an actor field, but no x-axon-actor header. The header is
    // the authority — body-level actor must NOT leak into the audit entry.
    server
        .post("/tenants/default/databases/default/entities/tasks/t-003")
        .json(&json!({"data": {"title": "imposter"}, "actor": "claimed-by-body"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .get("/tenants/default/databases/default/audit/entity/tasks/t-003")
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["actor"], "anonymous",
        "body-level actor must not bypass header-based identity extraction"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_caller_identity_empty_header_falls_back_to_anonymous() {
    let server = caller_identity_http_server();

    // An empty x-axon-actor header must be treated as "no header" rather
    // than recording an empty string as the actor.
    server
        .post("/tenants/default/databases/default/entities/tasks/t-006")
        .add_header("x-axon-actor", "")
        .json(&json!({"data": {"title": "empty-header"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .get("/tenants/default/databases/default/audit/entity/tasks/t-006")
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries[0]["actor"], "anonymous");
}

#[tokio::test(flavor = "multi_thread")]
async fn http_caller_identity_per_operation_type() {
    // Create, update, and delete all receive identity from the header.
    let server = caller_identity_http_server();

    server
        .post("/tenants/default/databases/default/entities/tasks/t-005")
        .add_header("x-axon-actor", "creator")
        .json(&json!({"data": {"title": "v1"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .put("/tenants/default/databases/default/entities/tasks/t-005")
        .add_header("x-axon-actor", "updater")
        .json(&json!({"data": {"title": "v2"}, "expected_version": 1}))
        .await
        .assert_status_ok();

    server
        .delete("/tenants/default/databases/default/entities/tasks/t-005")
        .add_header("x-axon-actor", "deleter")
        .await
        .assert_status_ok();

    let resp = server
        .get("/tenants/default/databases/default/audit/entity/tasks/t-005")
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    // Entries are ordered chronologically: create, update, delete.
    assert_eq!(entries[0]["actor"], "creator");
    assert_eq!(entries[1]["actor"], "updater");
    assert_eq!(entries[2]["actor"], "deleter");
}

#[tokio::test(flavor = "multi_thread")]
async fn http_policy_denials_and_audit_redaction_are_end_to_end() {
    let server = caller_identity_http_server();

    server
        .post("/tenants/default/databases/default/collections/policy_audit_docs")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "secret": { "type": "string" }
                    }
                },
                "access_control": {
                    "read": { "allow": [{ "name": "all-read" }] },
                    "create": { "allow": [{ "name": "all-create" }] },
                    "update": { "allow": [{ "name": "all-update" }] },
                    "delete": { "allow": [{ "name": "all-delete" }] },
                    "fields": {
                        "secret": {
                            "read": {
                                "deny": [{
                                    "name": "contractor-secret-redaction",
                                    "when": { "subject": "user_id", "eq": "contractor" },
                                    "redact_as": null
                                }]
                            },
                            "write": {
                                "deny": [{
                                    "name": "contractor-secret-write-denied",
                                    "when": { "subject": "user_id", "eq": "contractor" }
                                }]
                            }
                        }
                    }
                }
            },
            "actor": "schema-admin"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/tenants/default/databases/default/collections/policy_row_docs")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" }
                    }
                },
                "access_control": {
                    "read": { "allow": [{ "name": "all-read" }] },
                    "create": {
                        "allow": [{
                            "name": "operator-create",
                            "when": { "subject": "user_id", "eq": "operator" }
                        }]
                    }
                }
            },
            "actor": "schema-admin"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/tenants/default/databases/default/entities/policy_audit_docs/doc-1")
        .add_header("x-axon-actor", "operator")
        .json(&json!({
            "data": {
                "title": "seed",
                "secret": "classified"
            }
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .put("/tenants/default/databases/default/entities/policy_audit_docs/doc-1")
        .add_header("x-axon-actor", "operator")
        .json(&json!({
            "data": {
                "title": "updated",
                "secret": "changed"
            },
            "expected_version": 1
        }))
        .await
        .assert_status_ok();

    let operator_audit = server
        .get("/tenants/default/databases/default/audit/entity/policy_audit_docs/doc-1")
        .add_header("x-axon-actor", "operator")
        .await;
    operator_audit.assert_status_ok();
    let operator_audit: Value = operator_audit.json();
    let operator_entries = operator_audit["entries"].as_array().unwrap();
    assert_eq!(operator_entries.len(), 2);
    let operator_update = operator_entries
        .iter()
        .find(|entry| entry["mutation"] == "entity.update")
        .expect("operator should see the update audit entry");
    assert_eq!(operator_update["data_before"]["secret"], "classified");
    assert_eq!(operator_update["data_after"]["secret"], "changed");

    let contractor_audit = server
        .get("/tenants/default/databases/default/audit/entity/policy_audit_docs/doc-1")
        .add_header("x-axon-actor", "contractor")
        .await;
    contractor_audit.assert_status_ok();
    let contractor_audit: Value = contractor_audit.json();
    let contractor_entries = contractor_audit["entries"].as_array().unwrap();
    assert_eq!(contractor_entries.len(), 2);
    let contractor_update = contractor_entries
        .iter()
        .find(|entry| entry["mutation"] == "entity.update")
        .expect("contractor should see the redacted update audit entry");
    assert_eq!(contractor_update["data_before"]["secret"], Value::Null);
    assert_eq!(contractor_update["data_after"]["secret"], Value::Null);
    let contractor_audit_text = contractor_audit.to_string();
    assert!(!contractor_audit_text.contains("classified"));
    assert!(!contractor_audit_text.contains("changed"));

    let denied_field = server
        .put("/tenants/default/databases/default/entities/policy_audit_docs/doc-1")
        .add_header("x-axon-actor", "contractor")
        .json(&json!({
            "data": {
                "title": "bad update",
                "secret": "leaked"
            },
            "expected_version": 2
        }))
        .await;
    denied_field.assert_status(axum::http::StatusCode::FORBIDDEN);
    let denied_field_body: Value = denied_field.json();
    assert_eq!(denied_field_body["detail"]["reason"], "field_write_denied");
    assert_eq!(denied_field_body["detail"]["field_path"], "secret");

    let after_denied = server
        .get("/tenants/default/databases/default/entities/policy_audit_docs/doc-1")
        .add_header("x-axon-actor", "operator")
        .await;
    after_denied.assert_status_ok();
    let after_denied: Value = after_denied.json();
    assert_eq!(after_denied["entity"]["version"], 2);
    assert_eq!(after_denied["entity"]["data"]["title"], "updated");
    assert_eq!(after_denied["entity"]["data"]["secret"], "changed");

    let audit_after_denial = server
        .get("/tenants/default/databases/default/audit/entity/policy_audit_docs/doc-1")
        .add_header("x-axon-actor", "operator")
        .await;
    audit_after_denial.assert_status_ok();
    let audit_after_denial: Value = audit_after_denial.json();
    assert_eq!(
        audit_after_denial["entries"].as_array().unwrap().len(),
        operator_entries.len(),
        "denied field write must not append audit entries"
    );

    let denied_row = server
        .post("/tenants/default/databases/default/entities/policy_row_docs/row-denied")
        .add_header("x-axon-actor", "contractor")
        .json(&json!({
            "data": {
                "title": "blocked"
            }
        }))
        .await;
    denied_row.assert_status(axum::http::StatusCode::FORBIDDEN);
    let denied_row_body: Value = denied_row.json();
    assert_eq!(denied_row_body["detail"]["reason"], "row_write_denied");
    server
        .get("/tenants/default/databases/default/entities/policy_row_docs/row-denied")
        .add_header("x-axon-actor", "operator")
        .await
        .assert_status_not_found();
    let row_audit = server
        .get("/tenants/default/databases/default/audit/entity/policy_row_docs/row-denied")
        .add_header("x-axon-actor", "operator")
        .await;
    row_audit.assert_status_ok();
    let row_audit: Value = row_audit.json();
    assert!(row_audit["entries"].as_array().unwrap().is_empty());

    let tx_body = json!({
        "idempotency_key": "policy-denied-transaction-replay",
        "operations": [
            {
                "op": "create",
                "collection": "policy_audit_docs",
                "id": "tx-allowed",
                "data": { "title": "allowed" }
            },
            {
                "op": "create",
                "collection": "policy_audit_docs",
                "id": "tx-denied",
                "data": { "title": "denied", "secret": "classified" }
            }
        ]
    });
    let first_tx = server
        .post("/tenants/default/databases/default/transactions")
        .add_header("x-axon-actor", "contractor")
        .json(&tx_body)
        .await;
    first_tx.assert_status(axum::http::StatusCode::FORBIDDEN);
    let first_tx_body: Value = first_tx.json();
    assert_eq!(first_tx_body["detail"]["reason"], "field_write_denied");
    assert_eq!(first_tx_body["detail"]["field_path"], "secret");
    assert_eq!(first_tx_body["detail"]["operation_index"], 1);

    let replay_tx = server
        .post("/tenants/default/databases/default/transactions")
        .add_header("x-axon-actor", "contractor")
        .json(&tx_body)
        .await;
    replay_tx.assert_status(axum::http::StatusCode::FORBIDDEN);
    assert_eq!(
        replay_tx
            .headers()
            .get("x-idempotent-cache")
            .and_then(|value| value.to_str().ok()),
        Some("hit")
    );
    let replay_tx_body: Value = replay_tx.json();
    assert_eq!(replay_tx_body, first_tx_body);

    for entity_id in ["tx-allowed", "tx-denied"] {
        server
            .get(&format!(
                "/tenants/default/databases/default/entities/policy_audit_docs/{entity_id}"
            ))
            .add_header("x-axon-actor", "operator")
            .await
            .assert_status_not_found();
        let tx_audit = server
            .get(&format!(
                "/tenants/default/databases/default/audit/entity/policy_audit_docs/{entity_id}"
            ))
            .add_header("x-axon-actor", "operator")
            .await;
        tx_audit.assert_status_ok();
        let tx_audit: Value = tx_audit.json();
        assert!(
            tx_audit["entries"].as_array().unwrap().is_empty(),
            "aborted transaction must not append audit entries for {entity_id}"
        );
    }
}

// ── Idempotency tests (FEAT-008 US-081) ──────────────────────────────────────

/// AC1 + AC2: POST with `Idempotency-Key` stores the response; a retry with
/// the same key within the TTL returns the cached body without re-executing.
#[tokio::test(flavor = "multi_thread")]
async fn http_transactions_idempotent_key_caches_success() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    // Seed the entity at v1 so the transaction's update can reference a
    // concrete expected_version.
    http.post("/tenants/default/databases/default/entities/idem/e-1")
        .json(&json!({"data": {"v": 0}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let key = "idem-k-1";

    let resp1 = http
        .post("/tenants/default/databases/default/transactions")
        .add_header("idempotency-key", key)
        .json(&json!({
            "operations": [{
                "op": "update",
                "collection": "idem",
                "id": "e-1",
                "data": {"v": 1},
                "expected_version": 1
            }]
        }))
        .await;
    resp1.assert_status_ok();
    let body1: Value = resp1.json();
    assert!(body1["transaction_id"].is_string());

    // Entity is now at version 2; a fresh execution with expected_version=1
    // would fail. The idempotency cache must return the original response
    // without re-executing.
    let resp2 = http
        .post("/tenants/default/databases/default/transactions")
        .add_header("idempotency-key", key)
        .json(&json!({
            "operations": [{
                "op": "update",
                "collection": "idem",
                "id": "e-1",
                "data": {"v": 1},
                "expected_version": 1
            }]
        }))
        .await;
    resp2.assert_status_ok();
    let body2: Value = resp2.json();
    assert_eq!(body1, body2, "second response must be byte-identical");

    // Entity version must still be 2 (not re-applied to version 3).
    let get_resp = http
        .get("/tenants/default/databases/default/entities/idem/e-1")
        .await;
    get_resp.assert_status_ok();
    let get_body: Value = get_resp.json();
    assert_eq!(
        get_body["entity"]["version"], 2,
        "cached response must not re-execute the mutation"
    );
}

/// Back-compat: POST without an `Idempotency-Key` header must still execute
/// normally; the cache is not consulted.
#[tokio::test(flavor = "multi_thread")]
async fn http_transactions_without_key_executes_normally() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    let resp1 = http
        .post("/tenants/default/databases/default/transactions")
        .json(&json!({
            "operations": [{
                "op": "create",
                "collection": "noidem",
                "id": "e-1",
                "data": {"v": 1}
            }]
        }))
        .await;
    resp1.assert_status_ok();

    // Same body without a key — independent execution. Second create with
    // the same id must fail with AlreadyExists (would have been masked by a
    // cached response if the cache were active).
    let resp2 = http
        .post("/tenants/default/databases/default/transactions")
        .json(&json!({
            "operations": [{
                "op": "create",
                "collection": "noidem",
                "id": "e-1",
                "data": {"v": 1}
            }]
        }))
        .await;
    assert_eq!(
        resp2.status_code(),
        axum::http::StatusCode::CONFLICT,
        "second POST without idempotency key must re-execute and surface the conflict",
    );
}

/// AC4: a failed transaction (version conflict) must NOT be cached — a
/// retry with the same key re-executes.
#[tokio::test(flavor = "multi_thread")]
async fn http_transactions_idempotent_conflict_not_cached() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    // Seed at v1, then update to v2 out-of-band so the transaction's
    // expected_version=1 will fail deterministically.
    http.post("/tenants/default/databases/default/entities/conf/e-1")
        .json(&json!({"data": {"v": 0}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.put("/tenants/default/databases/default/entities/conf/e-1")
        .json(&json!({"data": {"v": 1}, "expected_version": 1}))
        .await
        .assert_status_ok();

    let key = "conf-k-1";
    let body = json!({
        "operations": [{
            "op": "update",
            "collection": "conf",
            "id": "e-1",
            "data": {"v": 99},
            "expected_version": 1
        }]
    });

    let resp1 = http
        .post("/tenants/default/databases/default/transactions")
        .add_header("idempotency-key", key)
        .json(&body)
        .await;
    assert_eq!(
        resp1.status_code(),
        axum::http::StatusCode::CONFLICT,
        "first attempt must produce a version conflict"
    );

    // Retry with the same key — failure was not cached, so this must also
    // re-execute and return 409 (not a cached success).
    let resp2 = http
        .post("/tenants/default/databases/default/transactions")
        .add_header("idempotency-key", key)
        .json(&body)
        .await;
    assert_eq!(
        resp2.status_code(),
        axum::http::StatusCode::CONFLICT,
        "retry after a non-cacheable failure must re-execute"
    );
    let err_body: Value = resp2.json();
    assert_eq!(err_body["code"], "version_conflict");
}

/// AC6: Idempotency keys are scoped per database. A key used in database A
/// must not short-circuit execution in database B.
#[tokio::test(flavor = "multi_thread")]
async fn http_transactions_idempotent_different_databases_isolated() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    let key = "shared-k-1";

    // POST to database "alpha" creates an entity there (path-scoped routing).
    let resp_a = http
        .post("/tenants/default/databases/alpha/transactions")
        .add_header("idempotency-key", key)
        .json(&json!({
            "operations": [{
                "op": "create",
                "collection": "iso",
                "id": "e-alpha",
                "data": {"db": "alpha"}
            }]
        }))
        .await;
    resp_a.assert_status_ok();
    let body_a: Value = resp_a.json();

    // Same key in database "beta" — must execute independently (not hit the
    // alpha cache entry).
    let resp_b = http
        .post("/tenants/default/databases/beta/transactions")
        .add_header("idempotency-key", key)
        .json(&json!({
            "operations": [{
                "op": "create",
                "collection": "iso",
                "id": "e-beta",
                "data": {"db": "beta"}
            }]
        }))
        .await;
    resp_b.assert_status_ok();
    let body_b: Value = resp_b.json();

    assert_ne!(
        body_a["transaction_id"], body_b["transaction_id"],
        "cross-database keys must not dedup — each request produces its own transaction",
    );

    // Both entities exist (TenantRouter::single stores them in the shared
    // handler; collection names are qualified per-database, so the IDs are
    // independent).
    http.get("/tenants/default/databases/alpha/entities/iso/e-alpha")
        .await
        .assert_status_ok();
    http.get("/tenants/default/databases/beta/entities/iso/e-beta")
        .await
        .assert_status_ok();
}

/// AC3: a retry after the TTL expires re-executes the transaction.
///
/// This test injects a 100 ms TTL store via [`build_router_with_idempotency`]
/// and calls `tokio::time::sleep` once to cross the boundary. A FakeClock
/// cannot be used here because the router constructs the store before any
/// requests run — once the router is built we can't advance time inside the
/// handler without threading the clock into the handler itself, and the
/// gateway's current design already keys on (`Clock::now`, TTL). A real
/// 100 ms sleep is still well below the test-suite default timeouts.
#[tokio::test(flavor = "multi_thread")]
async fn http_transactions_idempotent_expired_reexecutes() {
    use axon_core::clock::SystemClock;
    use axon_server::gateway::{build_router_with_idempotency, CachedHttpResponse};
    use axon_server::idempotency::IdempotencyStore;
    use std::time::Duration;

    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));

    let store = Arc::new(IdempotencyStore::<CachedHttpResponse>::new(
        Arc::new(SystemClock),
        Duration::from_millis(100),
    ));
    let http_app = build_router_with_idempotency(tenant_router, "memory", None, store);
    let http = axum_test::TestServer::new(http_app);

    // Seed an entity at v1 so each execution attempts a valid update.
    http.post("/tenants/default/databases/default/entities/exp/e-1")
        .json(&json!({"data": {"v": 0}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let key = "exp-k-1";

    let resp1 = http
        .post("/tenants/default/databases/default/transactions")
        .add_header("idempotency-key", key)
        .json(&json!({
            "operations": [{
                "op": "update",
                "collection": "exp",
                "id": "e-1",
                "data": {"v": 1},
                "expected_version": 1
            }]
        }))
        .await;
    resp1.assert_status_ok();

    // Wait past the 100ms TTL.
    tokio::time::sleep(Duration::from_millis(250)).await;

    // After expiry, the retry re-executes — but the entity is now at v2, so
    // the same expected_version=1 payload would conflict. Surfacing the
    // conflict proves re-execution (a cached response would have returned
    // the original 200 OK body).
    let resp2 = http
        .post("/tenants/default/databases/default/transactions")
        .add_header("idempotency-key", key)
        .json(&json!({
            "operations": [{
                "op": "update",
                "collection": "exp",
                "id": "e-1",
                "data": {"v": 1},
                "expected_version": 1
            }]
        }))
        .await;
    assert_eq!(
        resp2.status_code(),
        axum::http::StatusCode::CONFLICT,
        "post-TTL retry must re-execute and surface the real version conflict"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_transition_lifecycle_uses_metadata_actor() {
    let mut client = setup_lifecycle_client().await;

    // Send a transition with the caller identity in metadata. The proto
    // `actor` field is ignored in favor of the `x-axon-actor` metadata.
    let mut req = tonic::Request::new(proto::TransitionLifecycleRequest {
        collection: "tasks".into(),
        entity_id: "t-001".into(),
        lifecycle_name: "status".into(),
        target_state: "submitted".into(),
        expected_version: 1,
        actor: "ignored-body".into(),
    });
    req.metadata_mut()
        .insert("x-axon-actor", "grpc-agent".parse().unwrap());
    client.transition_lifecycle(req).await.unwrap();

    let resp = client
        .query_audit_by_entity(proto::QueryAuditByEntityRequest {
            collection: "tasks".into(),
            entity_id: "t-001".into(),
        })
        .await
        .unwrap();
    let entries = resp.into_inner().entries;
    // Entries include the setup create + the lifecycle transition update.
    let transition = entries
        .iter()
        .find(|e| e.mutation == "EntityUpdate")
        .expect("transition produces an EntityUpdate audit entry");
    assert_eq!(
        transition.actor, "grpc-agent",
        "gRPC x-axon-actor metadata must be recorded as the audit actor"
    );
}

/// FEAT-026 — `ChangeEvent.audit_id` must be populated from the audit append
/// result so live subscribers can use it as a resume cursor. Before this fix
/// the gateway broadcast functions hard-coded `audit_id: String::new()`.
#[tokio::test(flavor = "multi_thread")]
async fn http_create_entity_publishes_change_event_with_audit_id() {
    use axon_graphql::subscriptions::BroadcastBroker;
    use axon_server::gateway::build_router_with_broker;

    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler.clone()));

    // Inject a broker that the test can subscribe to before any writes run.
    let broker = BroadcastBroker::default();
    let mut rx = broker.subscribe();

    let http_app = build_router_with_broker(tenant_router, "memory", None, broker);
    let http = axum_test::TestServer::new(http_app);

    // POST creates the first entity in this handler — its audit id is 1.
    http.post("/tenants/default/databases/default/entities/tasks/t-001")
        .json(&json!({"data": {"title": "hello"}, "actor": "test"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("broker publishes within the timeout")
        .expect("broker channel delivers the event");

    assert_eq!(event.collection, "tasks");
    assert_eq!(event.entity_id, "t-001");
    assert_eq!(event.operation, "create");
    assert!(
        !event.audit_id.is_empty(),
        "audit_id must not be empty; got '{}'",
        event.audit_id
    );
    // The handler's audit log started empty, so the first append is id 1.
    assert_eq!(event.audit_id, "1");

    // Verify the audit_id actually resolves against the stored audit log.
    let audit_id: u64 = event.audit_id.parse().expect("audit_id parses as u64");
    let entry = {
        let h = handler.lock().await;
        use axon_audit::log::AuditLog;
        h.audit_log()
            .find_by_id(audit_id)
            .expect("audit lookup succeeds")
            .expect("audit entry exists")
    };
    assert_eq!(entry.collection.as_str(), "tasks");
    assert_eq!(entry.entity_id.as_str(), "t-001");
}

#[tokio::test(flavor = "multi_thread")]
async fn http_transaction_publishes_typed_change_events_with_before_snapshots() {
    use axon_graphql::subscriptions::{BroadcastBroker, ChangeEvent};
    use axon_server::gateway::build_router_with_broker;

    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let broker = BroadcastBroker::default();

    let http_app = build_router_with_broker(tenant_router, "memory", None, broker.clone());
    let http = axum_test::TestServer::new(http_app);

    http.post("/tenants/default/databases/default/entities/tasks/tx-update")
        .json(&json!({"data": {"title": "before-update"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.post("/tenants/default/databases/default/entities/tasks/tx-delete")
        .json(&json!({"data": {"title": "before-delete"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let mut rx = broker.subscribe();
    http.post("/tenants/default/databases/default/transactions")
        .add_header("x-axon-actor", "tx-agent")
        .json(&json!({
            "operations": [
                {
                    "op": "create",
                    "collection": "tasks",
                    "id": "tx-create",
                    "data": {"title": "created"}
                },
                {
                    "op": "update",
                    "collection": "tasks",
                    "id": "tx-update",
                    "expected_version": 1,
                    "data": {"title": "after-update"}
                },
                {
                    "op": "delete",
                    "collection": "tasks",
                    "id": "tx-delete",
                    "expected_version": 1
                }
            ]
        }))
        .await
        .assert_status(axum::http::StatusCode::OK);

    let mut events: Vec<ChangeEvent> = Vec::new();
    while events.len() < 3 {
        events.push(
            tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
                .await
                .expect("transaction broadcasts within the timeout")
                .expect("broker channel delivers the event"),
        );
    }

    let created = &events[0];
    assert_eq!(created.entity_id, "tx-create");
    assert_eq!(created.operation, "create");
    assert_eq!(created.data.as_ref().unwrap()["title"], "created");
    assert!(created.previous_data.is_none());
    assert_eq!(created.previous_version, None);
    assert_eq!(created.actor, "tx-agent");
    assert!(!created.audit_id.is_empty());

    let updated = &events[1];
    assert_eq!(updated.entity_id, "tx-update");
    assert_eq!(updated.operation, "update");
    assert_eq!(updated.data.as_ref().unwrap()["title"], "after-update");
    assert_eq!(
        updated.previous_data.as_ref().unwrap()["title"],
        "before-update"
    );
    assert_eq!(updated.version, 2);
    assert_eq!(updated.previous_version, Some(1));
    assert_eq!(updated.actor, "tx-agent");
    assert!(!updated.audit_id.is_empty());

    let deleted = &events[2];
    assert_eq!(deleted.entity_id, "tx-delete");
    assert_eq!(deleted.operation, "delete");
    assert!(deleted.data.is_none());
    assert_eq!(
        deleted.previous_data.as_ref().unwrap()["title"],
        "before-delete"
    );
    assert_eq!(deleted.version, 1);
    assert_eq!(deleted.previous_version, Some(1));
    assert_eq!(deleted.actor, "tx-agent");
    assert!(!deleted.audit_id.is_empty());
}
