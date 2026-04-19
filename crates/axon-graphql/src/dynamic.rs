//! Dynamic GraphQL schema builder from Axon collections.
//!
//! Generates a full GraphQL schema (queries + mutations + introspection)
//! from the set of registered collections and their entity schemas.
//!
//! When a shared `AxonHandler` is provided via [`build_schema_with_handler`],
//! resolvers delegate to the live handler for real CRUD operations. The
//! plain [`build_schema`] function builds a stub schema (useful for SDL
//! inspection and tests).

use std::sync::Arc;

use async_graphql::dynamic::{
    Field, FieldFuture, FieldValue, InputObject, InputValue, Object, Scalar, Schema, SchemaBuilder,
    Subscription, SubscriptionField, SubscriptionFieldFuture, TypeRef,
};
use async_graphql::futures_util::StreamExt;
use async_graphql::{Error as GqlError, ErrorExtensions, Value as GqlValue};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::subscriptions::BroadcastBroker;

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, DeleteLinkRequest,
    DescribeCollectionRequest, FieldFilter, FilterNode, FilterOp, GateFilter, GetEntityRequest,
    ListCollectionsRequest, PatchEntityRequest, QueryAuditRequest, QueryEntitiesRequest,
    SortDirection, SortField, TransitionLifecycleRequest, UpdateEntityRequest,
};
use axon_core::auth::CallerIdentity;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_schema::schema::CollectionSchema;
use axon_storage::adapter::StorageAdapter;

use crate::types::extract_fields;

/// Shared handle to an `AxonHandler` behind a `tokio::sync::Mutex`.
pub type SharedHandler<S> = Arc<Mutex<AxonHandler<S>>>;

/// Wrapper around the dynamically generated `async-graphql` schema.
pub struct AxonSchema {
    pub schema: Schema,
}

const FILTER_INPUT: &str = "AxonFilterInput";
const SORT_INPUT: &str = "AxonSortInput";
const ENTITY_TYPE: &str = "Entity";
const ENTITY_EDGE_TYPE: &str = "EntityEdge";
const ENTITY_CONNECTION_TYPE: &str = "EntityConnection";
const PAGE_INFO_TYPE: &str = "PageInfo";
const COLLECTION_META_TYPE: &str = "CollectionMeta";
const AUDIT_ENTRY_TYPE: &str = "AuditEntry";
const AUDIT_EDGE_TYPE: &str = "AuditEdge";
const AUDIT_CONNECTION_TYPE: &str = "AuditConnection";
const DEFAULT_MAX_GRAPHQL_DEPTH: usize = 10;
const DEFAULT_MAX_GRAPHQL_COMPLEXITY: usize = 256;
const MAX_DEPTH_ENV: &str = "AXON_GRAPHQL_MAX_DEPTH";
const MAX_COMPLEXITY_ENV: &str = "AXON_GRAPHQL_MAX_COMPLEXITY";

// ── Entity → GraphQL FieldValue conversion ──────────────────────────────────

/// Convert an `Entity` into an `async-graphql` `FieldValue` that the dynamic
/// object type can resolve field-by-field.
fn entity_to_field_value(entity: &Entity) -> FieldValue<'static> {
    json_to_field_value(entity_to_typed_json(entity))
}

fn entity_to_typed_json(entity: &Entity) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("id".into(), Value::String(entity.id.to_string()));
    map.insert("version".into(), json!(entity.version));
    if let Some(ns) = entity.created_at_ns {
        map.insert("createdAt".into(), Value::String(format_ns(ns)));
    }
    if let Some(ns) = entity.updated_at_ns {
        map.insert("updatedAt".into(), Value::String(format_ns(ns)));
    }
    // Merge user data fields.
    if let Value::Object(data) = &entity.data {
        for (k, v) in data {
            map.insert(k.clone(), v.clone());
        }
    }

    Value::Object(map)
}

fn entity_to_generic_json(entity: &Entity) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("id".into(), Value::String(entity.id.to_string()));
    map.insert(
        "collection".into(),
        Value::String(entity.collection.to_string()),
    );
    map.insert("version".into(), json!(entity.version));
    map.insert("data".into(), entity.data.clone());
    if let Some(ns) = entity.created_at_ns {
        map.insert("createdAt".into(), Value::String(format_ns(ns)));
    }
    if let Some(ns) = entity.updated_at_ns {
        map.insert("updatedAt".into(), Value::String(format_ns(ns)));
    }
    Value::Object(map)
}

fn json_to_field_value(value: Value) -> FieldValue<'static> {
    FieldValue::from(GqlValue::from_json(value).unwrap_or(GqlValue::Null))
}

fn parent_json_field(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    name: &str,
) -> Option<FieldValue<'static>> {
    match ctx.parent_value.try_to_value() {
        Ok(GqlValue::Object(map)) => map
            .get(&async_graphql::Name::new(name))
            .map(|value| FieldValue::from(value.clone())),
        _ => Some(FieldValue::NULL),
    }
}

fn json_object_field(name: &'static str, ty: TypeRef) -> Field {
    Field::new(name, ty, move |ctx| {
        FieldFuture::new(async move { Ok(parent_json_field(ctx, name)) })
    })
}

