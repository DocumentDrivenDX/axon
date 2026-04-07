use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest,
    DeleteLinkRequest, DescribeCollectionRequest, DropCollectionRequest, GetEntityRequest,
    GetSchemaRequest, ListCollectionsRequest, PutSchemaRequest, QueryEntitiesRequest,
    TraverseRequest, UpdateEntityRequest,
};
use axon_api::transaction::Transaction;
use axon_audit::log::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

// Include the generated protobuf code.
pub mod proto {
    tonic::include_proto!("axon.v1");
}

pub use proto::axon_service_server::{AxonService, AxonServiceServer};
pub use proto::{
    AuditEntryProto, CollectionMeta, CommitTransactionRequest as ProtoCommitTxReq,
    CommitTransactionResponse as ProtoCommitTxResp,
    CreateCollectionRequest as ProtoCreateCollectionReq,
    CreateCollectionResponse as ProtoCreateCollectionResp,
    CreateEntityRequest as ProtoCreateEntityReq, CreateEntityResponse as ProtoCreateEntityResp,
    CreateLinkRequest as ProtoCreateLinkReq, CreateLinkResponse as ProtoCreateLinkResp,
    DeleteEntityRequest as ProtoDeleteEntityReq, DeleteEntityResponse as ProtoDeleteEntityResp,
    DeleteLinkRequest as ProtoDeleteLinkReq, DeleteLinkResponse as ProtoDeleteLinkResp,
    DescribeCollectionRequest as ProtoDescribeCollectionReq,
    DescribeCollectionResponse as ProtoDescribeCollectionResp,
    DropCollectionRequest as ProtoDropCollectionReq,
    DropCollectionResponse as ProtoDropCollectionResp, EntityProto,
    GetEntityRequest as ProtoGetEntityReq, GetEntityResponse as ProtoGetEntityResp,
    GetSchemaRequest as ProtoGetSchemaReq, GetSchemaResponse as ProtoGetSchemaResp, LinkProto,
    ListCollectionsRequest as ProtoListCollectionsReq,
    ListCollectionsResponse as ProtoListCollectionsResp, PutSchemaRequest as ProtoPutSchemaReq,
    PutSchemaResponse as ProtoPutSchemaResp, QueryAuditByEntityRequest, QueryAuditByEntityResponse,
    QueryEntitiesRequest as ProtoQueryEntitiesReq, QueryEntitiesResponse as ProtoQueryEntitiesResp,
    TransactionOp as ProtoTxOp, TraverseRequest as ProtoTraverseReq,
    TraverseResponse as ProtoTraverseResp, UpdateEntityRequest as ProtoUpdateEntityReq,
    UpdateEntityResponse as ProtoUpdateEntityResp,
};

/// Convert an [`AxonError`] to a gRPC [`Status`] with a structured message.
fn axon_to_status(err: AxonError) -> Status {
    match err {
        AxonError::NotFound(msg) => Status::not_found(msg),
        AxonError::ConflictingVersion {
            expected,
            actual,
            current_entity,
        } => {
            let current_entity_json = match &current_entity {
                Some(e) => serde_json::to_string(e).unwrap_or_else(|_| "null".to_string()),
                None => "null".to_string(),
            };
            Status::failed_precondition(format!(
                "{{\"code\":\"version_conflict\",\"expected\":{expected},\"actual\":{actual},\"current_entity\":{current_entity_json}}}"
            ))
        }
        AxonError::SchemaValidation(detail) => Status::invalid_argument(format!(
            "{{\"code\":\"schema_validation\",\"detail\":{detail:?}}}"
        )),
        AxonError::AlreadyExists(msg) => Status::already_exists(msg),
        AxonError::InvalidArgument(msg) => Status::invalid_argument(msg),
        AxonError::InvalidOperation(msg) => Status::invalid_argument(msg),
        AxonError::Storage(msg) => {
            Status::internal(format!("{{\"code\":\"storage_error\",\"detail\":{msg:?}}}"))
        }
        AxonError::Serialization(e) => {
            Status::internal(format!("{{\"code\":\"serialization\",\"detail\":\"{e}\"}}"))
        }
        AxonError::UniqueViolation { field, value } => Status::already_exists(format!(
            "{{\"code\":\"unique_violation\",\"field\":{field:?},\"value\":{value:?}}}"
        )),
    }
}

fn entity_to_proto(e: axon_core::types::Entity) -> EntityProto {
    EntityProto {
        collection: e.collection.to_string(),
        id: e.id.to_string(),
        version: e.version,
        data_json: e.data.to_string(),
    }
}

