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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
async fn parity_create_get_entity() {
    // HTTP
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    let http_create = http
        .post("/entities/tasks/t-001")
        .json(&json!({"data": {"title": "hello"}, "actor": "test"}))
        .await;
    http_create.assert_status(axum::http::StatusCode::CREATED);
    let http_body: Value = http_create.json();

    let http_get = http.get("/entities/tasks/t-001").await;
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

#[tokio::test]
async fn parity_update_entity() {
    // HTTP
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    http.post("/entities/tasks/t-001")
        .json(&json!({"data": {"title": "v1"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let http_update = http
        .put("/entities/tasks/t-001")
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

#[tokio::test]
async fn parity_link_traverse() {
    // HTTP
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    http.post("/entities/users/u-001")
        .json(&json!({"data": {"name": "Alice"}}))
        .await;
    http.post("/entities/tasks/t-001")
        .json(&json!({"data": {"title": "Task 1"}}))
        .await;
    http.post("/links")
        .json(&json!({
            "source_collection": "users",
            "source_id": "u-001",
            "target_collection": "tasks",
            "target_id": "t-001",
            "link_type": "owns"
        }))
        .await;

    let http_traverse = http.get("/traverse/users/u-001?link_type=owns").await;
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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
#[tokio::test]
async fn http_traverse_direction_reverse() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    // Create entities A and B.
    http.post("/entities/nodes/a")
        .json(&json!({"data": {"name": "A"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.post("/entities/nodes/b")
        .json(&json!({"data": {"name": "B"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Create forward link: A -> B.
    http.post("/links")
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
        .get("/traverse/nodes/a?link_type=owns&direction=forward")
        .await;
    fwd.assert_status_ok();
    let fwd_body: Value = fwd.json();
    let fwd_entities = fwd_body["entities"].as_array().unwrap();
    assert_eq!(fwd_entities.len(), 1, "forward from A should return 1 entity");
    assert_eq!(fwd_entities[0]["id"], "b", "forward from A should return B");

    // Reverse traversal from B should return A (A links TO B).
    let rev = http
        .get("/traverse/nodes/b?link_type=owns&direction=reverse")
        .await;
    rev.assert_status_ok();
    let rev_body: Value = rev.json();
    let rev_entities = rev_body["entities"].as_array().unwrap();
    assert_eq!(rev_entities.len(), 1, "reverse from B should return 1 entity");
    assert_eq!(rev_entities[0]["id"], "a", "reverse from B should return A");
}

#[tokio::test]
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
async fn setup_lifecycle_client() -> proto::axon_service_client::AxonServiceClient<
    tonic::transport::Channel,
> {
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
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

#[tokio::test]
async fn http_caller_identity_from_header_recorded_in_audit() {
    let server = caller_identity_http_server();

    server
        .post("/entities/tasks/t-001")
        .add_header("x-axon-actor", "agent-1")
        .json(&json!({"data": {"title": "hello"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server.get("/audit/entity/tasks/t-001").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["actor"], "agent-1",
        "x-axon-actor header value must be recorded as the audit actor"
    );
}

#[tokio::test]
async fn http_missing_caller_identity_records_anonymous_in_audit() {
    let server = caller_identity_http_server();

    server
        .post("/entities/tasks/t-002")
        .json(&json!({"data": {"title": "no-header"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server.get("/audit/entity/tasks/t-002").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["actor"], "anonymous",
        "missing identity header falls back to the anonymous caller"
    );
}

#[tokio::test]
async fn http_caller_identity_body_actor_is_ignored() {
    let server = caller_identity_http_server();

    // Body carries an actor field, but no x-axon-actor header. The header is
    // the authority — body-level actor must NOT leak into the audit entry.
    server
        .post("/entities/tasks/t-003")
        .json(&json!({"data": {"title": "imposter"}, "actor": "claimed-by-body"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server.get("/audit/entity/tasks/t-003").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["actor"], "anonymous",
        "body-level actor must not bypass header-based identity extraction"
    );
}

#[tokio::test]
async fn http_caller_identity_empty_header_falls_back_to_anonymous() {
    let server = caller_identity_http_server();

    // An empty x-axon-actor header must be treated as "no header" rather
    // than recording an empty string as the actor.
    server
        .post("/entities/tasks/t-006")
        .add_header("x-axon-actor", "")
        .json(&json!({"data": {"title": "empty-header"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server.get("/audit/entity/tasks/t-006").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries[0]["actor"], "anonymous");
}

#[tokio::test]
async fn http_caller_identity_per_operation_type() {
    // Create, update, and delete all receive identity from the header.
    let server = caller_identity_http_server();

    server
        .post("/entities/tasks/t-005")
        .add_header("x-axon-actor", "creator")
        .json(&json!({"data": {"title": "v1"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .put("/entities/tasks/t-005")
        .add_header("x-axon-actor", "updater")
        .json(&json!({"data": {"title": "v2"}, "expected_version": 1}))
        .await
        .assert_status_ok();

    server
        .delete("/entities/tasks/t-005")
        .add_header("x-axon-actor", "deleter")
        .await
        .assert_status_ok();

    let resp = server.get("/audit/entity/tasks/t-005").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    // Entries are ordered chronologically: create, update, delete.
    assert_eq!(entries[0]["actor"], "creator");
    assert_eq!(entries[1]["actor"], "updater");
    assert_eq!(entries[2]["actor"], "deleter");
}

// ── Idempotency tests (FEAT-008 US-081) ──────────────────────────────────────

/// AC1 + AC2: POST with `Idempotency-Key` stores the response; a retry with
/// the same key within the TTL returns the cached body without re-executing.
#[tokio::test]
async fn http_transactions_idempotent_key_caches_success() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    // Seed the entity at v1 so the transaction's update can reference a
    // concrete expected_version.
    http.post("/entities/idem/e-1")
        .json(&json!({"data": {"v": 0}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let key = "idem-k-1";

    let resp1 = http
        .post("/transactions")
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
        .post("/transactions")
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
    let get_resp = http.get("/entities/idem/e-1").await;
    get_resp.assert_status_ok();
    let get_body: Value = get_resp.json();
    assert_eq!(
        get_body["entity"]["version"], 2,
        "cached response must not re-execute the mutation"
    );
}

/// Back-compat: POST without an `Idempotency-Key` header must still execute
/// normally; the cache is not consulted.
#[tokio::test]
async fn http_transactions_without_key_executes_normally() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    let resp1 = http
        .post("/transactions")
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
        .post("/transactions")
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
#[tokio::test]
async fn http_transactions_idempotent_conflict_not_cached() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    // Seed at v1, then update to v2 out-of-band so the transaction's
    // expected_version=1 will fail deterministically.
    http.post("/entities/conf/e-1")
        .json(&json!({"data": {"v": 0}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
    http.put("/entities/conf/e-1")
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
        .post("/transactions")
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
        .post("/transactions")
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
#[tokio::test]
async fn http_transactions_idempotent_different_databases_isolated() {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));
    let http_app = build_router(tenant_router, "memory", None);
    let http = axum_test::TestServer::new(http_app);

    let key = "shared-k-1";

    // POST to database "alpha" creates an entity there.
    let resp_a = http
        .post("/transactions")
        .add_header("idempotency-key", key)
        .add_header("x-axon-database", "alpha")
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
        .post("/transactions")
        .add_header("idempotency-key", key)
        .add_header("x-axon-database", "beta")
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
    http.get("/entities/iso/e-alpha")
        .add_header("x-axon-database", "alpha")
        .await
        .assert_status_ok();
    http.get("/entities/iso/e-beta")
        .add_header("x-axon-database", "beta")
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
#[tokio::test]
async fn http_transactions_idempotent_expired_reexecutes() {
    use axon_core::clock::SystemClock;
    use axon_server::gateway::build_router_with_idempotency;
    use axon_server::idempotency::IdempotencyStore;
    use std::time::Duration;

    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let http_handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(http_handler));

    let store = Arc::new(IdempotencyStore::<Value>::new(
        Arc::new(SystemClock),
        Duration::from_millis(100),
    ));
    let http_app = build_router_with_idempotency(tenant_router, "memory", None, store);
    let http = axum_test::TestServer::new(http_app);

    // Seed an entity at v1 so each execution attempts a valid update.
    http.post("/entities/exp/e-1")
        .json(&json!({"data": {"v": 0}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let key = "exp-k-1";

    let resp1 = http
        .post("/transactions")
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
        .post("/transactions")
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

#[tokio::test]
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
