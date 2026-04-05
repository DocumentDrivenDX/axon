use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, DeleteLinkRequest,
    GetEntityRequest, TraverseRequest, UpdateEntityRequest,
};
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
    AuditEntryProto, CreateEntityRequest as ProtoCreateEntityReq,
    CreateEntityResponse as ProtoCreateEntityResp, CreateLinkRequest as ProtoCreateLinkReq,
    CreateLinkResponse as ProtoCreateLinkResp, DeleteEntityRequest as ProtoDeleteEntityReq,
    DeleteEntityResponse as ProtoDeleteEntityResp, DeleteLinkRequest as ProtoDeleteLinkReq,
    DeleteLinkResponse as ProtoDeleteLinkResp, EntityProto, GetEntityRequest as ProtoGetEntityReq,
    GetEntityResponse as ProtoGetEntityResp, LinkProto, QueryAuditByEntityRequest,
    QueryAuditByEntityResponse, TraverseRequest as ProtoTraverseReq,
    TraverseResponse as ProtoTraverseResp, UpdateEntityRequest as ProtoUpdateEntityReq,
    UpdateEntityResponse as ProtoUpdateEntityResp,
};

/// Convert an [`AxonError`] to a gRPC [`Status`] with a structured message.
fn axon_to_status(err: AxonError) -> Status {
    match &err {
        AxonError::NotFound(msg) => Status::not_found(msg.clone()),
        AxonError::ConflictingVersion { expected, actual } => Status::failed_precondition(format!(
            "{{\"code\":\"version_conflict\",\"expected\":{expected},\"actual\":{actual}}}"
        )),
        AxonError::SchemaValidation(detail) => Status::invalid_argument(format!(
            "{{\"code\":\"schema_validation\",\"detail\":{detail:?}}}"
        )),
        AxonError::AlreadyExists(msg) => Status::already_exists(msg.clone()),
        AxonError::InvalidArgument(msg) => Status::invalid_argument(msg.clone()),
        AxonError::InvalidOperation(msg) => Status::invalid_argument(msg.clone()),
        AxonError::Storage(msg) => {
            Status::internal(format!("{{\"code\":\"storage_error\",\"detail\":{msg:?}}}"))
        }
        AxonError::Serialization(e) => {
            Status::internal(format!("{{\"code\":\"serialization\",\"detail\":\"{e}\"}}"))
        }
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
}
