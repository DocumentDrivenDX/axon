//! Reserved namespace parity tests for public HTTP and gRPC server surfaces.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_api::test_fixtures::{
    reserved_namespace_grpc_surface_parity_cases, reserved_namespace_http_surface_parity_cases,
    ReservedNamespaceSurfaceExposure, ReservedNamespaceSurfaceParityVector,
};
use axon_audit::entry::AuditEntry;
use axon_audit::log::{AuditPage, AuditQuery};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::{CollectionSchema, CollectionView};
use axon_server::gateway::build_router;
use axon_server::service::{proto, AxonService, AxonServiceImpl};
use axon_server::tenant_router::{TenantHandler, TenantRouter};
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;
use axum::http::StatusCode;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tonic::{Code, Request, Status};

#[derive(Clone, Default)]
struct StorageCallCounter {
    calls: Arc<AtomicUsize>,
}

impl StorageCallCounter {
    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn note(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

struct CountingStorageAdapter {
    inner: MemoryStorageAdapter,
    counter: StorageCallCounter,
}

impl CountingStorageAdapter {
    fn new(counter: StorageCallCounter) -> Self {
        Self {
            inner: MemoryStorageAdapter::default(),
            counter,
        }
    }

    fn note(&self) {
        self.counter.note();
    }
}

impl StorageAdapter for CountingStorageAdapter {
    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>, AxonError> {
        self.note();
        self.inner.get(collection, id)
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        self.note();
        self.inner.put(entity)
    }

    fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
        self.note();
        self.inner.delete(collection, id)
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        self.note();
        self.inner.count(collection)
    }

    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        self.note();
        self.inner.range_scan(collection, start, end, limit)
    }

    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        self.note();
        self.inner.compare_and_swap(entity, expected_version)
    }

    fn create_if_absent(
        &mut self,
        entity: Entity,
        expected_absent_version: u64,
    ) -> Result<Entity, AxonError> {
        self.note();
        self.inner.create_if_absent(entity, expected_absent_version)
    }

    fn begin_tx(&mut self) -> Result<(), AxonError> {
        self.note();
        self.inner.begin_tx()
    }

    fn commit_tx(&mut self) -> Result<(), AxonError> {
        self.note();
        self.inner.commit_tx()
    }

    fn abort_tx(&mut self) -> Result<(), AxonError> {
        self.note();
        self.inner.abort_tx()
    }

    fn append_audit_entry(&mut self, entry: AuditEntry) -> Result<AuditEntry, AxonError> {
        self.note();
        self.inner.append_audit_entry(entry)
    }

    fn owns_audit_log(&self) -> bool {
        self.note();
        self.inner.owns_audit_log()
    }

    fn query_audit_paginated(&self, query: AuditQuery) -> Result<AuditPage, AxonError> {
        self.note();
        self.inner.query_audit_paginated(query)
    }

    fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
        self.note();
        self.inner.put_schema(schema)
    }

    fn get_schema(&self, collection: &CollectionId) -> Result<Option<CollectionSchema>, AxonError> {
        self.note();
        self.inner.get_schema(collection)
    }

    fn delete_schema(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.note();
        self.inner.delete_schema(collection)
    }

    fn put_collection_view(&mut self, view: &CollectionView) -> Result<CollectionView, AxonError> {
        self.note();
        self.inner.put_collection_view(view)
    }

    fn get_collection_view(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionView>, AxonError> {
        self.note();
        self.inner.get_collection_view(collection)
    }

    fn delete_collection_view(&mut self, collection: &CollectionId) -> Result<(), AxonError> {
        self.note();
        self.inner.delete_collection_view(collection)
    }
}

fn exposed(
    case: axon_api::test_fixtures::ReservedNamespaceSurfaceParityCase,
) -> Option<ReservedNamespaceSurfaceParityVector> {
    match case.exposure {
        ReservedNamespaceSurfaceExposure::Exposed => Some(case.vector),
        ReservedNamespaceSurfaceExposure::NotExposed { reason } => {
            assert!(
                !reason.is_empty(),
                "not-exposed reserved namespace vector must record a reason"
            );
            None
        }
    }
}