fn filter_input_object() -> InputObject {
    InputObject::new(FILTER_INPUT)
        .description(
            "Composable Axon entity filter. Use field/op/value for field predicates, gate/pass for gate predicates, or and/or for nested boolean filters.",
        )
        .field(InputValue::new("field", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new("op", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new("value", TypeRef::named("JSON")))
        .field(InputValue::new("gate", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new("pass", TypeRef::named(TypeRef::BOOLEAN)))
        .field(InputValue::new("and", TypeRef::named_nn_list(FILTER_INPUT)))
        .field(InputValue::new("or", TypeRef::named_nn_list(FILTER_INPUT)))
}

fn sort_input_object() -> InputObject {
    InputObject::new(SORT_INPUT)
        .description("Axon entity sort field. Direction defaults to asc.")
        .field(InputValue::new("field", TypeRef::named_nn(TypeRef::STRING)))
        .field(InputValue::new(
            "direction",
            TypeRef::named(TypeRef::STRING),
        ))
}

fn gql_input_to_json(value: &GqlValue) -> Result<Value, GqlError> {
    value
        .clone()
        .into_json()
        .map_err(|e| GqlError::new(format!("invalid GraphQL input value: {e}")))
}

fn parse_graphql_filter_arg(value: &GqlValue) -> Result<FilterNode, GqlError> {
    parse_graphql_filter_json(&gql_input_to_json(value)?)
}

fn parse_graphql_filter_json(value: &Value) -> Result<FilterNode, GqlError> {
    let obj = value
        .as_object()
        .ok_or_else(|| GqlError::new("filter must be an object"))?;

    if let Some(filters) = obj.get("and") {
        return Ok(FilterNode::And {
            filters: parse_graphql_filter_list(filters, "and")?,
        });
    }

    if let Some(filters) = obj.get("or") {
        return Ok(FilterNode::Or {
            filters: parse_graphql_filter_list(filters, "or")?,
        });
    }

    if let Some(gate) = obj.get("gate") {
        let gate = gate
            .as_str()
            .ok_or_else(|| GqlError::new("filter.gate must be a string"))?
            .to_owned();
        let pass = obj.get("pass").and_then(Value::as_bool).unwrap_or(true);
        return Ok(FilterNode::Gate(GateFilter { gate, pass }));
    }

    let field = obj
        .get("field")
        .and_then(Value::as_str)
        .ok_or_else(|| GqlError::new("field filters require a string field"))?
        .to_owned();
    let op = obj
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or("eq")
        .to_ascii_lowercase();
    let value = obj.get("value").cloned().unwrap_or(Value::Null);

    let (op, value) = match op.as_str() {
        "eq" => (FilterOp::Eq, value),
        "ne" | "neq" | "not_eq" => (FilterOp::Ne, value),
        "gt" => (FilterOp::Gt, value),
        "gte" => (FilterOp::Gte, value),
        "lt" => (FilterOp::Lt, value),
        "lte" => (FilterOp::Lte, value),
        "in" => (FilterOp::In, value),
        "contains" => (FilterOp::Contains, value),
        "is_null" => (FilterOp::Eq, Value::Null),
        "is_not_null" => (FilterOp::Ne, Value::Null),
        _ => {
            return Err(GqlError::new(format!("unsupported filter operator '{op}'")));
        }
    };

    Ok(FilterNode::Field(FieldFilter { field, op, value }))
}

fn parse_graphql_filter_list(value: &Value, name: &str) -> Result<Vec<FilterNode>, GqlError> {
    let items = value
        .as_array()
        .ok_or_else(|| GqlError::new(format!("filter.{name} must be a list")))?;
    items.iter().map(parse_graphql_filter_json).collect()
}

fn parse_graphql_sort_arg(value: &GqlValue) -> Result<Vec<SortField>, GqlError> {
    let json = gql_input_to_json(value)?;
    let items = json
        .as_array()
        .ok_or_else(|| GqlError::new("sort must be a list"))?;

    items
        .iter()
        .map(|item| {
            let obj = item
                .as_object()
                .ok_or_else(|| GqlError::new("sort entries must be objects"))?;
            let field = obj
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| GqlError::new("sort entries require a string field"))?
                .to_owned();
            let direction = match obj
                .get("direction")
                .and_then(Value::as_str)
                .unwrap_or("asc")
                .to_ascii_lowercase()
                .as_str()
            {
                "asc" => SortDirection::Asc,
                "desc" => SortDirection::Desc,
                other => {
                    return Err(GqlError::new(format!(
                        "unsupported sort direction '{other}'"
                    )));
                }
            };
            Ok(SortField { field, direction })
        })
        .collect()
}

fn graphql_limit_from_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn max_graphql_depth() -> usize {
    graphql_limit_from_env(MAX_DEPTH_ENV, DEFAULT_MAX_GRAPHQL_DEPTH)
}

fn max_graphql_complexity() -> usize {
    graphql_limit_from_env(MAX_COMPLEXITY_ENV, DEFAULT_MAX_GRAPHQL_COMPLEXITY)
}

fn parse_optional_u64_arg(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
    name: &str,
) -> Result<Option<u64>, GqlError> {
    match ctx.args.try_get(name) {
        Ok(value) => value
            .string()
            .map_err(|_| GqlError::new(format!("{name} must be a stringified unsigned integer")))?
            .parse::<u64>()
            .map(Some)
            .map_err(|e| GqlError::new(format!("invalid {name}: {e}"))),
        Err(_) => Ok(None),
    }
}

fn page_info_json(
    start_cursor: Option<String>,
    end_cursor: Option<String>,
    has_next_page: bool,
    has_previous_page: bool,
) -> Value {
    json!({
        "hasNextPage": has_next_page,
        "hasPreviousPage": has_previous_page,
        "startCursor": start_cursor,
        "endCursor": end_cursor,
    })
}

fn entity_connection_value(
    entities: &[Entity],
    total_count: usize,
    next_cursor: Option<String>,
    has_previous_page: bool,
    generic_node: bool,
) -> FieldValue<'static> {
    let edges: Vec<Value> = entities
        .iter()
        .map(|entity| {
            json!({
                "cursor": entity.id.to_string(),
                "node": if generic_node {
                    entity_to_generic_json(entity)
                } else {
                    entity_to_typed_json(entity)
                },
            })
        })
        .collect();
    let start_cursor = entities.first().map(|entity| entity.id.to_string());
    let end_cursor = entities.last().map(|entity| entity.id.to_string());

    json_to_field_value(json!({
        "edges": edges,
        "pageInfo": page_info_json(
            start_cursor,
            end_cursor,
            next_cursor.is_some(),
            has_previous_page,
        ),
        "totalCount": total_count,
    }))
}

fn collection_meta_json(
    meta: &axon_api::response::CollectionMetadata,
    schema: Option<CollectionSchema>,
) -> Value {
    json!({
        "name": meta.name,
        "entityCount": meta.entity_count,
        "schemaVersion": meta.schema_version,
        "createdAt": meta.created_at_ns.map(format_ns),
        "updatedAt": meta.updated_at_ns.map(format_ns),
        "schema": schema,
    })
}

fn described_collection_json(
    description: &axon_api::response::DescribeCollectionResponse,
) -> Value {
    json!({
        "name": description.name,
        "entityCount": description.entity_count,
        "schemaVersion": description.schema.as_ref().map(|schema| schema.version),
        "createdAt": description.created_at_ns.map(format_ns),
        "updatedAt": description.updated_at_ns.map(format_ns),
        "schema": description.schema,
    })
}

fn audit_entry_json(entry: &axon_audit::AuditEntry) -> Value {
    json!({
        "id": entry.id.to_string(),
        "timestampNs": entry.timestamp_ns.to_string(),
        "collection": entry.collection.to_string(),
        "entityId": entry.entity_id.to_string(),
        "version": entry.version,
        "mutation": entry.mutation.to_string(),
        "dataBefore": entry.data_before,
        "dataAfter": entry.data_after,
        "actor": entry.actor,
        "metadata": entry.metadata,
        "transactionId": entry.transaction_id,
    })
}

fn audit_connection_value(
    entries: &[axon_audit::AuditEntry],
    next_cursor: Option<u64>,
    has_previous_page: bool,
) -> FieldValue<'static> {
    let edges: Vec<Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "cursor": entry.id.to_string(),
                "node": audit_entry_json(entry),
            })
        })
        .collect();
    let start_cursor = entries.first().map(|entry| entry.id.to_string());
    let end_cursor = entries.last().map(|entry| entry.id.to_string());

    json_to_field_value(json!({
        "edges": edges,
        "pageInfo": page_info_json(
            start_cursor,
            end_cursor,
            next_cursor.is_some(),
            has_previous_page,
        ),
        "totalCount": entries.len(),
    }))
}

fn page_info_object() -> Object {
    Object::new(PAGE_INFO_TYPE)
        .field(json_object_field(
            "hasNextPage",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "hasPreviousPage",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "startCursor",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "endCursor",
            TypeRef::named(TypeRef::STRING),
        ))
}

