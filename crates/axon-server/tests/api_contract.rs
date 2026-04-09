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
use axon_storage::memory::MemoryStorageAdapter;

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
    let http_handler = Arc::new(Mutex::new(
        AxonHandler::new(MemoryStorageAdapter::default()),
    ));
    let http_app = build_router(http_handler, "memory", None);
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
    let http_handler = Arc::new(Mutex::new(
        AxonHandler::new(MemoryStorageAdapter::default()),
    ));
    let http_app = build_router(http_handler, "memory", None);
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
    let http_handler = Arc::new(Mutex::new(
        AxonHandler::new(MemoryStorageAdapter::default()),
    ));
    let http_app = build_router(http_handler, "memory", None);
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