fn assert_no_storage_calls(
    counter: &StorageCallCounter,
    vector: &ReservedNamespaceSurfaceParityVector,
) {
    assert_eq!(
        counter.count(),
        0,
        "{} vector for {} touched storage before reserved namespace rejection",
        vector.detail_operation,
        vector.detail_name
    );
}

fn assert_http_reserved_namespace_error(
    body: &Value,
    vector: &ReservedNamespaceSurfaceParityVector,
) {
    assert_eq!(body["code"], vector.code, "{vector:?}");
    assert_eq!(body["detail"]["reason"], vector.reason, "{vector:?}");
    assert_eq!(body["detail"]["name"], vector.detail_name, "{vector:?}");
    assert_eq!(
        body["detail"]["operation"], vector.detail_operation,
        "{vector:?}"
    );
}

fn assert_grpc_reserved_namespace_error(
    status: Status,
    vector: &ReservedNamespaceSurfaceParityVector,
) {
    assert_eq!(status.code(), Code::InvalidArgument, "{vector:?}");
    let body: Value =
        serde_json::from_str(status.message()).expect("reserved namespace status must be JSON");
    assert_eq!(body["code"], vector.code, "{vector:?}");
    assert_eq!(body["reason"], vector.reason, "{vector:?}");
    assert_eq!(body["detail"]["name"], vector.detail_name, "{vector:?}");
    assert_eq!(
        body["detail"]["operation"], vector.detail_operation,
        "{vector:?}"
    );
}

fn http_server(counter: StorageCallCounter) -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(CountingStorageAdapter::new(counter));
    let handler: TenantHandler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    axum_test::TestServer::new(build_router(tenant_router, "memory", None))
}

async fn http_reserved_namespace_response(
    server: &axum_test::TestServer,
    vector: &ReservedNamespaceSurfaceParityVector,
) -> axum_test::TestResponse {
    let name = vector.detail_name;
    match vector.detail_operation {
        "entity" => {
            server
                .get(&format!(
                    "/tenants/default/databases/default/entities/{name}/reserved-id"
                ))
                .await
        }
        "schema" => {
            server
                .get(&format!(
                    "/tenants/default/databases/default/collections/{name}/schema"
                ))
                .await
        }
        "template" => {
            server
                .get(&format!(
                    "/tenants/default/databases/default/collections/{name}/template"
                ))
                .await
        }
        "lifecycle" => {
            server
                .post(&format!(
                    "/tenants/default/databases/default/lifecycle/{name}/reserved-id/transition"
                ))
                .json(&json!({
                    "lifecycle_name": "workflow",
                    "target_state": "done",
                    "expected_version": 1
                }))
                .await
        }
        "link" => {
            server
                .post("/tenants/default/databases/default/links")
                .json(&json!({
                    "source_collection": name,
                    "source_id": "reserved-id",
                    "target_collection": "public-target",
                    "target_id": "target-id",
                    "link_type": "reserved-namespace-parity",
                    "metadata": {}
                }))
                .await
        }
        "rollback" => {
            server
                .post(&format!(
                    "/tenants/default/databases/default/collections/{name}/entities/reserved-id/rollback"
                ))
                .json(&json!({
                    "to_version": 1,
                    "dry_run": true
                }))
                .await
        }
        "query" => {
            server
                .post(&format!(
                    "/tenants/default/databases/default/collections/{name}/query"
                ))
                .json(&json!({}))
                .await
        }
        "traverse" => {
            server
                .get(&format!(
                    "/tenants/default/databases/default/traverse/{name}/reserved-id?max_depth=1"
                ))
                .await
        }
        "transaction" => {
            server
                .post("/tenants/default/databases/default/transactions")
                .json(&json!({
                    "operations": [{
                        "op": "create",
                        "collection": name,
                        "id": "reserved-id",
                        "data": { "title": "reserved namespace parity" }
                    }]
                }))
                .await
        }
        "audit" => {
            server
                .get(&format!(
                    "/tenants/default/databases/default/audit/entity/{name}/reserved-id"
                ))
                .await
        }
        other => panic!("unexpected HTTP reserved namespace operation: {other}"),
    }
}

