use std::sync::Arc;

use crate::auth::{AuthContext, AuthError, Identity};
use crate::collection_listing::list_collections_for_database;
use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest,
    CreateNamespaceRequest, DeleteEntityRequest, DeleteLinkRequest, DescribeCollectionRequest,
    DropCollectionRequest, DropDatabaseRequest, DropNamespaceRequest, GetEntityRequest,
    GetSchemaRequest, ListCollectionsRequest, ListDatabasesRequest,
    ListNamespaceCollectionsRequest, ListNamespacesRequest, PutSchemaRequest, QueryEntitiesRequest,
    TraverseRequest, UpdateEntityRequest,
};
use axon_api::transaction::Transaction;
use axon_audit::log::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_DATABASE};
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

const AXON_DATABASE_HEADER: &str = "x-axon-database";

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
    CreateDatabaseRequest as ProtoCreateDatabaseReq,
    CreateDatabaseResponse as ProtoCreateDatabaseResp, CreateEntityRequest as ProtoCreateEntityReq,
    CreateEntityResponse as ProtoCreateEntityResp, CreateLinkRequest as ProtoCreateLinkReq,
    CreateLinkResponse as ProtoCreateLinkResp, CreateNamespaceRequest as ProtoCreateNamespaceReq,
    CreateNamespaceResponse as ProtoCreateNamespaceResp,
    DeleteEntityRequest as ProtoDeleteEntityReq, DeleteEntityResponse as ProtoDeleteEntityResp,
    DeleteLinkRequest as ProtoDeleteLinkReq, DeleteLinkResponse as ProtoDeleteLinkResp,
    DescribeCollectionRequest as ProtoDescribeCollectionReq,
    DescribeCollectionResponse as ProtoDescribeCollectionResp,
    DropCollectionRequest as ProtoDropCollectionReq,
    DropCollectionResponse as ProtoDropCollectionResp, DropDatabaseRequest as ProtoDropDatabaseReq,
    DropDatabaseResponse as ProtoDropDatabaseResp, DropNamespaceRequest as ProtoDropNamespaceReq,
    DropNamespaceResponse as ProtoDropNamespaceResp, EntityProto,
    GetEntityRequest as ProtoGetEntityReq, GetEntityResponse as ProtoGetEntityResp,
    GetSchemaRequest as ProtoGetSchemaReq, GetSchemaResponse as ProtoGetSchemaResp, LinkProto,
    ListCollectionsRequest as ProtoListCollectionsReq,
    ListCollectionsResponse as ProtoListCollectionsResp,
    ListDatabasesRequest as ProtoListDatabasesReq, ListDatabasesResponse as ProtoListDatabasesResp,
    ListNamespaceCollectionsRequest as ProtoListNamespaceCollectionsReq,
    ListNamespaceCollectionsResponse as ProtoListNamespaceCollectionsResp,
    ListNamespacesRequest as ProtoListNamespacesReq,
    ListNamespacesResponse as ProtoListNamespacesResp, PutSchemaRequest as ProtoPutSchemaReq,
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

fn auth_to_status(error: AuthError) -> Status {
    match error {
        AuthError::MissingPeerAddress | AuthError::Unauthorized(_) => {
            Status::unauthenticated(error.to_string())
        }
        AuthError::ProviderUnavailable(_) => Status::unavailable(error.to_string()),
    }
}

fn grpc_current_database<T>(request: &Request<T>) -> &str {
    grpc_requested_database(request).unwrap_or(DEFAULT_DATABASE)
}

fn grpc_requested_database<T>(request: &Request<T>) -> Option<&str> {
    request
        .metadata()
        .get(AXON_DATABASE_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|database| !database.is_empty())
}

fn qualify_collection_name(collection: &str, current_database: &str) -> CollectionId {
    if current_database == DEFAULT_DATABASE {
        return CollectionId::new(collection);
    }

    CollectionId::new(Namespace::qualify_with_database(
        collection,
        current_database,
    ))
}

/// Shared state for the gRPC service.
///
/// Wraps an `AxonHandler` in a `Mutex` so multiple async tasks can call it.
pub struct AxonServiceImpl<S: StorageAdapter> {
    handler: Arc<Mutex<AxonHandler<S>>>,
    auth: AuthContext,
}