/// Shared state for the gRPC service.
///
/// Wraps an `AxonHandler` in a `Mutex` so multiple async tasks can call it.
pub struct AxonServiceImpl {
    handler: Arc<Mutex<AxonHandler<MemoryStorageAdapter>>>,
}

impl AxonServiceImpl {
    /// Create a service backed by an in-memory storage adapter.
    pub fn new_in_memory() -> Self {
        Self {
            handler: Arc::new(Mutex::new(
                AxonHandler::new(MemoryStorageAdapter::default()),
            )),
        }
    }

    /// Create a service with a pre-built handler (useful for tests).
    pub fn from_handler(handler: AxonHandler<MemoryStorageAdapter>) -> Self {
        Self {
            handler: Arc::new(Mutex::new(handler)),
        }
    }

    /// Create a service sharing an existing handler reference.
    ///
    /// Use this to share state between the gRPC service and the HTTP gateway.
    pub fn from_shared(handler: Arc<Mutex<AxonHandler<MemoryStorageAdapter>>>) -> Self {
        Self { handler }
    }
}

#[tonic::async_trait]
impl AxonService for AxonServiceImpl {
    async fn create_entity(
        &self,
        request: Request<ProtoCreateEntityReq>,
    ) -> Result<Response<ProtoCreateEntityResp>, Status> {
        let req = request.into_inner();
        let data: serde_json::Value = serde_json::from_str(&req.data_json)
            .map_err(|e| Status::invalid_argument(format!("invalid data_json: {e}")))?;
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new(&req.collection),
                id: EntityId::new(&req.id),
                data,
                actor,
                audit_metadata: None,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoCreateEntityResp {
            entity: Some(entity_to_proto(resp.entity)),
        }))
    }

    async fn get_entity(
        &self,
        request: Request<ProtoGetEntityReq>,
    ) -> Result<Response<ProtoGetEntityResp>, Status> {
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .get_entity(GetEntityRequest {
                collection: CollectionId::new(&req.collection),
                id: EntityId::new(&req.id),
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoGetEntityResp {
            entity: Some(entity_to_proto(resp.entity)),
        }))
    }

    async fn update_entity(
        &self,
        request: Request<ProtoUpdateEntityReq>,
    ) -> Result<Response<ProtoUpdateEntityResp>, Status> {
        let req = request.into_inner();
        let data: serde_json::Value = serde_json::from_str(&req.data_json)
            .map_err(|e| Status::invalid_argument(format!("invalid data_json: {e}")))?;
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .update_entity(UpdateEntityRequest {
                collection: CollectionId::new(&req.collection),
                id: EntityId::new(&req.id),
                data,
                expected_version: req.expected_version,
                actor,
                audit_metadata: None,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoUpdateEntityResp {
            entity: Some(entity_to_proto(resp.entity)),
        }))
    }

    async fn delete_entity(
        &self,
        request: Request<ProtoDeleteEntityReq>,
    ) -> Result<Response<ProtoDeleteEntityResp>, Status> {
        let req = request.into_inner();
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .delete_entity(DeleteEntityRequest {
                collection: CollectionId::new(&req.collection),
                id: EntityId::new(&req.id),
                actor,
                audit_metadata: None,
                force: false,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoDeleteEntityResp {
            collection: resp.collection,
            id: resp.id,
        }))
    }

    async fn create_link(
        &self,
        request: Request<ProtoCreateLinkReq>,
    ) -> Result<Response<ProtoCreateLinkResp>, Status> {
        let req = request.into_inner();
        let metadata: serde_json::Value = if req.metadata_json.is_empty() {
            json!(null)
        } else {
            serde_json::from_str(&req.metadata_json)
                .map_err(|e| Status::invalid_argument(format!("invalid metadata_json: {e}")))?
        };
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new(&req.source_collection),
                source_id: EntityId::new(&req.source_id),
                target_collection: CollectionId::new(&req.target_collection),
                target_id: EntityId::new(&req.target_id),
                link_type: req.link_type.clone(),
                metadata,
                actor,
            })
            .map_err(axon_to_status)?;

        let link = resp.link;
        Ok(Response::new(ProtoCreateLinkResp {
            link: Some(LinkProto {
                source_collection: link.source_collection.to_string(),
                source_id: link.source_id.to_string(),
                target_collection: link.target_collection.to_string(),
                target_id: link.target_id.to_string(),
                link_type: link.link_type,
                metadata_json: link.metadata.to_string(),
            }),
        }))
    }

    async fn delete_link(
        &self,
        request: Request<ProtoDeleteLinkReq>,
    ) -> Result<Response<ProtoDeleteLinkResp>, Status> {
        let req = request.into_inner();
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .delete_link(DeleteLinkRequest {
                source_collection: CollectionId::new(&req.source_collection),
                source_id: EntityId::new(&req.source_id),
                target_collection: CollectionId::new(&req.target_collection),
                target_id: EntityId::new(&req.target_id),
                link_type: req.link_type.clone(),
                actor,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoDeleteLinkResp {
            source_collection: resp.source_collection,
            source_id: resp.source_id,
            target_collection: resp.target_collection,
            target_id: resp.target_id,
            link_type: resp.link_type,
        }))
    }

    async fn traverse(
        &self,
        request: Request<ProtoTraverseReq>,
    ) -> Result<Response<ProtoTraverseResp>, Status> {
        let req = request.into_inner();
        let link_type = if req.link_type.is_empty() {
            None
        } else {
            Some(req.link_type.clone())
        };
        let max_depth = if req.max_depth == 0 {
            None
        } else {
            Some(req.max_depth as usize)
        };

        let resp = self
            .handler
            .lock()
            .await
            .traverse(TraverseRequest {
                collection: CollectionId::new(&req.collection),
                id: EntityId::new(&req.id),
                link_type,
                max_depth,
                direction: Default::default(),
                hop_filter: None,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoTraverseResp {
            entities: resp.entities.into_iter().map(entity_to_proto).collect(),
        }))
    }

    async fn query_audit_by_entity(
        &self,
        request: Request<QueryAuditByEntityRequest>,
    ) -> Result<Response<QueryAuditByEntityResponse>, Status> {
        let req = request.into_inner();
        let handler = self.handler.lock().await;
        let entries = handler
            .audit_log()
            .query_by_entity(
                &CollectionId::new(&req.collection),
                &EntityId::new(&req.entity_id),
            )
            .map_err(axon_to_status)?;

        let proto_entries = entries
            .into_iter()
            .map(|e| AuditEntryProto {
                id: e.id,
                timestamp_ns: e.timestamp_ns,
                collection: e.collection.to_string(),
                entity_id: e.entity_id.to_string(),
                version: e.version,
                mutation: format!("{:?}", e.mutation),
                data_before_json: e
                    .data_before
                    .as_ref()
                    .map(|v: &serde_json::Value| v.to_string())
                    .unwrap_or_default(),
                data_after_json: e
                    .data_after
                    .as_ref()
                    .map(|v: &serde_json::Value| v.to_string())
                    .unwrap_or_default(),
                actor: e.actor,
                transaction_id: e.transaction_id.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(QueryAuditByEntityResponse {
            entries: proto_entries,
        }))
    }

    async fn commit_transaction(
        &self,
        request: Request<ProtoCommitTxReq>,
    ) -> Result<Response<ProtoCommitTxResp>, Status> {
        let req = request.into_inner();
        let mut tx = Transaction::new();

        for op in &req.operations {
            let result = match op.op.as_str() {
                "create" => {
                    let data: serde_json::Value = serde_json::from_str(&op.data_json)
                        .map_err(|e| Status::invalid_argument(format!("invalid data_json: {e}")))?;
                    tx.create(axon_core::types::Entity::new(
                        CollectionId::new(&op.collection),
                        EntityId::new(&op.id),
                        data,
                    ))
                }
                "update" => {
                    let data: serde_json::Value = serde_json::from_str(&op.data_json)
                        .map_err(|e| Status::invalid_argument(format!("invalid data_json: {e}")))?;
                    let h = self.handler.lock().await;
                    let data_before = h
                        .get_entity(GetEntityRequest {
                            collection: CollectionId::new(&op.collection),
                            id: EntityId::new(&op.id),
                        })
                        .ok()
                        .map(|r| r.entity.data);
                    drop(h);
                    tx.update(
                        axon_core::types::Entity::new(
                            CollectionId::new(&op.collection),
                            EntityId::new(&op.id),
                            data,
                        ),
                        op.expected_version,
                        data_before,
                    )
                }
                "delete" => {
                    let h = self.handler.lock().await;
                    let data_before = h
                        .get_entity(GetEntityRequest {
                            collection: CollectionId::new(&op.collection),
                            id: EntityId::new(&op.id),
                        })
                        .ok()
                        .map(|r| r.entity.data);
                    drop(h);
                    tx.delete(
                        CollectionId::new(&op.collection),
                        EntityId::new(&op.id),
                        op.expected_version,
                        data_before,
                    )
                }
                other => {
                    return Err(Status::invalid_argument(format!(
                        "unknown operation: {other}"
                    )));
                }
            };
            result.map_err(axon_to_status)?;
        }

        let tx_id = tx.id.clone();
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor)
        };

        let mut h = self.handler.lock().await;
        let (storage, audit) = h.storage_and_audit_mut();
        let written = tx.commit(storage, audit, actor).map_err(axon_to_status)?;

        Ok(Response::new(ProtoCommitTxResp {
            transaction_id: tx_id,
            entities: written.into_iter().map(entity_to_proto).collect(),
        }))
    }

    async fn query_entities(
        &self,
        request: Request<ProtoQueryEntitiesReq>,
    ) -> Result<Response<ProtoQueryEntitiesResp>, Status> {
        let req = request.into_inner();
        let filter = if req.filter_json.is_empty() {
            None
        } else {
            Some(
                serde_json::from_str(&req.filter_json)
                    .map_err(|e| Status::invalid_argument(format!("invalid filter_json: {e}")))?,
            )
        };
        let limit = if req.limit == 0 {
            None
        } else {
            Some(req.limit as usize)
        };
        let after_id = if req.after_id.is_empty() {
            None
        } else {
            Some(axon_core::id::EntityId::new(&req.after_id))
        };

        let resp = self
            .handler
            .lock()
            .await
            .query_entities(QueryEntitiesRequest {
                collection: axon_core::id::CollectionId::new(&req.collection),
                filter,
                sort: vec![],
                limit,
                after_id,
                count_only: false,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoQueryEntitiesResp {
            entities: resp.entities.into_iter().map(entity_to_proto).collect(),
            total_count: resp.total_count as u64,
            next_cursor: resp.next_cursor.unwrap_or_default(),
        }))
    }

    async fn put_schema(
        &self,
        request: Request<ProtoPutSchemaReq>,
    ) -> Result<Response<ProtoPutSchemaResp>, Status> {
        let req = request.into_inner();
        let schema: axon_schema::schema::CollectionSchema = serde_json::from_str(&req.schema_json)
            .map_err(|e| Status::invalid_argument(format!("invalid schema_json: {e}")))?;
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor,
                force: req.force,
                dry_run: req.dry_run,
            })
            .map_err(axon_to_status)?;

        let compatibility = resp
            .compatibility
            .map(|c| format!("{c:?}"))
            .unwrap_or_default();
        let schema_json = serde_json::to_string(&resp.schema)
            .map_err(|e| Status::internal(format!("serialization error: {e}")))?;

        Ok(Response::new(ProtoPutSchemaResp {
            schema_json,
            compatibility,
            dry_run: resp.dry_run,
        }))
    }

    async fn get_schema(
        &self,
        request: Request<ProtoGetSchemaReq>,
    ) -> Result<Response<ProtoGetSchemaResp>, Status> {
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .handle_get_schema(GetSchemaRequest {
                collection: axon_core::id::CollectionId::new(&req.collection),
            })
            .map_err(axon_to_status)?;

        let schema_json = serde_json::to_string(&resp.schema)
            .map_err(|e| Status::internal(format!("serialization error: {e}")))?;

        Ok(Response::new(ProtoGetSchemaResp { schema_json }))
    }

    async fn create_collection(
        &self,
        request: Request<ProtoCreateCollectionReq>,
    ) -> Result<Response<ProtoCreateCollectionResp>, Status> {
        let req = request.into_inner();
        let schema: axon_schema::schema::CollectionSchema = serde_json::from_str(&req.schema_json)
            .map_err(|e| Status::invalid_argument(format!("invalid schema_json: {e}")))?;
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .create_collection(CreateCollectionRequest {
                name: axon_core::id::CollectionId::new(&req.name),
                schema,
                actor,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoCreateCollectionResp { name: resp.name }))
    }

    async fn drop_collection(
        &self,
        request: Request<ProtoDropCollectionReq>,
    ) -> Result<Response<ProtoDropCollectionResp>, Status> {
        let req = request.into_inner();
        let actor = if req.actor.is_empty() {
            None
        } else {
            Some(req.actor.clone())
        };

        let resp = self
            .handler
            .lock()
            .await
            .drop_collection(DropCollectionRequest {
                name: axon_core::id::CollectionId::new(&req.name),
                actor,
                confirm: req.confirm,
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoDropCollectionResp {
            name: resp.name,
            entities_removed: resp.entities_removed as u64,
        }))
    }

    async fn list_collections(
        &self,
        _request: Request<ProtoListCollectionsReq>,
    ) -> Result<Response<ProtoListCollectionsResp>, Status> {
        let resp = self
            .handler
            .lock()
            .await
            .list_collections(ListCollectionsRequest {})
            .map_err(axon_to_status)?;

        let collections = resp
            .collections
            .into_iter()
            .map(|c| CollectionMeta {
                name: c.name,
                entity_count: c.entity_count as u64,
                schema_version: c.schema_version.unwrap_or(0),
            })
            .collect();

        Ok(Response::new(ProtoListCollectionsResp { collections }))
    }

    async fn describe_collection(
        &self,
        request: Request<ProtoDescribeCollectionReq>,
    ) -> Result<Response<ProtoDescribeCollectionResp>, Status> {
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .describe_collection(DescribeCollectionRequest {
                name: axon_core::id::CollectionId::new(&req.name),
            })
            .map_err(axon_to_status)?;

        let schema_json = match resp.schema {
            Some(s) => serde_json::to_string(&s)
                .map_err(|e| Status::internal(format!("serialization error: {e}")))?,
            None => String::new(),
        };

        Ok(Response::new(ProtoDescribeCollectionResp {
            name: resp.name,
            entity_count: resp.entity_count as u64,
            schema_json,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tonic::Code;

    /// Build a service instance and create one entity in collection `col` with id `id`.
    async fn make_service_with_entity(col: &str, id: &str) -> AxonServiceImpl {
        let svc = AxonServiceImpl::new_in_memory();
        svc.create_entity(Request::new(ProtoCreateEntityReq {
            collection: col.to_string(),
            id: id.to_string(),
            data_json: json!({"x": 1}).to_string(),
            actor: String::new(),
        }))
        .await
        .expect("create should succeed");
        svc
    }

    /// FEAT-004 US-010 AC4 / FEAT-008 US-021 AC2:
    /// A version-conflict gRPC response must include the current entity state so
    /// the caller can merge and retry without a separate GetEntity round-trip.
    #[tokio::test]
    async fn grpc_version_conflict_includes_current_entity() {
        let svc = make_service_with_entity("tasks", "t-001").await;

        // Attempt update with a wrong expected_version.
        let err = svc
            .update_entity(Request::new(ProtoUpdateEntityReq {
                collection: "tasks".to_string(),
                id: "t-001".to_string(),
                data_json: json!({"x": 2}).to_string(),
                expected_version: 99, // wrong — actual is 1
                actor: String::new(),
            }))
            .await
            .expect_err("should fail with version conflict");

        assert_eq!(
            err.code(),
            Code::FailedPrecondition,
            "wrong gRPC status code"
        );

        let msg: serde_json::Value =
            serde_json::from_str(err.message()).expect("status message must be valid JSON");

        assert_eq!(msg["code"], "version_conflict");
        assert_eq!(msg["expected"], 99_u64);
        assert_eq!(msg["actual"], 1_u64);

        // current_entity must be present and non-null (FEAT-004 US-010 AC4).
        let current = &msg["current_entity"];
        assert!(
            !current.is_null(),
            "current_entity must not be null in a conflict response; got: {msg}"
        );
        assert_eq!(current["id"], "t-001", "current_entity.id mismatch");
        assert_eq!(current["version"], 1_u64, "current_entity.version mismatch");
        assert_eq!(current["data"]["x"], 1, "current_entity.data mismatch");
    }

    /// Verify that a version conflict with no surviving entity yields current_entity: null.
    #[tokio::test]
    async fn grpc_version_conflict_null_current_entity_when_missing() {
        // We can trigger a null current_entity by injecting an AxonError directly
        // through axon_to_status (the private function under test).
        let status = axon_to_status(AxonError::ConflictingVersion {
            expected: 5,
            actual: 3,
            current_entity: None,
        });

        assert_eq!(status.code(), Code::FailedPrecondition);

        let msg: serde_json::Value =
            serde_json::from_str(status.message()).expect("status message must be valid JSON");

        assert_eq!(msg["code"], "version_conflict");
        assert!(msg["current_entity"].is_null());
    }
}