fn grpc_service(counter: StorageCallCounter) -> AxonServiceImpl<CountingStorageAdapter> {
    AxonServiceImpl::from_handler(AxonHandler::new(CountingStorageAdapter::new(counter)))
}

async fn grpc_reserved_namespace_status(
    service: &AxonServiceImpl<CountingStorageAdapter>,
    vector: &ReservedNamespaceSurfaceParityVector,
) -> Status {
    let name = vector.detail_name.to_string();
    match vector.detail_operation {
        "entity" => service
            .get_entity(Request::new(proto::GetEntityRequest {
                collection: name,
                id: "reserved-id".into(),
            }))
            .await
            .expect_err("reserved entity vector must be rejected"),
        "schema" => service
            .get_schema(Request::new(proto::GetSchemaRequest { collection: name }))
            .await
            .expect_err("reserved schema vector must be rejected"),
        "lifecycle" => service
            .transition_lifecycle(Request::new(proto::TransitionLifecycleRequest {
                collection: name,
                entity_id: "reserved-id".into(),
                lifecycle_name: "workflow".into(),
                target_state: "done".into(),
                expected_version: 1,
                actor: String::new(),
            }))
            .await
            .expect_err("reserved lifecycle vector must be rejected"),
        "link" => service
            .create_link(Request::new(proto::CreateLinkRequest {
                source_collection: name,
                source_id: "reserved-id".into(),
                target_collection: "public-target".into(),
                target_id: "target-id".into(),
                link_type: "reserved-namespace-parity".into(),
                metadata_json: "{}".into(),
                actor: String::new(),
            }))
            .await
            .expect_err("reserved link vector must be rejected"),
        "query" => service
            .query_entities(Request::new(proto::QueryEntitiesRequest {
                collection: name,
                filter_json: String::new(),
                limit: 0,
                after_id: String::new(),
            }))
            .await
            .expect_err("reserved query vector must be rejected"),
        "traverse" => service
            .traverse(Request::new(proto::TraverseRequest {
                collection: name,
                id: "reserved-id".into(),
                link_type: String::new(),
                max_depth: 1,
            }))
            .await
            .expect_err("reserved traverse vector must be rejected"),
        "transaction" => service
            .commit_transaction(Request::new(proto::CommitTransactionRequest {
                operations: vec![proto::TransactionOp {
                    op: "create".into(),
                    collection: name,
                    id: "reserved-id".into(),
                    data_json: json!({ "title": "reserved namespace parity" }).to_string(),
                    expected_version: 0,
                }],
                actor: String::new(),
            }))
            .await
            .expect_err("reserved transaction vector must be rejected"),
        "audit" => service
            .query_audit_by_entity(Request::new(proto::QueryAuditByEntityRequest {
                collection: name,
                entity_id: "reserved-id".into(),
            }))
            .await
            .expect_err("reserved audit vector must be rejected"),
        other => panic!("unexpected gRPC reserved namespace operation: {other}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn http_reserved_namespace_surface_parity() {
    let counter = StorageCallCounter::default();
    let server = http_server(counter.clone());

    for vector in reserved_namespace_http_surface_parity_cases()
        .into_iter()
        .filter_map(exposed)
    {
        let response = http_reserved_namespace_response(&server, &vector).await;
        assert_eq!(
            response.status_code(),
            StatusCode::BAD_REQUEST,
            "{vector:?}"
        );
        let body = response.json::<Value>();
        assert_http_reserved_namespace_error(&body, &vector);
        assert_no_storage_calls(&counter, &vector);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn grpc_reserved_namespace_surface_parity() {
    let counter = StorageCallCounter::default();
    let service = grpc_service(counter.clone());

    for vector in reserved_namespace_grpc_surface_parity_cases()
        .into_iter()
        .filter_map(exposed)
    {
        let status = grpc_reserved_namespace_status(&service, &vector).await;
        assert_grpc_reserved_namespace_error(status, &vector);
        assert_no_storage_calls(&counter, &vector);
    }
}