impl<S: StorageAdapter> AxonServiceImpl<S> {
    pub fn from_handler(handler: AxonHandler<S>) -> Self {
        Self::from_handler_with_auth(handler, AuthContext::no_auth())
    }

    pub fn from_handler_with_auth(handler: AxonHandler<S>, auth: AuthContext) -> Self {
        Self {
            handler: Arc::new(Mutex::new(handler)),
            auth,
        }
    }

    /// Create a service sharing an existing handler reference.
    ///
    /// Use this to share state between the gRPC service and the HTTP gateway.
    pub fn from_shared(handler: Arc<Mutex<AxonHandler<S>>>) -> Self {
        Self::from_shared_with_auth(handler, AuthContext::no_auth())
    }

    /// Create a service sharing an existing handler reference and auth policy.
    pub fn from_shared_with_auth(handler: Arc<Mutex<AxonHandler<S>>>, auth: AuthContext) -> Self {
        Self { handler, auth }
    }

    async fn authorize(&self, peer: Option<std::net::SocketAddr>) -> Result<Identity, Status> {
        self.auth.resolve_peer(peer).await.map_err(auth_to_status)
    }
}

impl AxonServiceImpl<MemoryStorageAdapter> {
    /// Create a service backed by an in-memory storage adapter.
    pub fn new_in_memory() -> Self {
        Self::from_handler(AxonHandler::new(MemoryStorageAdapter::default()))
    }
}