fn generic_entity_object() -> Object {
    Object::new(ENTITY_TYPE)
        .field(json_object_field("id", TypeRef::named_nn(TypeRef::ID)))
        .field(json_object_field(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "version",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field("data", TypeRef::named("JSON")))
        .field(json_object_field(
            "createdAt",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "updatedAt",
            TypeRef::named(TypeRef::STRING),
        ))
}

fn entity_edge_object() -> Object {
    Object::new(ENTITY_EDGE_TYPE)
        .field(json_object_field(
            "cursor",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("node", TypeRef::named_nn(ENTITY_TYPE)))
}

fn entity_connection_object() -> Object {
    Object::new(ENTITY_CONNECTION_TYPE)
        .field(json_object_field(
            "edges",
            TypeRef::named_nn_list_nn(ENTITY_EDGE_TYPE),
        ))
        .field(json_object_field(
            "pageInfo",
            TypeRef::named_nn(PAGE_INFO_TYPE),
        ))
        .field(json_object_field(
            "totalCount",
            TypeRef::named_nn(TypeRef::INT),
        ))
}

fn typed_edge_object(edge_type: &str, node_type: &str) -> Object {
    Object::new(edge_type)
        .field(json_object_field(
            "cursor",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("node", TypeRef::named_nn(node_type)))
}

fn typed_connection_object(connection_type: &str, edge_type: &str) -> Object {
    Object::new(connection_type)
        .field(json_object_field(
            "edges",
            TypeRef::named_nn_list_nn(edge_type),
        ))
        .field(json_object_field(
            "pageInfo",
            TypeRef::named_nn(PAGE_INFO_TYPE),
        ))
        .field(json_object_field(
            "totalCount",
            TypeRef::named_nn(TypeRef::INT),
        ))
}

fn collection_meta_object() -> Object {
    Object::new(COLLECTION_META_TYPE)
        .field(json_object_field(
            "name",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "entityCount",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "schemaVersion",
            TypeRef::named(TypeRef::INT),
        ))
        .field(json_object_field(
            "createdAt",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "updatedAt",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field("schema", TypeRef::named("JSON")))
}

fn audit_entry_object() -> Object {
    Object::new(AUDIT_ENTRY_TYPE)
        .field(json_object_field("id", TypeRef::named_nn(TypeRef::ID)))
        .field(json_object_field(
            "timestampNs",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "entityId",
            TypeRef::named_nn(TypeRef::ID),
        ))
        .field(json_object_field(
            "version",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "mutation",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("dataBefore", TypeRef::named("JSON")))
        .field(json_object_field("dataAfter", TypeRef::named("JSON")))
        .field(json_object_field(
            "actor",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("metadata", TypeRef::named("JSON")))
        .field(json_object_field(
            "transactionId",
            TypeRef::named(TypeRef::STRING),
        ))
}

fn audit_edge_object() -> Object {
    Object::new(AUDIT_EDGE_TYPE)
        .field(json_object_field(
            "cursor",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "node",
            TypeRef::named_nn(AUDIT_ENTRY_TYPE),
        ))
}

fn audit_connection_object() -> Object {
    Object::new(AUDIT_CONNECTION_TYPE)
        .field(json_object_field(
            "edges",
            TypeRef::named_nn_list_nn(AUDIT_EDGE_TYPE),
        ))
        .field(json_object_field(
            "pageInfo",
            TypeRef::named_nn(PAGE_INFO_TYPE),
        ))
        .field(json_object_field(
            "totalCount",
            TypeRef::named_nn(TypeRef::INT),
        ))
}

fn register_root_objects(mut schema_builder: SchemaBuilder) -> SchemaBuilder {
    schema_builder = schema_builder
        .register(page_info_object())
        .register(generic_entity_object())
        .register(entity_edge_object())
        .register(entity_connection_object())
        .register(collection_meta_object())
        .register(audit_entry_object())
        .register(audit_edge_object())
        .register(audit_connection_object());
    schema_builder
}

fn add_handler_root_query_fields<S: StorageAdapter + 'static>(
    mut query: Object,
    handler: SharedHandler<S>,
) -> Object {
    let handler_get = Arc::clone(&handler);
    query = query.field(
        Field::new("entity", TypeRef::named(ENTITY_TYPE), move |ctx| {
            let handler = Arc::clone(&handler_get);
            FieldFuture::new(async move {
                let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                let id = ctx.args.try_get("id")?.string()?.to_owned();
                let guard = handler.lock().await;
                match guard.get_entity(GetEntityRequest {
                    collection: CollectionId::new(collection),
                    id: EntityId::new(id),
                }) {
                    Ok(resp) => Ok(Some(json_to_field_value(entity_to_generic_json(
                        &resp.entity,
                    )))),
                    Err(AxonError::NotFound(_)) => Ok(None),
                    Err(e) => Err(axon_error_to_gql(e)),
                }
            })
        })
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID))),
    );

    let handler_entities = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "entities",
            TypeRef::named_nn(ENTITY_CONNECTION_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_entities);
                FieldFuture::new(async move {
                    let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                    let limit = ctx
                        .args
                        .try_get("limit")
                        .ok()
                        .and_then(|v| v.i64().ok())
                        .map(|v| v as usize);
                    let after_id = ctx
                        .args
                        .try_get("after")
                        .ok()
                        .and_then(|v| v.string().ok())
                        .map(EntityId::new);
                    let filter = ctx
                        .args
                        .try_get("filter")
                        .ok()
                        .map(|v| parse_graphql_filter_arg(v.as_value()))
                        .transpose()?;
                    let sort = ctx
                        .args
                        .try_get("sort")
                        .ok()
                        .map(|v| parse_graphql_sort_arg(v.as_value()))
                        .transpose()?
                        .unwrap_or_default();
                    let has_previous_page = after_id.is_some();

                    let guard = handler.lock().await;
                    match guard.query_entities(QueryEntitiesRequest {
                        collection: CollectionId::new(collection),
                        filter,
                        sort,
                        limit,
                        after_id,
                        count_only: false,
                    }) {
                        Ok(resp) => Ok(Some(entity_connection_value(
                            &resp.entities,
                            resp.total_count,
                            resp.next_cursor,
                            has_previous_page,
                            true,
                        ))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)))
        .argument(InputValue::new("sort", TypeRef::named_nn_list(SORT_INPUT)))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::ID))),
    );

    let handler_collections = Arc::clone(&handler);
    query = query.field(Field::new(
        "collections",
        TypeRef::named_nn_list_nn(COLLECTION_META_TYPE),
        move |_ctx| {
            let handler = Arc::clone(&handler_collections);
            FieldFuture::new(async move {
                let guard = handler.lock().await;
                match guard.list_collections(ListCollectionsRequest {}) {
                    Ok(resp) => {
                        let values: Vec<FieldValue> = resp
                            .collections
                            .iter()
                            .map(|meta| json_to_field_value(collection_meta_json(meta, None)))
                            .collect();
                        Ok(Some(FieldValue::list(values)))
                    }
                    Err(e) => Err(axon_error_to_gql(e)),
                }
            })
        },
    ));

    let handler_collection = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "collection",
            TypeRef::named(COLLECTION_META_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_collection);
                FieldFuture::new(async move {
                    let name = ctx.args.try_get("name")?.string()?.to_owned();
                    let guard = handler.lock().await;
                    match guard.describe_collection(DescribeCollectionRequest {
                        name: CollectionId::new(name),
                    }) {
                        Ok(resp) => Ok(Some(json_to_field_value(described_collection_json(&resp)))),
                        Err(AxonError::NotFound(_)) => Ok(None),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("name", TypeRef::named_nn(TypeRef::STRING))),
    );

    let handler_audit = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "auditLog",
            TypeRef::named_nn(AUDIT_CONNECTION_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_audit);
                FieldFuture::new(async move {
                    let collection = ctx
                        .args
                        .try_get("collection")
                        .ok()
                        .and_then(|v| v.string().ok())
                        .map(CollectionId::new);
                    let entity_id = ctx
                        .args
                        .try_get("entityId")
                        .ok()
                        .and_then(|v| v.string().ok())
                        .map(EntityId::new);
                    let actor = ctx
                        .args
                        .try_get("actor")
                        .ok()
                        .and_then(|v| v.string().ok())
                        .map(ToOwned::to_owned);
                    let operation = ctx
                        .args
                        .try_get("operation")
                        .ok()
                        .and_then(|v| v.string().ok())
                        .map(ToOwned::to_owned);
                    let since_ns = parse_optional_u64_arg(&ctx, "sinceNs")?;
                    let until_ns = parse_optional_u64_arg(&ctx, "untilNs")?;
                    let after_id = parse_optional_u64_arg(&ctx, "after")?;
                    let limit = ctx
                        .args
                        .try_get("limit")
                        .ok()
                        .and_then(|v| v.i64().ok())
                        .map(|v| v as usize);
                    let has_previous_page = after_id.is_some();

                    let guard = handler.lock().await;
                    match guard.query_audit(QueryAuditRequest {
                        database: None,
                        collection,
                        collection_ids: Vec::new(),
                        entity_id,
                        actor,
                        operation,
                        since_ns,
                        until_ns,
                        after_id,
                        limit,
                    }) {
                        Ok(resp) => Ok(Some(audit_connection_value(
                            &resp.entries,
                            resp.next_cursor,
                            has_previous_page,
                        ))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new("entityId", TypeRef::named(TypeRef::ID)))
        .argument(InputValue::new("actor", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new(
            "operation",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new("sinceNs", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("untilNs", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT))),
    );

    query
}

fn add_stub_root_query_fields(mut query: Object) -> Object {
    query = query.field(
        Field::new("entity", TypeRef::named(ENTITY_TYPE), |_ctx| {
            FieldFuture::new(async move { Ok(None::<FieldValue>) })
        })
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID))),
    );
    query = query.field(
        Field::new(
            "entities",
            TypeRef::named_nn(ENTITY_CONNECTION_TYPE),
            |_ctx| {
                FieldFuture::new(async move {
                    Ok(Some(entity_connection_value(&[], 0, None, false, true)))
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)))
        .argument(InputValue::new("sort", TypeRef::named_nn_list(SORT_INPUT)))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::ID))),
    );
    query = query.field(Field::new(
        "collections",
        TypeRef::named_nn_list_nn(COLLECTION_META_TYPE),
        |_ctx| {
            FieldFuture::new(async move { Ok(Some(FieldValue::list(Vec::<FieldValue>::new()))) })
        },
    ));
    query = query.field(
        Field::new("collection", TypeRef::named(COLLECTION_META_TYPE), |_ctx| {
            FieldFuture::new(async move { Ok(None::<FieldValue>) })
        })
        .argument(InputValue::new("name", TypeRef::named_nn(TypeRef::STRING))),
    );
    query = query.field(
        Field::new(
            "auditLog",
            TypeRef::named_nn(AUDIT_CONNECTION_TYPE),
            |_ctx| {
                FieldFuture::new(async move { Ok(Some(audit_connection_value(&[], None, false))) })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new("entityId", TypeRef::named(TypeRef::ID)))
        .argument(InputValue::new("actor", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new(
            "operation",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new("sinceNs", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("untilNs", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT))),
    );
    query
}

/// Format nanosecond timestamp as ISO-8601 string.
fn format_ns(ns: u64) -> String {
    let secs = (ns / 1_000_000_000) as i64;
    let nanos = (ns % 1_000_000_000) as u32;
    time_from_epoch(secs, nanos)
}

/// Simple epoch seconds → ISO-8601 without external crate.
fn time_from_epoch(secs: i64, _nanos: u32) -> String {
    const SECS_PER_DAY: i64 = 86400;
    const DAYS_PER_YEAR: i64 = 365;
    const DAYS_PER_4YEARS: i64 = 1461;
    const DAYS_PER_100YEARS: i64 = 36524;
    const DAYS_PER_400YEARS: i64 = 146097;

    let mut days = secs / SECS_PER_DAY;
    let day_secs = (secs % SECS_PER_DAY + SECS_PER_DAY) % SECS_PER_DAY;
    if secs % SECS_PER_DAY < 0 {
        days -= 1;
    }
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Days since 1970-01-01 → civil date.
    days += 719_468; // shift to 0000-03-01
    let era = if days >= 0 {
        days / DAYS_PER_400YEARS
    } else {
        (days - (DAYS_PER_400YEARS - 1)) / DAYS_PER_400YEARS
    };
    let doe = days - era * DAYS_PER_400YEARS;
    let yoe = (doe - doe / DAYS_PER_4YEARS + doe / DAYS_PER_100YEARS - doe / DAYS_PER_400YEARS)
        / DAYS_PER_YEAR;
    let y = yoe + era * 400;
    let doy = doe - (DAYS_PER_YEAR * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Resolve a [`CallerIdentity`] from the async-graphql request context.
///
/// Mutation resolvers call this before invoking `_with_caller` handler methods
/// so audit entries reflect the authenticated caller populated by the HTTP
/// layer's middleware (FEAT-012). When the transport did not inject an
/// identity (e.g. the in-process unit tests below), falls back to
/// [`CallerIdentity::anonymous`].
fn caller_from_ctx(ctx: &async_graphql::dynamic::ResolverContext<'_>) -> CallerIdentity {
    ctx.data::<CallerIdentity>()
        .cloned()
        .unwrap_or_else(|_| CallerIdentity::anonymous())
}

/// Convert an `AxonError` into an `async-graphql` `Error` with structured
/// extensions for OCC conflicts and other error kinds.
fn axon_error_to_gql(err: AxonError) -> GqlError {
    match err {
        AxonError::ConflictingVersion {
            expected,
            actual,
            current_entity,
        } => {
            let entity_json = current_entity.as_ref().map(|e| {
                json!({
                    "id": e.id.to_string(),
                    "version": e.version,
                    "data": &e.data,
                    "collection": e.collection.to_string(),
                })
            });
            GqlError::new(format!(
                "version conflict: expected {expected}, actual {actual}"
            ))
            .extend_with(|_err, ext| {
                ext.set("code", "VERSION_CONFLICT");
                ext.set("expected", GqlValue::from(expected as i64));
                ext.set("actual", GqlValue::from(actual as i64));
                if let Some(ej) = &entity_json {
                    if let Ok(gql_val) = GqlValue::from_json(ej.clone()) {
                        ext.set("currentEntity", gql_val);
                    }
                }
            })
        }
        AxonError::NotFound(msg) => {
            GqlError::new(format!("not found: {msg}")).extend_with(|_err, ext| {
                ext.set("code", "NOT_FOUND");
            })
        }
        AxonError::SchemaValidation(detail) => {
            GqlError::new(format!("schema validation failed: {detail}")).extend_with(|_err, ext| {
                // Keep legacy `SCHEMA_VALIDATION` code for existing clients;
                // expose the raw detail string in the structured extension so
                // clients can surface it without string-parsing the message.
                ext.set("code", "SCHEMA_VALIDATION");
                ext.set("detail", detail.as_str());
            })
        }
        AxonError::UniqueViolation { field, value } => GqlError::new(format!(
            "unique violation on field `{field}`: {value}"
        ))
        .extend_with(|_err, ext| {
            ext.set("code", "UNIQUE_VIOLATION");
            ext.set("field", field.as_str());
            ext.set("value", value.as_str());
        }),
        AxonError::InvalidTransition {
            lifecycle_name,
            current_state,
            target_state,
            valid_transitions,
        } => GqlError::new(format!(
            "invalid transition in lifecycle `{lifecycle_name}`: \
             cannot transition from `{current_state}` to `{target_state}`"
        ))
        .extend_with(move |_err, ext| {
            ext.set("code", "INVALID_TRANSITION");
            ext.set("lifecycleName", lifecycle_name.as_str());
            ext.set("currentState", current_state.as_str());
            ext.set("targetState", target_state.as_str());
            ext.set(
                "validTransitions",
                GqlValue::List(
                    valid_transitions
                        .iter()
                        .map(|s| GqlValue::String(s.clone()))
                        .collect(),
                ),
            );
        }),
        AxonError::LifecycleNotFound { lifecycle_name } => GqlError::new(format!(
            "lifecycle not found: {lifecycle_name}"
        ))
        .extend_with(move |_err, ext| {
            ext.set("code", "LIFECYCLE_NOT_FOUND");
            ext.set("lifecycleName", lifecycle_name.as_str());
        }),
        AxonError::InvalidArgument(msg) => GqlError::new(format!("invalid argument: {msg}"))
            .extend_with(|_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
            }),
        AxonError::InvalidOperation(msg) => GqlError::new(format!("invalid operation: {msg}"))
            .extend_with(|_err, ext| {
                ext.set("code", "INVALID_OPERATION");
            }),
        other => GqlError::new(other.to_string()).extend_with(|_err, ext| {
            ext.set("code", "INTERNAL_ERROR");
        }),
    }
}

// ── Schema builders ─────────────────────────────────────────────────────────

/// Build a dynamic GraphQL schema from the given collection schemas, wired
/// to a live `AxonHandler` for real CRUD operations.
///
/// Each collection produces:
/// - A query field `<collection>(id: ID!): <CollectionType>`
/// - A query field `<collection>s(limit: Int, afterId: ID): [<CollectionType>]`
/// - A mutation field `create<Collection>(id: ID!, input: JSON!): <CollectionType>`
/// - A mutation field `update<Collection>(id: ID!, version: Int!, input: JSON!): <CollectionType>`
/// - A mutation field `patch<Collection>(id: ID!, version: Int!, patch: JSON!): <CollectionType>`
/// - A mutation field `delete<Collection>(id: ID!): Boolean!`
pub fn build_schema_with_handler<S: StorageAdapter + 'static>(
    collections: &[CollectionSchema],
    handler: SharedHandler<S>,
) -> Result<AxonSchema, String> {
    build_schema_with_handler_and_broker(collections, handler, None)
}

/// Build a dynamic GraphQL schema with both a handler and optional broadcast
/// broker for subscriptions.
pub fn build_schema_with_handler_and_broker<S: StorageAdapter + 'static>(
    collections: &[CollectionSchema],
    handler: SharedHandler<S>,
    broker: Option<BroadcastBroker>,
) -> Result<AxonSchema, String> {
    let mut query = Object::new("Query");
    let mut mutation = Object::new("Mutation");
    let mut type_objects = Vec::new();

    query = add_handler_root_query_fields(query, Arc::clone(&handler));

    for schema in collections {
        let collection_name = schema.collection.as_str();
        let type_name = pascal_case(collection_name);
        let edge_type_name = format!("{type_name}Edge");
        let connection_type_name = format!("{type_name}Connection");
        let get_field_name = collection_field_name(collection_name);
        let list_field_name = collection_list_field_name(collection_name);
        let fields = extract_fields(schema);

        // ── Build the GraphQL object type ────────────────────────────────
        let mut obj = Object::new(&type_name);
        for (field_name, gql_type, _required) in &fields {
            let type_ref = parse_type_ref(gql_type);
            let fname = field_name.clone();
            obj = obj.field(Field::new(field_name, type_ref, move |ctx| {
                let fname = fname.clone();
                FieldFuture::new(async move {
                    match ctx.parent_value.try_to_value() {
                        Ok(GqlValue::Object(map)) => {
                            let key = async_graphql::Name::new(&fname);
                            Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                        }
                        _ => Ok(Some(FieldValue::NULL)),
                    }
                })
            }));
        }
        type_objects.push(obj);
        type_objects.push(typed_edge_object(&edge_type_name, &type_name));
        type_objects.push(typed_connection_object(
            &connection_type_name,
            &edge_type_name,
        ));

        // ── Query: get by ID ─────────────────────────────────────────────
        let col_id = CollectionId::new(collection_name);
        let handler_get = Arc::clone(&handler);
        let col_for_get = col_id.clone();
        let get_field = Field::new(&get_field_name, TypeRef::named(&type_name), move |ctx| {
            let handler = Arc::clone(&handler_get);
            let col = col_for_get.clone();
            FieldFuture::new(async move {
                let id_str = ctx.args.try_get("id")?.string()?;

                let guard = handler.lock().await;
                match guard.get_entity(GetEntityRequest {
                    collection: col.clone(),
                    id: EntityId::new(id_str),
                }) {
                    Ok(resp) => Ok(Some(entity_to_field_value(&resp.entity))),
                    Err(AxonError::NotFound(_)) => Ok(None),
                    Err(e) => Err(axon_error_to_gql(e)),
                }
            })
        })
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)));
        query = query.field(get_field);

        // ── Query: list ──────────────────────────────────────────────────
        let handler_list = Arc::clone(&handler);
        let col_for_list = col_id.clone();
        let type_name_list = type_name.clone();
        let list_field = Field::new(
            &list_field_name,
            TypeRef::named_list(&type_name_list),
            move |ctx| {
                let handler = Arc::clone(&handler_list);
                let col = col_for_list.clone();
                FieldFuture::new(async move {
                    let limit = ctx
                        .args
                        .try_get("limit")
                        .ok()
                        .and_then(|v| v.i64().ok())
                        .map(|v| v as usize);

                    let after_id = ctx
                        .args
                        .try_get("afterId")
                        .ok()
                        .and_then(|v| v.string().ok())
                        .map(EntityId::new);

                    let filter = ctx
                        .args
                        .try_get("filter")
                        .ok()
                        .map(|v| parse_graphql_filter_arg(v.as_value()))
                        .transpose()?;

                    let sort = ctx
                        .args
                        .try_get("sort")
                        .ok()
                        .map(|v| parse_graphql_sort_arg(v.as_value()))
                        .transpose()?
                        .unwrap_or_default();

                    let guard = handler.lock().await;
                    match guard.query_entities(QueryEntitiesRequest {
                        collection: col.clone(),
                        filter,
                        sort,
                        limit,
                        after_id,
                        count_only: false,
                    }) {
                        Ok(resp) => {
                            let items: Vec<FieldValue> = resp
                                .entities
                                .iter()
                                .map(|e| entity_to_field_value(e))
                                .collect();
                            Ok(Some(FieldValue::list(items)))
                        }
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID)))
        .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)))
        .argument(InputValue::new("sort", TypeRef::named_nn_list(SORT_INPUT)));
        query = query.field(list_field);

        let list_connection_field_name = format!("{list_field_name}Connection");
        let handler_list_connection = Arc::clone(&handler);
        let col_for_list_connection = col_id.clone();
        let connection_type_name_ref = connection_type_name.clone();
        let list_connection_field = Field::new(
            &list_connection_field_name,
            TypeRef::named_nn(&connection_type_name_ref),
            move |ctx| {
                let handler = Arc::clone(&handler_list_connection);
                let col = col_for_list_connection.clone();
                FieldFuture::new(async move {
                    let limit = ctx
                        .args
                        .try_get("limit")
                        .ok()
                        .and_then(|v| v.i64().ok())
                        .map(|v| v as usize);

                    let after_id = ctx
                        .args
                        .try_get("afterId")
                        .ok()
                        .and_then(|v| v.string().ok())
                        .map(EntityId::new);

                    let filter = ctx
                        .args
                        .try_get("filter")
                        .ok()
                        .map(|v| parse_graphql_filter_arg(v.as_value()))
                        .transpose()?;

                    let sort = ctx
                        .args
                        .try_get("sort")
                        .ok()
                        .map(|v| parse_graphql_sort_arg(v.as_value()))
                        .transpose()?
                        .unwrap_or_default();
                    let has_previous_page = after_id.is_some();

                    let guard = handler.lock().await;
                    match guard.query_entities(QueryEntitiesRequest {
                        collection: col.clone(),
                        filter,
                        sort,
                        limit,
                        after_id,
                        count_only: false,
                    }) {
                        Ok(resp) => Ok(Some(entity_connection_value(
                            &resp.entities,
                            resp.total_count,
                            resp.next_cursor,
                            has_previous_page,
                            false,
                        ))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID)))
        .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)))
        .argument(InputValue::new("sort", TypeRef::named_nn_list(SORT_INPUT)));
        query = query.field(list_connection_field);

        // ── Mutation: create ─────────────────────────────────────────────
        let create_field_name = format!("create{type_name}");
        let handler_create = Arc::clone(&handler);
        let col_for_create = col_id.clone();
        let type_name_create = type_name.clone();
        let create_field = Field::new(
            &create_field_name,
            TypeRef::named(&type_name_create),
            move |ctx| {
                let handler = Arc::clone(&handler_create);
                let col = col_for_create.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;

                    let input_str = ctx.args.try_get("input")?.string()?;

                    let data: Value = serde_json::from_str(input_str)
                        .map_err(|e| GqlError::new(format!("invalid JSON input: {e}")))?;

                    let mut guard = handler.lock().await;
                    match guard.create_entity_with_caller(
                        CreateEntityRequest {
                            collection: col.clone(),
                            id: EntityId::new(id_str),
                            data,
                            actor: None,
                            audit_metadata: None,
                            attribution: None,
                        },
                        &caller,
                        None,
                    ) {
                        Ok(resp) => Ok(Some(entity_to_field_value(&resp.entity))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new("input", TypeRef::named_nn(TypeRef::STRING)));
        mutation = mutation.field(create_field);

        // ── Mutation: update ─────────────────────────────────────────────
        let update_field_name = format!("update{type_name}");
        let handler_update = Arc::clone(&handler);
        let col_for_update = col_id.clone();
        let type_name_update = type_name.clone();
        let update_field = Field::new(
            &update_field_name,
            TypeRef::named(&type_name_update),
            move |ctx| {
                let handler = Arc::clone(&handler_update);
                let col = col_for_update.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;
                    let version = ctx.args.try_get("version")?.i64()? as u64;

                    let input_str = ctx.args.try_get("input")?.string()?;

                    let data: Value = serde_json::from_str(input_str)
                        .map_err(|e| GqlError::new(format!("invalid JSON input: {e}")))?;

                    let mut guard = handler.lock().await;
                    match guard.update_entity_with_caller(
                        UpdateEntityRequest {
                            collection: col.clone(),
                            id: EntityId::new(id_str),
                            data,
                            expected_version: version,
                            actor: None,
                            audit_metadata: None,
                            attribution: None,
                        },
                        &caller,
                        None,
                    ) {
                        Ok(resp) => Ok(Some(entity_to_field_value(&resp.entity))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new("version", TypeRef::named_nn(TypeRef::INT)))
        .argument(InputValue::new("input", TypeRef::named_nn(TypeRef::STRING)));
        mutation = mutation.field(update_field);

        // ── Mutation: patch ──────────────────────────────────────────────
        let patch_field_name = format!("patch{type_name}");
        let handler_patch = Arc::clone(&handler);
        let col_for_patch = col_id.clone();
        let type_name_patch = type_name.clone();
        let patch_field = Field::new(
            &patch_field_name,
            TypeRef::named(&type_name_patch),
            move |ctx| {
                let handler = Arc::clone(&handler_patch);
                let col = col_for_patch.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;
                    let version = ctx.args.try_get("version")?.i64()? as u64;

                    let patch_str = ctx.args.try_get("patch")?.string()?;

                    let patch: Value = serde_json::from_str(patch_str)
                        .map_err(|e| GqlError::new(format!("invalid JSON patch: {e}")))?;

                    let mut guard = handler.lock().await;
                    match guard.patch_entity_with_caller(
                        PatchEntityRequest {
                            collection: col.clone(),
                            id: EntityId::new(id_str),
                            patch,
                            expected_version: version,
                            actor: None,
                            audit_metadata: None,
                            attribution: None,
                        },
                        &caller,
                        None,
                    ) {
                        Ok(resp) => Ok(Some(entity_to_field_value(&resp.entity))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new("version", TypeRef::named_nn(TypeRef::INT)))
        .argument(InputValue::new("patch", TypeRef::named_nn(TypeRef::STRING)));
        mutation = mutation.field(patch_field);

        // ── Mutation: delete ─────────────────────────────────────────────
        let delete_field_name = format!("delete{type_name}");
        let handler_delete = Arc::clone(&handler);
        let col_for_delete = col_id.clone();
        let delete_field = Field::new(
            &delete_field_name,
            TypeRef::named_nn(TypeRef::BOOLEAN),
            move |ctx| {
                let handler = Arc::clone(&handler_delete);
                let col = col_for_delete.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;

                    let mut guard = handler.lock().await;
                    match guard.delete_entity_with_caller(
                        DeleteEntityRequest {
                            collection: col.clone(),
                            id: EntityId::new(id_str),
                            actor: None,
                            force: false,
                            audit_metadata: None,
                            attribution: None,
                        },
                        &caller,
                        None,
                    ) {
                        Ok(_) => Ok(Some(FieldValue::from(GqlValue::from(true)))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)));
        mutation = mutation.field(delete_field);

        // ── Mutation: transition<Collection>Lifecycle ────────────────────
        let transition_field_name = format!("transition{type_name}Lifecycle");
        let handler_transition = Arc::clone(&handler);
        let col_for_transition = col_id.clone();
        let type_name_transition = type_name.clone();
        let transition_field = Field::new(
            &transition_field_name,
            TypeRef::named(&type_name_transition),
            move |ctx| {
                let handler = Arc::clone(&handler_transition);
                let col = col_for_transition.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;
                    let lifecycle_name = ctx.args.try_get("lifecycleName")?.string()?.to_owned();
                    let target_state = ctx.args.try_get("targetState")?.string()?.to_owned();
                    let expected_version = ctx.args.try_get("expectedVersion")?.i64()? as u64;

                    let mut guard = handler.lock().await;
                    match guard.transition_lifecycle_with_caller(
                        TransitionLifecycleRequest {
                            collection_id: col.clone(),
                            entity_id: EntityId::new(id_str),
                            lifecycle_name,
                            target_state,
                            expected_version,
                            actor: None,
                            audit_metadata: None,
                            attribution: None,
                        },
                        &caller,
                        None,
                    ) {
                        Ok(resp) => Ok(Some(entity_to_field_value(&resp.entity))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "lifecycleName",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "targetState",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "expectedVersion",
            TypeRef::named_nn(TypeRef::INT),
        ));
        mutation = mutation.field(transition_field);
    }

    // ── Global link mutations ────────────────────────────────────────────────
    //
    // Links span two collections and are not backed by a GraphQL Entity type,
    // so `createLink` / `deleteLink` are exposed as global (collection-less)
    // mutations returning `Boolean!`. The structured request type carries the
    // full source/target coordinates.
    {
        let handler_create_link = Arc::clone(&handler);
        let create_link_field = Field::new(
            "createLink",
            TypeRef::named_nn(TypeRef::BOOLEAN),
            move |ctx| {
                let handler = Arc::clone(&handler_create_link);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let source_collection =
                        ctx.args.try_get("sourceCollection")?.string()?.to_owned();
                    let source_id = ctx.args.try_get("sourceId")?.string()?.to_owned();
                    let target_collection =
                        ctx.args.try_get("targetCollection")?.string()?.to_owned();
                    let target_id = ctx.args.try_get("targetId")?.string()?.to_owned();
                    let link_type = ctx.args.try_get("linkType")?.string()?.to_owned();
                    let metadata = match ctx.args.try_get("metadata") {
                        Ok(v) => match v.string() {
                            Ok(s) => serde_json::from_str::<Value>(s).map_err(|e| {
                                GqlError::new(format!("invalid JSON metadata: {e}"))
                            })?,
                            Err(_) => Value::Null,
                        },
                        Err(_) => Value::Null,
                    };

                    let mut guard = handler.lock().await;
                    match guard.create_link_with_caller(
                        CreateLinkRequest {
                            source_collection: CollectionId::new(source_collection),
                            source_id: EntityId::new(source_id),
                            target_collection: CollectionId::new(target_collection),
                            target_id: EntityId::new(target_id),
                            link_type,
                            metadata,
                            actor: None,
                            attribution: None,
                        },
                        &caller,
                        None,
                    ) {
                        Ok(_) => Ok(Some(FieldValue::from(GqlValue::from(true)))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new(
            "sourceCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("sourceId", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "targetCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("targetId", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("metadata", TypeRef::named(TypeRef::STRING)));
        mutation = mutation.field(create_link_field);

        let handler_delete_link = Arc::clone(&handler);
        let delete_link_field = Field::new(
            "deleteLink",
            TypeRef::named_nn(TypeRef::BOOLEAN),
            move |ctx| {
                let handler = Arc::clone(&handler_delete_link);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let source_collection =
                        ctx.args.try_get("sourceCollection")?.string()?.to_owned();
                    let source_id = ctx.args.try_get("sourceId")?.string()?.to_owned();
                    let target_collection =
                        ctx.args.try_get("targetCollection")?.string()?.to_owned();
                    let target_id = ctx.args.try_get("targetId")?.string()?.to_owned();
                    let link_type = ctx.args.try_get("linkType")?.string()?.to_owned();

                    let mut guard = handler.lock().await;
                    match guard.delete_link_with_caller(
                        DeleteLinkRequest {
                            source_collection: CollectionId::new(source_collection),
                            source_id: EntityId::new(source_id),
                            target_collection: CollectionId::new(target_collection),
                            target_id: EntityId::new(target_id),
                            link_type,
                            actor: None,
                            attribution: None,
                        },
                        &caller,
                        None,
                    ) {
                        Ok(_) => Ok(Some(FieldValue::from(GqlValue::from(true)))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new(
            "sourceCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("sourceId", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "targetCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("targetId", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ));
        mutation = mutation.field(delete_link_field);
    }

    // -- Subscription type ---------------------------------------------------
    let subscription = broker.map(build_entity_changed_subscription);

    let subscription_name = subscription.as_ref().map(|s| s.type_name().to_owned());
    let mut schema_builder = Schema::build(
        query.type_name(),
        Some(mutation.type_name()),
        subscription_name.as_deref(),
    )
    .limit_depth(max_graphql_depth())
    .limit_complexity(max_graphql_complexity())
    .register(Scalar::new("JSON"))
    .register(filter_input_object())
    .register(sort_input_object())
    .register(query)
    .register(mutation);

    schema_builder = register_root_objects(schema_builder);

    if let Some(sub) = subscription {
        schema_builder = schema_builder.register(sub);
        // Register the ChangeEvent object type so subscription resolvers can
        // return structured data.
        schema_builder = schema_builder.register(change_event_object());
    }

    for obj in type_objects {
        schema_builder = schema_builder.register(obj);
    }

    let schema = schema_builder
        .finish()
        .map_err(|e| format!("failed to build GraphQL schema: {e}"))?;

    Ok(AxonSchema { schema })
}

/// Build a stub dynamic GraphQL schema from the given collection schemas.
///
/// Resolvers return `NULL` / empty lists — useful for SDL introspection and
/// tests that only need the schema shape, not live data.
pub fn build_schema(collections: &[CollectionSchema]) -> Result<AxonSchema, String> {
    let mut query = Object::new("Query");
    let mut mutation = Object::new("Mutation");
    let mut type_objects = Vec::new();

    query = add_stub_root_query_fields(query);

    for schema in collections {
        let collection_name = schema.collection.as_str();
        let type_name = pascal_case(collection_name);
        let edge_type_name = format!("{type_name}Edge");
        let connection_type_name = format!("{type_name}Connection");
        let get_field_name = collection_field_name(collection_name);
        let list_field_name = collection_list_field_name(collection_name);
        let fields = extract_fields(schema);

        // Build the GraphQL object type for this collection.
        let mut obj = Object::new(&type_name);
        for (field_name, gql_type, _required) in &fields {
            let type_ref = parse_type_ref(gql_type);
            obj = obj.field(Field::new(field_name, type_ref, |_ctx| {
                FieldFuture::new(async move { Ok(Some(FieldValue::NULL)) })
            }));
        }
        type_objects.push(obj);
        type_objects.push(typed_edge_object(&edge_type_name, &type_name));
        type_objects.push(typed_connection_object(
            &connection_type_name,
            &edge_type_name,
        ));

        // Query: get by ID.
        let type_name_ref = type_name.clone();
        query = query.field(Field::new(
            &get_field_name,
            TypeRef::named(&type_name_ref),
            |_ctx| FieldFuture::new(async move { Ok(Some(FieldValue::NULL)) }),
        ));

        // Query: list.
        let type_name_list = type_name.clone();
        query = query.field(
            Field::new(
                &list_field_name,
                TypeRef::named_list(&type_name_list),
                |_ctx| {
                    FieldFuture::new(
                        async move { Ok(Some(FieldValue::list(Vec::<FieldValue>::new()))) },
                    )
                },
            )
            .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
            .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID)))
            .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)))
            .argument(InputValue::new("sort", TypeRef::named_nn_list(SORT_INPUT))),
        );

        let list_connection_field_name = format!("{list_field_name}Connection");
        let connection_type_name_ref = connection_type_name.clone();
        query = query.field(
            Field::new(
                &list_connection_field_name,
                TypeRef::named_nn(&connection_type_name_ref),
                |_ctx| {
                    FieldFuture::new(async move {
                        Ok(Some(entity_connection_value(&[], 0, None, false, false)))
                    })
                },
            )
            .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
            .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID)))
            .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)))
            .argument(InputValue::new("sort", TypeRef::named_nn_list(SORT_INPUT))),
        );

        // Mutation: create.
        let create_field_name = format!("create{type_name}");
        let type_name_create = type_name.clone();
        mutation = mutation.field(Field::new(
            &create_field_name,
            TypeRef::named(&type_name_create),
            |_ctx| FieldFuture::new(async move { Ok(Some(FieldValue::NULL)) }),
        ));

        // Mutation: update.
        let update_field_name = format!("update{type_name}");
        let type_name_update = type_name.clone();
        mutation = mutation.field(Field::new(
            &update_field_name,
            TypeRef::named(&type_name_update),
            |_ctx| FieldFuture::new(async move { Ok(Some(FieldValue::NULL)) }),
        ));

        // Mutation: delete.
        let delete_field_name = format!("delete{type_name}");
        mutation = mutation.field(Field::new(
            &delete_field_name,
            TypeRef::named_nn(TypeRef::BOOLEAN),
            |_ctx| {
                FieldFuture::new(async move { Ok(Some(FieldValue::from(GqlValue::from(true)))) })
            },
        ));
    }

    let mutation_type = if collections.is_empty() {
        None
    } else {
        Some(mutation.type_name())
    };

    let mut schema_builder = Schema::build(query.type_name(), mutation_type, None)
        .limit_depth(max_graphql_depth())
        .limit_complexity(max_graphql_complexity())
        .register(Scalar::new("JSON"))
        .register(filter_input_object())
        .register(sort_input_object())
        .register(query);

    if !collections.is_empty() {
        schema_builder = schema_builder.register(mutation);
    }

    schema_builder = register_root_objects(schema_builder);

    for obj in type_objects {
        schema_builder = schema_builder.register(obj);
    }

    let schema = schema_builder
        .finish()
        .map_err(|e| format!("failed to build GraphQL schema: {e}"))?;

    Ok(AxonSchema { schema })
}

// -- Subscription helpers -----------------------------------------------------

/// Build the `ChangeEvent` GraphQL object type used by subscription resolvers.
fn change_event_object() -> Object {
    Object::new("ChangeEvent")
        .field(Field::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
            |ctx| {
                FieldFuture::new(async move {
                    match ctx.parent_value.try_to_value() {
                        Ok(GqlValue::Object(map)) => {
                            let key = async_graphql::Name::new("collection");
                            Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                        }
                        _ => Ok(Some(FieldValue::NULL)),
                    }
                })
            },
        ))
        .field(Field::new(
            "entityId",
            TypeRef::named_nn(TypeRef::STRING),
            |ctx| {
                FieldFuture::new(async move {
                    match ctx.parent_value.try_to_value() {
                        Ok(GqlValue::Object(map)) => {
                            let key = async_graphql::Name::new("entityId");
                            Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                        }
                        _ => Ok(Some(FieldValue::NULL)),
                    }
                })
            },
        ))
        .field(Field::new(
            "operation",
            TypeRef::named_nn(TypeRef::STRING),
            |ctx| {
                FieldFuture::new(async move {
                    match ctx.parent_value.try_to_value() {
                        Ok(GqlValue::Object(map)) => {
                            let key = async_graphql::Name::new("operation");
                            Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                        }
                        _ => Ok(Some(FieldValue::NULL)),
                    }
                })
            },
        ))
        .field(Field::new("data", TypeRef::named(TypeRef::STRING), |ctx| {
            FieldFuture::new(async move {
                match ctx.parent_value.try_to_value() {
                    Ok(GqlValue::Object(map)) => {
                        let key = async_graphql::Name::new("data");
                        Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                    }
                    _ => Ok(Some(FieldValue::NULL)),
                }
            })
        }))
        .field(Field::new(
            "version",
            TypeRef::named_nn(TypeRef::INT),
            |ctx| {
                FieldFuture::new(async move {
                    match ctx.parent_value.try_to_value() {
                        Ok(GqlValue::Object(map)) => {
                            let key = async_graphql::Name::new("version");
                            Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                        }
                        _ => Ok(Some(FieldValue::NULL)),
                    }
                })
            },
        ))
        .field(Field::new(
            "timestampMs",
            TypeRef::named_nn(TypeRef::INT),
            |ctx| {
                FieldFuture::new(async move {
                    match ctx.parent_value.try_to_value() {
                        Ok(GqlValue::Object(map)) => {
                            let key = async_graphql::Name::new("timestampMs");
                            Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                        }
                        _ => Ok(Some(FieldValue::NULL)),
                    }
                })
            },
        ))
        .field(Field::new(
            "actor",
            TypeRef::named_nn(TypeRef::STRING),
            |ctx| {
                FieldFuture::new(async move {
                    match ctx.parent_value.try_to_value() {
                        Ok(GqlValue::Object(map)) => {
                            let key = async_graphql::Name::new("actor");
                            Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                        }
                        _ => Ok(Some(FieldValue::NULL)),
                    }
                })
            },
        ))
}

/// Convert a `ChangeEvent` into a `FieldValue` suitable for subscription emission.
fn change_event_to_field_value(event: &crate::subscriptions::ChangeEvent) -> FieldValue<'static> {
    let mut map = serde_json::Map::new();
    map.insert("collection".into(), Value::String(event.collection.clone()));
    map.insert("entityId".into(), Value::String(event.entity_id.clone()));
    map.insert("operation".into(), Value::String(event.operation.clone()));
    if let Some(data) = &event.data {
        map.insert("data".into(), Value::String(data.to_string()));
    }
    map.insert("version".into(), json!(event.version));
    map.insert("timestampMs".into(), json!(event.timestamp_ms));
    map.insert("actor".into(), Value::String(event.actor.clone()));

    FieldValue::from(GqlValue::from_json(Value::Object(map)).unwrap_or(GqlValue::Null))
}

/// Build the `Subscription` type with an `entityChanged` field that
/// streams change events from the `BroadcastBroker`.
fn build_entity_changed_subscription(broker: BroadcastBroker) -> Subscription {
    let entity_changed = SubscriptionField::new(
        "entityChanged",
        TypeRef::named_nn("ChangeEvent"),
        move |ctx| {
            let broker = broker.clone();

            // Optional collection filter from argument.
            let collection_filter: Option<String> = ctx
                .args
                .try_get("collection")
                .ok()
                .and_then(|v| v.string().ok())
                .map(|s| s.to_owned());

            SubscriptionFieldFuture::new(async move {
                let rx = broker.subscribe();
                let stream =
                    tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(move |result| {
                        let filter = collection_filter.clone();
                        async move {
                            match result {
                                Ok(event) => {
                                    // Apply optional collection filter.
                                    if let Some(ref col) = filter {
                                        if event.collection != *col {
                                            return None;
                                        }
                                    }
                                    Some(Ok(change_event_to_field_value(&event)))
                                }
                                // Lagged -- some events were dropped; skip.
                                Err(_) => None,
                            }
                        }
                    });

                Ok(stream)
            })
        },
    )
    .argument(InputValue::new(
        "collection",
        TypeRef::named(TypeRef::STRING),
    ))
    .description("Subscribe to entity change events. Optionally filter by collection name.");

    Subscription::new("Subscription").field(entity_changed)
}

/// Convert a snake_case collection name to PascalCase for the GraphQL type.
fn pascal_case(s: &str) -> String {
    let mut name: String = graphql_name_words(s)
        .into_iter()
        .flat_map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect::<Vec<_>>(),
                None => Vec::new(),
            }
        })
        .collect();

    if name
        .chars()
        .next()
        .map_or(true, |first| !first.is_ascii_alphabetic() && first != '_')
    {
        name = format!("Collection{name}");
    }
    if is_reserved_graphql_type_name(&name) {
        name.push_str("Record");
    }
    name
}

fn collection_field_name(collection: &str) -> String {
    let mut words = graphql_name_words(collection);
    let first = words
        .first_mut()
        .map(|word| word.to_ascii_lowercase())
        .unwrap_or_else(|| String::from("collection"));
    let mut name = first;
    for word in words.iter().skip(1) {
        let mut chars = word.chars();
        if let Some(c) = chars.next() {
            name.extend(c.to_uppercase());
            name.push_str(&chars.as_str().to_ascii_lowercase());
        }
    }
    if name
        .chars()
        .next()
        .map_or(true, |first| !first.is_ascii_alphabetic() && first != '_')
    {
        name = format!("collection{name}");
    }
    if is_reserved_query_field_name(&name) {
        name.push_str("Collection");
    }
    name
}

fn collection_list_field_name(collection: &str) -> String {
    let field_name = collection_field_name(collection);
    if is_simple_graphql_name(collection) && !field_name.ends_with('s') {
        format!("{field_name}s")
    } else {
        format!("{field_name}List")
    }
}

fn graphql_name_words(s: &str) -> Vec<String> {
    let words: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if words.is_empty() {
        vec![String::from("collection")]
    } else {
        words
    }
}

fn is_simple_graphql_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_reserved_query_field_name(name: &str) -> bool {
    matches!(
        name,
        "entity" | "entities" | "collections" | "collection" | "auditLog"
    )
}

fn is_reserved_graphql_type_name(name: &str) -> bool {
    matches!(
        name,
        "Query"
            | "Mutation"
            | "Subscription"
            | "Entity"
            | "EntityEdge"
            | "EntityConnection"
            | "PageInfo"
            | "CollectionMeta"
            | "AuditEntry"
            | "AuditEdge"
            | "AuditConnection"
            | "String"
            | "Int"
            | "Float"
            | "Boolean"
            | "ID"
            | "JSON"
    )
}

/// Parse a simplified GraphQL type reference string.
fn parse_type_ref(type_str: &str) -> TypeRef {
    if let Some(inner) = type_str.strip_suffix('!') {
        TypeRef::named_nn(inner)
    } else {
        TypeRef::named(type_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::CollectionId;
    use axon_storage::MemoryStorageAdapter;
    use serde_json::json;

    fn test_schema() -> CollectionSchema {
        CollectionSchema {
            collection: CollectionId::new("tasks"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": { "type": "string" },
                    "status": { "type": "string" },
                    "priority": { "type": "integer" }
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        }
    }

    /// Create a shared handler with the given collection schemas registered.
    async fn make_handler(schemas: &[CollectionSchema]) -> SharedHandler<MemoryStorageAdapter> {
        let storage = MemoryStorageAdapter::default();
        let handler = AxonHandler::new(storage);
        let handler = Arc::new(Mutex::new(handler));

        {
            let mut guard = handler.lock().await;
            for s in schemas {
                let _ = guard.put_schema(s.clone());
            }
        }

        handler
    }

    #[test]
    fn pascal_case_conversion() {
        assert_eq!(pascal_case("tasks"), "Tasks");
        assert_eq!(pascal_case("line_items"), "LineItems");
        assert_eq!(pascal_case("a_b_c"), "ABC");
        assert_eq!(pascal_case("time-entries"), "TimeEntries");
        assert_eq!(pascal_case("123 imports"), "Collection123Imports");
        assert_eq!(pascal_case("entity"), "EntityRecord");
    }

    #[test]
    fn collection_field_name_conversion() {
        assert_eq!(collection_field_name("item"), "item");
        assert_eq!(collection_list_field_name("item"), "items");
        assert_eq!(collection_field_name("time_entries"), "timeEntries");
        assert_eq!(
            collection_list_field_name("time_entries"),
            "timeEntriesList"
        );
        assert_eq!(collection_list_field_name("tasks"), "tasksList");
        assert_eq!(collection_field_name("entity"), "entityCollection");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn build_schema_from_single_collection() {
        let schema = build_schema(&[test_schema()]).expect("schema should build");
        let sdl = schema.schema.sdl();
        assert!(sdl.contains("type Tasks"), "SDL should contain Tasks type");
        assert!(sdl.contains("tasks"), "SDL should contain tasks query");
        assert!(
            sdl.contains("createTasks"),
            "SDL should contain createTasks mutation"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn build_schema_with_broker_includes_subscription_type() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;
        let broker = crate::subscriptions::BroadcastBroker::default();

        let schema =
            build_schema_with_handler_and_broker(&[ts], Arc::clone(&handler), Some(broker))
                .expect("schema with broker should build");
        let sdl = schema.schema.sdl();
        assert!(
            sdl.contains("type Subscription"),
            "SDL should contain Subscription type"
        );
        assert!(
            sdl.contains("entityChanged"),
            "SDL should contain entityChanged field"
        );
        assert!(
            sdl.contains("type ChangeEvent"),
            "SDL should contain ChangeEvent type"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn build_schema_without_broker_has_no_subscription() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");
        let sdl = schema.schema.sdl();
        assert!(
            !sdl.contains("type Subscription"),
            "SDL should NOT contain Subscription type when no broker"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn build_schema_with_multiple_collections() {
        let tasks = test_schema();
        let users = CollectionSchema {
            collection: CollectionId::new("users"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "email": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        };

        let schema = build_schema(&[tasks, users]).expect("schema should build");
        let sdl = schema.schema.sdl();
        assert!(sdl.contains("type Tasks"));
        assert!(sdl.contains("type Users"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn introspection_query_works() {
        let schema = build_schema(&[test_schema()]).expect("schema should build");
        let result = schema
            .schema
            .execute("{ __schema { types { name } } }")
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ui_helper_queries_match_current_dynamic_schema() {
        let schema = build_schema(&[test_schema()]).expect("schema should build");
        let helper_queries = [
            ("fetchCollections", "{ collections { name entityCount } }"),
            (
                "fetchEntities",
                r#"{
                    entities(collection: "tasks", limit: 50) {
                        edges { node { id version data } }
                        pageInfo { hasNextPage endCursor }
                    }
                }"#,
            ),
            (
                "fetchEntity",
                r#"{
                    entity(collection: "tasks", id: "task-1") {
                        id
                        version
                        data
                        createdAt
                        updatedAt
                    }
                }"#,
            ),
        ];

        for (name, query) in helper_queries {
            let result = schema.schema.execute(query).await;
            assert!(
                result.errors.is_empty(),
                "{name} should match the dynamic schema: {:?}",
                result.errors,
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_collections_builds_valid_schema() {
        let schema = build_schema(&[]).expect("empty schema should build");
        let result = schema
            .schema
            .execute("{ collections { name } entities(collection: \"missing\") { totalCount } }")
            .await;
        assert!(
            result.errors.is_empty(),
            "empty root schema should be queryable: {:?}",
            result.errors
        );
    }

    // ── Live handler integration tests ──────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_get_entity_by_id() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        {
            let mut guard = handler.lock().await;
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: EntityId::new("t1"),
                    data: json!({"title": "Hello", "status": "open"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("create should succeed");
        }

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(r#"{ tasks(id: "t1") { id version title status } }"#)
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        let task = &data["tasks"];
        assert_eq!(task["id"], "t1");
        assert_eq!(task["version"], 1);
        assert_eq!(task["title"], "Hello");
        assert_eq!(task["status"], "open");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_list_entities() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        {
            let mut guard = handler.lock().await;
            for i in 1..=3 {
                guard
                    .create_entity(CreateEntityRequest {
                        collection: CollectionId::new("tasks"),
                        id: EntityId::new(format!("t{i}")),
                        data: json!({"title": format!("Task {i}")}),
                        actor: None,
                        audit_metadata: None,
                        attribution: None,
                    })
                    .expect("create should succeed");
            }
        }

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute("{ tasksList(limit: 2) { id title } }")
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        let tasks = data["tasksList"].as_array().expect("should be array");
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_create_mutation() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(
                r#"mutation { createTasks(id: "t1", input: "{\"title\":\"New\"}") { id version title } }"#,
            )
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        assert_eq!(data["createTasks"]["id"], "t1");
        assert_eq!(data["createTasks"]["version"], 1);
        assert_eq!(data["createTasks"]["title"], "New");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_update_mutation() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        {
            let mut guard = handler.lock().await;
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: EntityId::new("t1"),
                    data: json!({"title": "Old"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("create should succeed");
        }

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(
                r#"mutation { updateTasks(id: "t1", version: 1, input: "{\"title\":\"Updated\"}") { id version title } }"#,
            )
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        assert_eq!(data["updateTasks"]["version"], 2);
        assert_eq!(data["updateTasks"]["title"], "Updated");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_update_version_conflict() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        {
            let mut guard = handler.lock().await;
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: EntityId::new("t1"),
                    data: json!({"title": "V1"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("create should succeed");
            guard
                .update_entity(UpdateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: EntityId::new("t1"),
                    data: json!({"title": "V2"}),
                    expected_version: 1,
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("update should succeed");
        }

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(
                r#"mutation { updateTasks(id: "t1", version: 1, input: "{\"title\":\"Stale\"}") { id version } }"#,
            )
            .await;
        assert!(
            !result.errors.is_empty(),
            "should have version conflict error"
        );

        let err = &result.errors[0];
        assert!(
            err.message.contains("version conflict"),
            "error message: {}",
            err.message
        );

        let ext = &err.extensions;
        assert!(ext.is_some(), "error should have extensions");
        let ext = ext.as_ref().expect("extensions");
        let code = ext.get("code");
        assert!(
            matches!(code, Some(GqlValue::String(s)) if s == "VERSION_CONFLICT"),
            "expected VERSION_CONFLICT code in extensions, got: {code:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_delete_mutation() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        {
            let mut guard = handler.lock().await;
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: EntityId::new("t1"),
                    data: json!({"title": "To delete"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("create should succeed");
        }

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(r#"mutation { deleteTasks(id: "t1") }"#)
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        assert_eq!(data["deleteTasks"], true);

        // Verify the entity is gone.
        let get_result = schema.schema.execute(r#"{ tasks(id: "t1") { id } }"#).await;
        assert!(get_result.errors.is_empty());
        let get_data = get_result.data.into_json().expect("json");
        assert!(get_data["tasks"].is_null());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_patch_mutation() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        {
            let mut guard = handler.lock().await;
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("tasks"),
                    id: EntityId::new("t1"),
                    data: json!({"title": "Original", "status": "open"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("create should succeed");
        }

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(
                r#"mutation { patchTasks(id: "t1", version: 1, patch: "{\"status\":\"closed\"}") { id version title status } }"#,
            )
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        assert_eq!(data["patchTasks"]["version"], 2);
        assert_eq!(data["patchTasks"]["title"], "Original");
        assert_eq!(data["patchTasks"]["status"], "closed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handler_schema_get_not_found_returns_null() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;

        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(r#"{ tasks(id: "nonexistent") { id } }"#)
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let data = result.data.into_json().expect("json");
        assert!(data["tasks"].is_null());
    }
}
