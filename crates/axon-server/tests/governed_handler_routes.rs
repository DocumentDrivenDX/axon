#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::service::proto;
use axon_server::service::AxonService;
use axon_server::service::AxonServiceImpl;
use axon_server::tenant_router::{TenantHandler, TenantRouter};
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tonic::Request;

type TestStorage = Box<dyn StorageAdapter + Send + Sync>;

fn shared_handler() -> TenantHandler {
    let storage: TestStorage =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite should open"));
    Arc::new(Mutex::new(AxonHandler::new(storage)))
}

fn http_server(handler: TenantHandler) -> TestServer {
    let tenant_router = Arc::new(TenantRouter::single(handler));
    TestServer::new(build_router(tenant_router, "memory", None))
}

#[tokio::test(flavor = "multi_thread")]
async fn governed_handler_routes_http_gateway_transaction_audit_query() {
    let server = http_server(shared_handler());

    server
        .post("/tenants/default/databases/default/entities/accounts/A")
        .json(&json!({"data": {"balance": 100}}))
        .await
        .assert_status(StatusCode::CREATED);
    server
        .post("/tenants/default/databases/default/entities/accounts/B")
        .json(&json!({"data": {"balance": 50}}))
        .await
        .assert_status(StatusCode::CREATED);

    let commit = server
        .post("/tenants/default/databases/default/transactions")
        .json(&json!({
            "operations": [
                {
                    "op": "update",
                    "collection": "accounts",
                    "id": "A",
                    "data": {"balance": 70},
                    "expected_version": 1
                },
                {
                    "op": "update",
                    "collection": "accounts",
                    "id": "B",
                    "data": {"balance": 80},
                    "expected_version": 1
                }
            ]
        }))
        .await;
    commit.assert_status_ok();
    let commit_body: Value = commit.json();
    let transaction_id = commit_body["transaction_id"]
        .as_str()
        .expect("transaction id should be present");

    let audit = server
        .get("/tenants/default/databases/default/audit/query?collection=accounts&limit=100")
        .await;
    audit.assert_status_ok();
    let audit_body: Value = audit.json();
    let entries = audit_body["entries"]
        .as_array()
        .expect("audit query should return entries");

    assert!(entries
        .iter()
        .any(|entry| entry["transaction_id"] == transaction_id && entry["entity_id"] == "A"));
    assert!(entries
        .iter()
        .any(|entry| entry["transaction_id"] == transaction_id && entry["entity_id"] == "B"));
}

#[tokio::test(flavor = "multi_thread")]
async fn governed_handler_routes_grpc_transaction_audit_query() {
    let svc = AxonServiceImpl::from_shared(shared_handler());

    svc.create_entity(Request::new(proto::CreateEntityRequest {
        collection: "tasks".into(),
        id: "t-001".into(),
        data_json: json!({"status": "draft"}).to_string(),
        actor: String::new(),
    }))
    .await
    .expect("seed create should succeed");

    let committed = svc
        .commit_transaction(Request::new(proto::CommitTransactionRequest {
            operations: vec![proto::TransactionOp {
                op: "update".into(),
                collection: "tasks".into(),
                id: "t-001".into(),
                data_json: json!({"status": "done"}).to_string(),
                expected_version: 1,
            }],
            actor: String::new(),
        }))
        .await
        .expect("transaction commit should succeed")
        .into_inner();

    let audit = svc
        .query_audit_by_entity(Request::new(proto::QueryAuditByEntityRequest {
            collection: "tasks".into(),
            entity_id: "t-001".into(),
        }))
        .await
        .expect("audit query should succeed")
        .into_inner();

    assert!(audit.entries.iter().any(|entry| {
        entry.transaction_id == committed.transaction_id && entry.mutation == "EntityUpdate"
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn governed_handler_routes_embedded_shared_server_paths() {
    let handler = shared_handler();
    let server = http_server(handler.clone());
    let grpc = AxonServiceImpl::from_shared(handler);

    server
        .post("/tenants/default/databases/default/entities/tasks/t-embedded")
        .json(&json!({"data": {"status": "draft"}}))
        .await
        .assert_status(StatusCode::CREATED);

    let committed = grpc
        .commit_transaction(Request::new(proto::CommitTransactionRequest {
            operations: vec![proto::TransactionOp {
                op: "update".into(),
                collection: "tasks".into(),
                id: "t-embedded".into(),
                data_json: json!({"status": "done"}).to_string(),
                expected_version: 1,
            }],
            actor: String::new(),
        }))
        .await
        .expect("shared-handler gRPC transaction should succeed")
        .into_inner();

    let entity = server
        .get("/tenants/default/databases/default/entities/tasks/t-embedded")
        .await;
    entity.assert_status_ok();
    let entity_body: Value = entity.json();
    assert_eq!(entity_body["entity"]["data"]["status"], "done");

    let audit = server
        .get("/tenants/default/databases/default/audit/entity/tasks/t-embedded")
        .await;
    audit.assert_status_ok();
    let audit_body: Value = audit.json();
    let entries = audit_body["entries"]
        .as_array()
        .expect("embedded audit query should return entries");
    assert!(entries
        .iter()
        .any(|entry| entry["transaction_id"] == committed.transaction_id));
}