#[tonic::async_trait]
impl<S: StorageAdapter + 'static> AxonService for AxonServiceImpl<S> {
    async fn create_entity(
        &self,
        request: Request<ProtoCreateEntityReq>,
    ) -> Result<Response<ProtoCreateEntityResp>, Status> {
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let data: serde_json::Value = serde_json::from_str(&req.data_json)
            .map_err(|e| Status::invalid_argument(format!("invalid data_json: {e}")))?;

        let resp = self
            .handler
            .lock()
            .await
            .create_entity(CreateEntityRequest {
                collection: qualify_collection_name(&req.collection, &current_database),
                id: EntityId::new(&req.id),
                data,
                actor: Some(identity.actor),
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
        self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .get_entity(GetEntityRequest {
                collection: qualify_collection_name(&req.collection, &current_database),
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
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let data: serde_json::Value = serde_json::from_str(&req.data_json)
            .map_err(|e| Status::invalid_argument(format!("invalid data_json: {e}")))?;

        let resp = self
            .handler
            .lock()
            .await
            .update_entity(UpdateEntityRequest {
                collection: qualify_collection_name(&req.collection, &current_database),
                id: EntityId::new(&req.id),
                data,
                expected_version: req.expected_version,
                actor: Some(identity.actor),
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
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();

        let resp = self
            .handler
            .lock()
            .await
            .delete_entity(DeleteEntityRequest {
                collection: qualify_collection_name(&req.collection, &current_database),
                id: EntityId::new(&req.id),
                actor: Some(identity.actor),
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
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let metadata: serde_json::Value = if req.metadata_json.is_empty() {
            json!(null)
        } else {
            serde_json::from_str(&req.metadata_json)
                .map_err(|e| Status::invalid_argument(format!("invalid metadata_json: {e}")))?
        };

        let resp = self
            .handler
            .lock()
            .await
            .create_link(CreateLinkRequest {
                source_collection: qualify_collection_name(
                    &req.source_collection,
                    &current_database,
                ),
                source_id: EntityId::new(&req.source_id),
                target_collection: qualify_collection_name(
                    &req.target_collection,
                    &current_database,
                ),
                target_id: EntityId::new(&req.target_id),
                link_type: req.link_type.clone(),
                metadata,
                actor: Some(identity.actor),
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
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();

        let resp = self
            .handler
            .lock()
            .await
            .delete_link(DeleteLinkRequest {
                source_collection: qualify_collection_name(
                    &req.source_collection,
                    &current_database,
                ),
                source_id: EntityId::new(&req.source_id),
                target_collection: qualify_collection_name(
                    &req.target_collection,
                    &current_database,
                ),
                target_id: EntityId::new(&req.target_id),
                link_type: req.link_type.clone(),
                actor: Some(identity.actor),
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
        self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
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
                collection: qualify_collection_name(&req.collection, &current_database),
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
        self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let handler = self.handler.lock().await;
        let entries = handler
            .audit_log()
            .query_by_entity(
                &qualify_collection_name(&req.collection, &current_database),
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
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let mut tx = Transaction::new();

        for op in &req.operations {
            let result = match op.op.as_str() {
                "create" => {
                    let data: serde_json::Value = serde_json::from_str(&op.data_json)
                        .map_err(|e| Status::invalid_argument(format!("invalid data_json: {e}")))?;
                    tx.create(axon_core::types::Entity::new(
                        qualify_collection_name(&op.collection, &current_database),
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
                            collection: qualify_collection_name(&op.collection, &current_database),
                            id: EntityId::new(&op.id),
                        })
                        .ok()
                        .map(|r| r.entity.data);
                    drop(h);
                    tx.update(
                        axon_core::types::Entity::new(
                            qualify_collection_name(&op.collection, &current_database),
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
                            collection: qualify_collection_name(&op.collection, &current_database),
                            id: EntityId::new(&op.id),
                        })
                        .ok()
                        .map(|r| r.entity.data);
                    drop(h);
                    tx.delete(
                        qualify_collection_name(&op.collection, &current_database),
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
        let mut h = self.handler.lock().await;
        let (storage, audit) = h.storage_and_audit_mut();
        let written = tx
            .commit(storage, audit, Some(identity.actor))
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoCommitTxResp {
            transaction_id: tx_id,
            entities: written.into_iter().map(entity_to_proto).collect(),
        }))
    }

    async fn query_entities(
        &self,
        request: Request<ProtoQueryEntitiesReq>,
    ) -> Result<Response<ProtoQueryEntitiesResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
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
                collection: qualify_collection_name(&req.collection, &current_database),
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
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let mut schema: axon_schema::schema::CollectionSchema =
            serde_json::from_str(&req.schema_json)
                .map_err(|e| Status::invalid_argument(format!("invalid schema_json: {e}")))?;
        schema.collection = qualify_collection_name(schema.collection.as_str(), &current_database);

        let resp = self
            .handler
            .lock()
            .await
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: Some(identity.actor),
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
        self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .handle_get_schema(GetSchemaRequest {
                collection: qualify_collection_name(&req.collection, &current_database),
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
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let mut schema: axon_schema::schema::CollectionSchema =
            serde_json::from_str(&req.schema_json)
                .map_err(|e| Status::invalid_argument(format!("invalid schema_json: {e}")))?;
        let collection = qualify_collection_name(&req.name, &current_database);
        schema.collection = qualify_collection_name(schema.collection.as_str(), &current_database);

        let resp = self
            .handler
            .lock()
            .await
            .create_collection(CreateCollectionRequest {
                name: collection,
                schema,
                actor: Some(identity.actor),
            })
            .map_err(axon_to_status)?;

        Ok(Response::new(ProtoCreateCollectionResp { name: resp.name }))
    }

    async fn drop_collection(
        &self,
        request: Request<ProtoDropCollectionReq>,
    ) -> Result<Response<ProtoDropCollectionResp>, Status> {
        let identity = self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();

        let resp = self
            .handler
            .lock()
            .await
            .drop_collection(DropCollectionRequest {
                name: qualify_collection_name(&req.name, &current_database),
                actor: Some(identity.actor),
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
        request: Request<ProtoListCollectionsReq>,
    ) -> Result<Response<ProtoListCollectionsResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let requested_database = grpc_requested_database(&request).map(str::to_string);
        let handler = self.handler.lock().await;
        let collections = match requested_database {
            Some(database) => list_collections_for_database(&handler, &database),
            None => handler
                .list_collections(ListCollectionsRequest {})
                .map(|resp| resp.collections),
        }
        .map_err(axon_to_status)?
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
        self.authorize(request.remote_addr()).await?;
        let current_database = grpc_current_database(&request).to_string();
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .describe_collection(DescribeCollectionRequest {
                name: qualify_collection_name(&req.name, &current_database),
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

    async fn create_database(
        &self,
        request: Request<ProtoCreateDatabaseReq>,
    ) -> Result<Response<ProtoCreateDatabaseResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .create_database(CreateDatabaseRequest { name: req.name })
            .map_err(axon_to_status)?;
        Ok(Response::new(ProtoCreateDatabaseResp { name: resp.name }))
    }

    async fn list_databases(
        &self,
        request: Request<ProtoListDatabasesReq>,
    ) -> Result<Response<ProtoListDatabasesResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let resp = self
            .handler
            .lock()
            .await
            .list_databases(ListDatabasesRequest {})
            .map_err(axon_to_status)?;
        Ok(Response::new(ProtoListDatabasesResp {
            databases: resp.databases,
        }))
    }

    async fn drop_database(
        &self,
        request: Request<ProtoDropDatabaseReq>,
    ) -> Result<Response<ProtoDropDatabaseResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .drop_database(DropDatabaseRequest {
                name: req.name,
                force: req.force,
            })
            .map_err(axon_to_status)?;
        Ok(Response::new(ProtoDropDatabaseResp {
            name: resp.name,
            collections_removed: resp.collections_removed as u64,
        }))
    }

    async fn create_namespace(
        &self,
        request: Request<ProtoCreateNamespaceReq>,
    ) -> Result<Response<ProtoCreateNamespaceResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .create_namespace(CreateNamespaceRequest {
                database: req.database,
                schema: req.schema,
            })
            .map_err(axon_to_status)?;
        Ok(Response::new(ProtoCreateNamespaceResp {
            database: resp.database,
            schema: resp.schema,
        }))
    }

    async fn list_namespaces(
        &self,
        request: Request<ProtoListNamespacesReq>,
    ) -> Result<Response<ProtoListNamespacesResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .list_namespaces(ListNamespacesRequest {
                database: req.database,
            })
            .map_err(axon_to_status)?;
        Ok(Response::new(ProtoListNamespacesResp {
            database: resp.database,
            schemas: resp.schemas,
        }))
    }

    async fn list_namespace_collections(
        &self,
        request: Request<ProtoListNamespaceCollectionsReq>,
    ) -> Result<Response<ProtoListNamespaceCollectionsResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .list_namespace_collections(ListNamespaceCollectionsRequest {
                database: req.database,
                schema: req.schema,
            })
            .map_err(axon_to_status)?;
        Ok(Response::new(ProtoListNamespaceCollectionsResp {
            database: resp.database,
            schema: resp.schema,
            collections: resp.collections,
        }))
    }

    async fn drop_namespace(
        &self,
        request: Request<ProtoDropNamespaceReq>,
    ) -> Result<Response<ProtoDropNamespaceResp>, Status> {
        self.authorize(request.remote_addr()).await?;
        let req = request.into_inner();
        let resp = self
            .handler
            .lock()
            .await
            .drop_namespace(DropNamespaceRequest {
                database: req.database,
                schema: req.schema,
                force: req.force,
            })
            .map_err(axon_to_status)?;
        Ok(Response::new(ProtoDropNamespaceResp {
            database: resp.database,
            schema: resp.schema,
            collections_removed: resp.collections_removed as u64,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use super::*;
    use crate::auth::{
        AuthContext, AuthError, AuthMode, Role, TailscaleWhoisProvider, TailscaleWhoisResponse,
    };
    use axon_storage::MemoryStorageAdapter;
    use serde_json::json;
    use tonic::metadata::MetadataValue;
    use tonic::transport::server::TcpConnectInfo;
    use tonic::Code;

    struct FakeWhoisProvider {
        results: StdMutex<HashMap<SocketAddr, Result<TailscaleWhoisResponse, AuthError>>>,
    }

    impl FakeWhoisProvider {
        fn with_result(
            peer: SocketAddr,
            result: Result<TailscaleWhoisResponse, AuthError>,
        ) -> Self {
            let mut results = HashMap::new();
            results.insert(peer, result);
            Self {
                results: StdMutex::new(results),
            }
        }
    }

    impl TailscaleWhoisProvider for FakeWhoisProvider {
        fn verify(&self) -> Pin<Box<dyn Future<Output = Result<(), AuthError>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn whois(
            &self,
            address: SocketAddr,
        ) -> Pin<Box<dyn Future<Output = Result<TailscaleWhoisResponse, AuthError>> + Send + '_>>
        {
            Box::pin(async move {
                let results = match self.results.lock() {
                    Ok(results) => results,
                    Err(poisoned) => poisoned.into_inner(),
                };
                results.get(&address).cloned().unwrap_or_else(|| {
                    Err(AuthError::Unauthorized(
                        "peer is not a recognized tailnet address".into(),
                    ))
                })
            })
        }
    }

    fn request_with_peer<T>(message: T, peer: SocketAddr) -> Request<T> {
        let mut request = Request::new(message);
        request.extensions_mut().insert(TcpConnectInfo {
            local_addr: None,
            remote_addr: Some(peer),
        });
        request
    }

    fn request_with_database<T>(message: T, database: &str) -> Request<T> {
        let mut request = Request::new(message);
        request.metadata_mut().insert(
            AXON_DATABASE_HEADER,
            MetadataValue::try_from(database).expect("database metadata must be valid"),
        );
        request
    }

    /// Build a service instance and create one entity in collection `col` with id `id`.
    async fn make_service_with_entity(
        col: &str,
        id: &str,
    ) -> AxonServiceImpl<MemoryStorageAdapter> {
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

    #[tokio::test]
    async fn grpc_database_and_namespace_round_trip() {
        let svc = AxonServiceImpl::new_in_memory();

        let created = svc
            .create_database(Request::new(ProtoCreateDatabaseReq {
                name: "prod".to_string(),
            }))
            .await
            .expect("database create should succeed")
            .into_inner();
        assert_eq!(created.name, "prod");

        let listed = svc
            .list_databases(Request::new(ProtoListDatabasesReq {}))
            .await
            .expect("database list should succeed")
            .into_inner();
        assert!(listed.databases.iter().any(|database| database == "prod"));

        let namespace = svc
            .create_namespace(Request::new(ProtoCreateNamespaceReq {
                database: "prod".to_string(),
                schema: "billing".to_string(),
            }))
            .await
            .expect("namespace create should succeed")
            .into_inner();
        assert_eq!(namespace.database, "prod");
        assert_eq!(namespace.schema, "billing");

        let namespaces = svc
            .list_namespaces(Request::new(ProtoListNamespacesReq {
                database: "prod".to_string(),
            }))
            .await
            .expect("namespace list should succeed")
            .into_inner();
        assert!(namespaces.schemas.iter().any(|schema| schema == "billing"));
    }

    #[tokio::test]
    async fn grpc_metadata_current_database_routes_unqualified_collection_operations() {
        let svc = AxonServiceImpl::new_in_memory();
        let default_schema = serde_json::to_string(&axon_schema::schema::CollectionSchema::new(
            CollectionId::new("tasks"),
        ))
        .expect("default schema should serialize");

        svc.create_collection(Request::new(ProtoCreateCollectionReq {
            name: "tasks".into(),
            schema_json: default_schema.clone(),
            actor: String::new(),
        }))
        .await
        .expect("default collection create should succeed");

        svc.create_database(Request::new(ProtoCreateDatabaseReq {
            name: "prod".into(),
        }))
        .await
        .expect("database create should succeed");

        svc.create_collection(request_with_database(
            ProtoCreateCollectionReq {
                name: "tasks".into(),
                schema_json: default_schema,
                actor: String::new(),
            },
            "prod",
        ))
        .await
        .expect("prod collection create should succeed");

        svc.create_entity(Request::new(ProtoCreateEntityReq {
            collection: "tasks".into(),
            id: "t-001".into(),
            data_json: json!({"scope": "default"}).to_string(),
            actor: String::new(),
        }))
        .await
        .expect("default entity create should succeed");

        svc.create_entity(request_with_database(
            ProtoCreateEntityReq {
                collection: "tasks".into(),
                id: "t-001".into(),
                data_json: json!({"scope": "prod"}).to_string(),
                actor: String::new(),
            },
            "prod",
        ))
        .await
        .expect("prod entity create should succeed");

        let default_entity = svc
            .get_entity(Request::new(ProtoGetEntityReq {
                collection: "tasks".into(),
                id: "t-001".into(),
            }))
            .await
            .expect("default entity get should succeed")
            .into_inner()
            .entity
            .expect("default entity should be present");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&default_entity.data_json)
                .expect("default entity JSON should parse")["scope"],
            "default"
        );

        let prod_entity = svc
            .get_entity(request_with_database(
                ProtoGetEntityReq {
                    collection: "tasks".into(),
                    id: "t-001".into(),
                },
                "prod",
            ))
            .await
            .expect("prod entity get should succeed")
            .into_inner()
            .entity
            .expect("prod entity should be present");
        assert_eq!(prod_entity.collection, "prod.default.tasks");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&prod_entity.data_json)
                .expect("prod entity JSON should parse")["scope"],
            "prod"
        );
    }

    #[tokio::test]
    async fn grpc_list_collections_scopes_to_metadata_database_only_when_present() {
        let svc = AxonServiceImpl::new_in_memory();
        let default_schema = serde_json::to_string(&axon_schema::schema::CollectionSchema::new(
            CollectionId::new("tasks"),
        ))
        .expect("default schema should serialize");

        svc.create_collection(Request::new(ProtoCreateCollectionReq {
            name: "tasks".into(),
            schema_json: default_schema.clone(),
            actor: String::new(),
        }))
        .await
        .expect("default collection create should succeed");

        svc.create_database(Request::new(ProtoCreateDatabaseReq {
            name: "prod".into(),
        }))
        .await
        .expect("database create should succeed");

        svc.create_collection(request_with_database(
            ProtoCreateCollectionReq {
                name: "tasks".into(),
                schema_json: default_schema,
                actor: String::new(),
            },
            "prod",
        ))
        .await
        .expect("prod collection create should succeed");

        let global = svc
            .list_collections(Request::new(ProtoListCollectionsReq {}))
            .await
            .expect("global list_collections should succeed")
            .into_inner();
        assert_eq!(global.collections.len(), 2);
        assert_eq!(global.collections[0].name, "tasks");
        assert_eq!(global.collections[1].name, "tasks");

        let prod = svc
            .list_collections(request_with_database(ProtoListCollectionsReq {}, "prod"))
            .await
            .expect("prod-scoped list_collections should succeed")
            .into_inner();
        assert_eq!(prod.collections.len(), 1);
        assert_eq!(prod.collections[0].name, "prod.default.tasks");
    }

    #[tokio::test]
    async fn grpc_tailscale_identity_overrides_body_actor_in_audit() {
        let peer = SocketAddr::from(([100, 64, 0, 11], 50051));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(FakeWhoisProvider::with_result(
                peer,
                Ok(TailscaleWhoisResponse {
                    node_name: "grpc-agent".into(),
                    user_login: "agent@example.com".into(),
                    tags: vec!["tag:axon-write".into()],
                }),
            )),
            Duration::from_secs(60),
        );
        let svc = AxonServiceImpl::from_handler_with_auth(
            AxonHandler::new(MemoryStorageAdapter::default()),
            auth,
        );

        svc.create_entity(request_with_peer(
            ProtoCreateEntityReq {
                collection: "tasks".into(),
                id: "t-001".into(),
                data_json: json!({"title": "hello"}).to_string(),
                actor: "spoofed".into(),
            },
            peer,
        ))
        .await
        .expect("create should succeed");

        let resp = svc
            .query_audit_by_entity(request_with_peer(
                QueryAuditByEntityRequest {
                    collection: "tasks".into(),
                    entity_id: "t-001".into(),
                },
                peer,
            ))
            .await
            .expect("audit query should succeed")
            .into_inner();

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].actor, "grpc-agent");
    }

    #[tokio::test]
    async fn grpc_tailscale_rejects_non_tailnet_peer() {
        let peer = SocketAddr::from(([127, 0, 0, 1], 50051));
        let auth = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(FakeWhoisProvider::with_result(
                peer,
                Err(AuthError::Unauthorized(
                    "peer is not a recognized tailnet address".into(),
                )),
            )),
            Duration::from_secs(60),
        );
        let svc = AxonServiceImpl::from_handler_with_auth(
            AxonHandler::new(MemoryStorageAdapter::default()),
            auth,
        );

        let error = svc
            .get_entity(request_with_peer(
                ProtoGetEntityReq {
                    collection: "tasks".into(),
                    id: "t-001".into(),
                },
                peer,
            ))
            .await
            .expect_err("non-tailnet peers must be rejected");

        assert_eq!(error.code(), Code::Unauthenticated);
    }
}
