//! Dynamic GraphQL schema builder from Axon collections.
//!
//! Generates a full GraphQL schema (queries + mutations + introspection)
//! from the set of registered collections and their entity schemas.
//!
//! When a shared `AxonHandler` is provided via [`build_schema_with_handler`],
//! resolvers delegate to the live handler for real CRUD operations. The
//! plain [`build_schema`] function builds a stub schema (useful for SDL
//! inspection and tests).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use async_graphql::dynamic::{
    Enum, Field, FieldFuture, FieldValue, InputObject, InputValue, Object, Scalar, Schema,
    SchemaBuilder, Subscription, SubscriptionField, SubscriptionFieldFuture, TypeRef,
};
use async_graphql::futures_util::StreamExt;
use async_graphql::{Error as GqlError, ErrorExtensions, Value as GqlValue};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::subscriptions::BroadcastBroker;

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest,
    DeleteLinkRequest, DescribeCollectionRequest, DropCollectionRequest, FieldFilter, FilterNode,
    FilterOp, FindLinkCandidatesRequest, GateFilter, GetEntityRequest, ListCollectionsRequest,
    PatchEntityRequest, PutSchemaRequest, QueryAuditRequest, QueryEntitiesRequest, SortDirection,
    SortField, TransitionLifecycleRequest, TraverseDirection, TraverseRequest, UpdateEntityRequest,
};
use axon_core::auth::{CallerIdentity, Operation};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::{Entity, Link};
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
const STRING_FILTER_INPUT: &str = "AxonStringFilterInput";
const INT_FILTER_INPUT: &str = "AxonIntFilterInput";
const FLOAT_FILTER_INPUT: &str = "AxonFloatFilterInput";
const BOOLEAN_FILTER_INPUT: &str = "AxonBooleanFilterInput";
const JSON_FILTER_INPUT: &str = "AxonJsonFilterInput";
const AGGREGATE_FUNCTION_ENUM: &str = "AxonAggregateFunction";
const AGGREGATE_VALUE_TYPE: &str = "AxonAggregateValue";
const ENTITY_TYPE: &str = "Entity";
const ENTITY_EDGE_TYPE: &str = "EntityEdge";
const ENTITY_CONNECTION_TYPE: &str = "EntityConnection";
const PAGE_INFO_TYPE: &str = "PageInfo";
const COLLECTION_META_TYPE: &str = "CollectionMeta";
const AUDIT_ENTRY_TYPE: &str = "AuditEntry";
const AUDIT_EDGE_TYPE: &str = "AuditEdge";
const AUDIT_CONNECTION_TYPE: &str = "AuditConnection";
const LINK_CANDIDATE_TYPE: &str = "LinkCandidate";
const LINK_CANDIDATES_PAYLOAD_TYPE: &str = "LinkCandidatesPayload";
const NEIGHBOR_EDGE_TYPE: &str = "NeighborEdge";
const NEIGHBOR_GROUP_TYPE: &str = "NeighborGroup";
const NEIGHBORS_CONNECTION_TYPE: &str = "NeighborsConnection";
const CREATE_COLLECTION_INPUT: &str = "CreateCollectionInput";
const DROP_COLLECTION_INPUT: &str = "DropCollectionInput";
const PUT_SCHEMA_INPUT: &str = "PutSchemaInput";
const DROP_COLLECTION_PAYLOAD: &str = "DropCollectionPayload";
const PUT_SCHEMA_PAYLOAD: &str = "PutSchemaPayload";
const COMMIT_TRANSACTION_INPUT: &str = "CommitTransactionInput";
const TRANSACTION_OPERATION_INPUT: &str = "TransactionOperationInput";
const CREATE_ENTITY_TRANSACTION_INPUT: &str = "CreateEntityTransactionInput";
const UPDATE_ENTITY_TRANSACTION_INPUT: &str = "UpdateEntityTransactionInput";
const PATCH_ENTITY_TRANSACTION_INPUT: &str = "PatchEntityTransactionInput";
const DELETE_ENTITY_TRANSACTION_INPUT: &str = "DeleteEntityTransactionInput";
const CREATE_LINK_TRANSACTION_INPUT: &str = "CreateLinkTransactionInput";
const DELETE_LINK_TRANSACTION_INPUT: &str = "DeleteLinkTransactionInput";
const COMMIT_TRANSACTION_PAYLOAD: &str = "CommitTransactionPayload";
const TRANSACTION_OPERATION_RESULT: &str = "TransactionOperationResult";
const DEFAULT_MAX_GRAPHQL_DEPTH: usize = 10;
const DEFAULT_MAX_GRAPHQL_COMPLEXITY: usize = 256;
const MAX_DEPTH_ENV: &str = "AXON_GRAPHQL_MAX_DEPTH";
const MAX_COMPLEXITY_ENV: &str = "AXON_GRAPHQL_MAX_COMPLEXITY";
const IDEMPOTENCY_TTL: Duration = Duration::from_secs(5 * 60);

static GRAPHQL_IDEMPOTENCY_CACHE: OnceLock<StdMutex<HashMap<(String, String), IdempotencyEntry>>> =
    OnceLock::new();

#[derive(Clone, Debug)]
pub struct GraphqlIdempotencyScope(pub String);

#[derive(Clone, Debug)]
struct IdempotencyEntry {
    response: Value,
    expires_at: Instant,
}

fn graphql_idempotency_cache() -> &'static StdMutex<HashMap<(String, String), IdempotencyEntry>> {
    GRAPHQL_IDEMPOTENCY_CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

// ── Entity → GraphQL FieldValue conversion ──────────────────────────────────

fn entity_to_field_value_with_schema(
    entity: &Entity,
    schema: Option<&CollectionSchema>,
) -> FieldValue<'static> {
    json_to_field_value(entity_to_typed_json_with_schema(entity, schema))
}

fn entity_to_typed_json_with_schema(entity: &Entity, schema: Option<&CollectionSchema>) -> Value {
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
    map.insert(
        "lifecycles".into(),
        lifecycle_metadata_json(schema, &entity.data),
    );

    Value::Object(map)
}

fn entity_to_generic_json(entity: &Entity) -> Value {
    entity_to_generic_json_with_schema(entity, None)
}

fn entity_to_generic_json_with_schema(entity: &Entity, schema: Option<&CollectionSchema>) -> Value {
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
    map.insert(
        "lifecycles".into(),
        lifecycle_metadata_json(schema, &entity.data),
    );
    Value::Object(map)
}

fn lifecycle_metadata_json(schema: Option<&CollectionSchema>, data: &Value) -> Value {
    let Some(schema) = schema else {
        return json!({});
    };
    let mut lifecycles = serde_json::Map::new();
    for (name, lifecycle) in &schema.lifecycles {
        let current_state = data
            .get(&lifecycle.field)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let valid_transitions = current_state
            .as_deref()
            .and_then(|state| lifecycle.transitions.get(state))
            .cloned()
            .unwrap_or_default();
        lifecycles.insert(
            name.clone(),
            json!({
                "field": lifecycle.field,
                "initial": lifecycle.initial,
                "currentState": current_state,
                "validTransitions": valid_transitions,
            }),
        );
    }
    Value::Object(lifecycles)
}

fn lifecycle_valid_transitions_from_parent(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    let lifecycle_name = ctx.args.try_get("lifecycleName")?.string()?.to_owned();
    match ctx.parent_value.try_to_value() {
        Ok(GqlValue::Object(map)) => {
            let transitions = map
                .get(&async_graphql::Name::new("lifecycles"))
                .and_then(|lifecycles| lifecycles.clone().into_json().ok())
                .and_then(|lifecycles| lifecycles.get(&lifecycle_name).cloned())
                .and_then(|metadata| metadata.get("validTransitions").cloned())
                .unwrap_or_else(|| json!([]));
            Ok(Some(json_to_field_value(transitions)))
        }
        _ => Ok(Some(json_to_field_value(json!([])))),
    }
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

fn json_object_field(name: impl Into<String>, ty: TypeRef) -> Field {
    let name = name.into();
    let lookup_name = name.clone();
    Field::new(name, ty, move |ctx| {
        let lookup_name = lookup_name.clone();
        FieldFuture::new(async move { Ok(parent_json_field(ctx, &lookup_name)) })
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

fn scalar_filter_input_objects() -> Vec<InputObject> {
    vec![
        operator_filter_input_object(STRING_FILTER_INPUT, TypeRef::STRING, true),
        operator_filter_input_object(INT_FILTER_INPUT, TypeRef::INT, false),
        operator_filter_input_object(FLOAT_FILTER_INPUT, TypeRef::FLOAT, false),
        operator_filter_input_object(BOOLEAN_FILTER_INPUT, TypeRef::BOOLEAN, false),
        operator_filter_input_object(JSON_FILTER_INPUT, "JSON", true),
    ]
}

fn operator_filter_input_object(name: &str, scalar: &str, contains: bool) -> InputObject {
    let mut input = InputObject::new(name)
        .field(InputValue::new("eq", TypeRef::named(scalar)))
        .field(InputValue::new("ne", TypeRef::named(scalar)))
        .field(InputValue::new("in", TypeRef::named_nn_list(scalar)))
        .field(InputValue::new("isNull", TypeRef::named(TypeRef::BOOLEAN)))
        .field(InputValue::new(
            "isNotNull",
            TypeRef::named(TypeRef::BOOLEAN),
        ));
    if scalar == TypeRef::INT || scalar == TypeRef::FLOAT {
        input = input
            .field(InputValue::new("gt", TypeRef::named(scalar)))
            .field(InputValue::new("gte", TypeRef::named(scalar)))
            .field(InputValue::new("lt", TypeRef::named(scalar)))
            .field(InputValue::new("lte", TypeRef::named(scalar)));
    }
    if contains {
        input = input.field(InputValue::new("contains", TypeRef::named(scalar)));
    }
    input
}

fn typed_filter_input_object(name: &str, fields: &[(String, String, bool)]) -> InputObject {
    let mut input = InputObject::new(name)
        .field(InputValue::new("field", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new("op", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new("value", TypeRef::named("JSON")))
        .field(InputValue::new("and", TypeRef::named_nn_list(name)))
        .field(InputValue::new("or", TypeRef::named_nn_list(name)))
        .field(InputValue::new("gate", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new("pass", TypeRef::named(TypeRef::BOOLEAN)));

    for (field_name, gql_type, _) in fields {
        input = input.field(InputValue::new(
            field_name,
            TypeRef::named(filter_input_name_for_type(gql_type)),
        ));
    }
    input
}

fn typed_sort_field_enum(name: &str, fields: &[(String, String, bool)]) -> Enum {
    let mut sort_enum = Enum::new(name).item("id").item("version");
    for (field_name, _, _) in fields {
        sort_enum = sort_enum.item(field_name);
    }
    sort_enum
}

fn typed_sort_input_object(name: &str, sort_field_enum: &str) -> InputObject {
    InputObject::new(name)
        .field(InputValue::new("field", TypeRef::named_nn(sort_field_enum)))
        .field(InputValue::new(
            "direction",
            TypeRef::named(TypeRef::STRING),
        ))
}

fn aggregate_function_enum() -> Enum {
    Enum::new(AGGREGATE_FUNCTION_ENUM)
        .item("COUNT")
        .item("SUM")
        .item("AVG")
        .item("MIN")
        .item("MAX")
}

fn aggregate_input_object(name: &str, field_enum: &str) -> InputObject {
    InputObject::new(name)
        .field(InputValue::new(
            "function",
            TypeRef::named_nn(AGGREGATE_FUNCTION_ENUM),
        ))
        .field(InputValue::new("field", TypeRef::named(field_enum)))
}

fn aggregate_value_object() -> Object {
    Object::new(AGGREGATE_VALUE_TYPE)
        .field(json_object_field(
            "function",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("field", TypeRef::named(TypeRef::STRING)))
        .field(json_object_field("value", TypeRef::named("JSON")))
        .field(json_object_field("count", TypeRef::named_nn(TypeRef::INT)))
}

fn aggregate_group_object(name: &str) -> Object {
    Object::new(name)
        .field(json_object_field("key", TypeRef::named("JSON")))
        .field(json_object_field("keyFields", TypeRef::named("JSON")))
        .field(json_object_field("count", TypeRef::named_nn(TypeRef::INT)))
        .field(json_object_field(
            "values",
            TypeRef::named_nn_list(AGGREGATE_VALUE_TYPE),
        ))
}

fn aggregate_result_object(name: &str, group_type: &str) -> Object {
    Object::new(name)
        .field(json_object_field(
            "totalCount",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "groups",
            TypeRef::named_nn_list(group_type),
        ))
}

fn typed_entity_input_object(
    name: &str,
    fields: &[(String, String, bool)],
    required_fields: bool,
) -> InputObject {
    let mut input = InputObject::new(name);
    for (field_name, gql_type, required) in fields {
        input = input.field(InputValue::new(
            field_name,
            input_type_ref_for_field(gql_type, required_fields && *required),
        ));
    }
    input
}

fn patch_entity_input_object(name: &str) -> InputObject {
    InputObject::new(name).field(InputValue::new("patch", TypeRef::named_nn("JSON")))
}

fn delete_entity_input_object(name: &str) -> InputObject {
    InputObject::new(name)
        .field(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new("version", TypeRef::named(TypeRef::INT)))
}

fn typed_entity_payload_object(
    name: &str,
    entity_type: &str,
    fields: &[(String, String, bool)],
) -> Object {
    let mut obj = Object::new(name)
        .field(json_object_field("entity", TypeRef::named(entity_type)))
        .field(json_object_field("id", TypeRef::named_nn(TypeRef::ID)))
        .field(json_object_field(
            "version",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "createdAt",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "updatedAt",
            TypeRef::named(TypeRef::STRING),
        ));
    for (field_name, gql_type, _) in fields {
        obj = obj.field(json_object_field(field_name, parse_type_ref(gql_type)));
    }
    add_entity_lifecycle_fields(obj)
}

fn delete_entity_payload_object(name: &str, entity_type: &str) -> Object {
    Object::new(name)
        .field(json_object_field(
            "deleted",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field("id", TypeRef::named_nn(TypeRef::ID)))
        .field(json_object_field("entity", TypeRef::named(entity_type)))
}

fn typed_entity_payload_value(entity: &Entity, schema: Option<&CollectionSchema>) -> Value {
    let entity_json = entity_to_typed_json_with_schema(entity, schema);
    let mut payload = entity_json.as_object().cloned().unwrap_or_default();
    payload.insert("entity".into(), entity_json);
    Value::Object(payload)
}

fn is_system_entity_field(field_name: &str) -> bool {
    matches!(field_name, "id" | "version" | "createdAt" | "updatedAt")
}

fn filter_input_name_for_type(gql_type: &str) -> &'static str {
    match gql_type.trim_end_matches('!') {
        TypeRef::STRING | TypeRef::ID => STRING_FILTER_INPUT,
        TypeRef::INT => INT_FILTER_INPUT,
        TypeRef::FLOAT => FLOAT_FILTER_INPUT,
        TypeRef::BOOLEAN => BOOLEAN_FILTER_INPUT,
        _ => JSON_FILTER_INPUT,
    }
}

fn input_type_ref_for_field(gql_type: &str, required: bool) -> TypeRef {
    let base = gql_type.trim_end_matches('!');
    if required {
        TypeRef::named_nn(base)
    } else {
        TypeRef::named(base)
    }
}

fn create_collection_input_object() -> InputObject {
    InputObject::new(CREATE_COLLECTION_INPUT)
        .field(InputValue::new("name", TypeRef::named_nn(TypeRef::STRING)))
        .field(InputValue::new("schema", TypeRef::named_nn("JSON")))
}

fn drop_collection_input_object() -> InputObject {
    InputObject::new(DROP_COLLECTION_INPUT)
        .field(InputValue::new("name", TypeRef::named_nn(TypeRef::STRING)))
        .field(InputValue::new(
            "confirm",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
}

fn put_schema_input_object() -> InputObject {
    InputObject::new(PUT_SCHEMA_INPUT)
        .field(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("schema", TypeRef::named_nn("JSON")))
        .field(InputValue::new("force", TypeRef::named(TypeRef::BOOLEAN)))
        .field(InputValue::new("dryRun", TypeRef::named(TypeRef::BOOLEAN)))
}

fn commit_transaction_input_object() -> InputObject {
    InputObject::new(COMMIT_TRANSACTION_INPUT)
        .field(InputValue::new(
            "idempotencyKey",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(InputValue::new(
            "operations",
            TypeRef::named_nn_list_nn(TRANSACTION_OPERATION_INPUT),
        ))
}

fn transaction_operation_input_object() -> InputObject {
    InputObject::new(TRANSACTION_OPERATION_INPUT)
        .field(InputValue::new(
            "createEntity",
            TypeRef::named(CREATE_ENTITY_TRANSACTION_INPUT),
        ))
        .field(InputValue::new(
            "updateEntity",
            TypeRef::named(UPDATE_ENTITY_TRANSACTION_INPUT),
        ))
        .field(InputValue::new(
            "patchEntity",
            TypeRef::named(PATCH_ENTITY_TRANSACTION_INPUT),
        ))
        .field(InputValue::new(
            "deleteEntity",
            TypeRef::named(DELETE_ENTITY_TRANSACTION_INPUT),
        ))
        .field(InputValue::new(
            "createLink",
            TypeRef::named(CREATE_LINK_TRANSACTION_INPUT),
        ))
        .field(InputValue::new(
            "deleteLink",
            TypeRef::named(DELETE_LINK_TRANSACTION_INPUT),
        ))
}

fn create_entity_transaction_input_object() -> InputObject {
    InputObject::new(CREATE_ENTITY_TRANSACTION_INPUT)
        .field(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new("data", TypeRef::named_nn("JSON")))
}

fn update_entity_transaction_input_object() -> InputObject {
    InputObject::new(UPDATE_ENTITY_TRANSACTION_INPUT)
        .field(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "expectedVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(InputValue::new("data", TypeRef::named_nn("JSON")))
}

fn patch_entity_transaction_input_object() -> InputObject {
    InputObject::new(PATCH_ENTITY_TRANSACTION_INPUT)
        .field(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "expectedVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(InputValue::new("patch", TypeRef::named_nn("JSON")))
}

fn delete_entity_transaction_input_object() -> InputObject {
    InputObject::new(DELETE_ENTITY_TRANSACTION_INPUT)
        .field(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "expectedVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
}

fn create_link_transaction_input_object() -> InputObject {
    InputObject::new(CREATE_LINK_TRANSACTION_INPUT)
        .field(InputValue::new(
            "sourceCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("sourceId", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "targetCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("targetId", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("metadata", TypeRef::named("JSON")))
}

fn delete_link_transaction_input_object() -> InputObject {
    InputObject::new(DELETE_LINK_TRANSACTION_INPUT)
        .field(InputValue::new(
            "sourceCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("sourceId", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "targetCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("targetId", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
}

fn gql_input_to_json(value: &GqlValue) -> Result<Value, GqlError> {
    value
        .clone()
        .into_json()
        .map_err(|e| GqlError::new(format!("invalid GraphQL input value: {e}")))
}

fn gql_json_or_legacy_string_arg(value: &GqlValue, name: &str) -> Result<Value, GqlError> {
    let json = gql_input_to_json(value)?;
    match json {
        Value::String(input) => serde_json::from_str(&input)
            .map_err(|e| GqlError::new(format!("invalid JSON {name}: {e}"))),
        other => Ok(other),
    }
}

fn mutation_data_arg(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
    input_name: &str,
    legacy_name: &str,
) -> Result<Value, GqlError> {
    if let Ok(input) = ctx.args.try_get(input_name) {
        return gql_input_to_json(input.as_value());
    }
    if let Ok(legacy) = ctx.args.try_get(legacy_name) {
        return gql_json_or_legacy_string_arg(legacy.as_value(), legacy_name);
    }
    Err(
        GqlError::new(format!("{input_name} or {legacy_name} is required")).extend_with(
            |_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
            },
        ),
    )
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

    let Some(field_value) = obj.get("field") else {
        return parse_typed_filter_fields(obj);
    };
    let field = field_value
        .as_str()
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

fn parse_typed_filter_fields(obj: &serde_json::Map<String, Value>) -> Result<FilterNode, GqlError> {
    let mut filters = Vec::new();
    for (field, predicate) in obj {
        if matches!(field.as_str(), "and" | "or" | "gate" | "pass") || predicate.is_null() {
            continue;
        }
        let predicate = predicate
            .as_object()
            .ok_or_else(|| GqlError::new(format!("filter.{field} must be an object")))?;
        for (op, value) in predicate {
            if value.is_null() {
                continue;
            }
            let filter = typed_filter_op(field, op, value)?;
            filters.push(filter);
        }
    }

    match filters.len() {
        0 => Err(GqlError::new("filter must contain at least one predicate")),
        1 => Ok(filters.remove(0)),
        _ => Ok(FilterNode::And { filters }),
    }
}

fn typed_filter_op(field: &str, op: &str, value: &Value) -> Result<FilterNode, GqlError> {
    let (op, value) = match op {
        "eq" => (FilterOp::Eq, value.clone()),
        "ne" => (FilterOp::Ne, value.clone()),
        "gt" => (FilterOp::Gt, value.clone()),
        "gte" => (FilterOp::Gte, value.clone()),
        "lt" => (FilterOp::Lt, value.clone()),
        "lte" => (FilterOp::Lte, value.clone()),
        "in" => (FilterOp::In, value.clone()),
        "contains" => (FilterOp::Contains, value.clone()),
        "isNull" if value.as_bool().unwrap_or(false) => (FilterOp::Eq, Value::Null),
        "isNotNull" if value.as_bool().unwrap_or(false) => (FilterOp::Ne, Value::Null),
        "isNull" | "isNotNull" => {
            return Err(GqlError::new(format!(
                "filter.{field}.{op} must be true when present"
            )))
        }
        _ => return Err(GqlError::new(format!("unsupported filter operator '{op}'"))),
    };
    Ok(FilterNode::Field(FieldFilter {
        field: field.to_string(),
        op,
        value,
    }))
}

fn parse_graphql_filter_list(value: &Value, name: &str) -> Result<Vec<FilterNode>, GqlError> {
    let items = value
        .as_array()
        .ok_or_else(|| GqlError::new(format!("filter.{name} must be a list")))?;
    items.iter().map(parse_graphql_filter_json).collect()
}

#[derive(Debug, Clone, Copy)]
enum GraphqlAggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl GraphqlAggregateFunction {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "COUNT" => Some(Self::Count),
            "SUM" => Some(Self::Sum),
            "AVG" => Some(Self::Avg),
            "MIN" => Some(Self::Min),
            "MAX" => Some(Self::Max),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }

    fn is_numeric(self) -> bool {
        !matches!(self, Self::Count)
    }
}

#[derive(Debug, Clone)]
struct GraphqlAggregationSpec {
    function: GraphqlAggregateFunction,
    field: Option<String>,
}

fn parse_graphql_group_by_arg(value: &GqlValue) -> Result<Vec<String>, GqlError> {
    let json = gql_input_to_json(value)?;
    let items = json
        .as_array()
        .ok_or_else(|| invalid_aggregate_argument("groupBy must be a list"))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| invalid_aggregate_argument("groupBy entries must be fields"))
        })
        .collect()
}

fn parse_graphql_aggregations_arg(
    value: &GqlValue,
) -> Result<Vec<GraphqlAggregationSpec>, GqlError> {
    let json = gql_input_to_json(value)?;
    let items = json
        .as_array()
        .ok_or_else(|| invalid_aggregate_argument("aggregations must be a list"))?;
    if items.is_empty() {
        return Err(invalid_aggregate_argument(
            "aggregations must contain at least one entry",
        ));
    }

    let mut specs = Vec::with_capacity(items.len());
    for item in items {
        let obj = item
            .as_object()
            .ok_or_else(|| invalid_aggregate_argument("aggregation entries must be objects"))?;
        let function = obj
            .get("function")
            .and_then(Value::as_str)
            .and_then(GraphqlAggregateFunction::parse)
            .ok_or_else(|| invalid_aggregate_argument("unknown aggregation function"))?;
        let field = obj
            .get("field")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        if function.is_numeric() && field.is_none() {
            return Err(invalid_aggregate_argument(
                "numeric aggregations require a field",
            ));
        }
        specs.push(GraphqlAggregationSpec { function, field });
    }
    Ok(specs)
}

fn invalid_aggregate_argument(message: impl Into<String>) -> GqlError {
    GqlError::new(message.into()).extend_with(|_err, ext| {
        ext.set("code", "INVALID_ARGUMENT");
        ext.set("category", "AGGREGATION");
    })
}

fn graphql_aggregate_response(
    entities: &[Entity],
    total_count: usize,
    group_by: &[String],
    specs: &[GraphqlAggregationSpec],
) -> Result<Value, GqlError> {
    let mut groups: BTreeMap<String, (Value, Value, Vec<&Entity>)> = BTreeMap::new();

    for entity in entities {
        let (key, key_fields) = aggregate_group_key(entity, group_by);
        let map_key = serde_json::to_string(&key_fields).map_err(|e| {
            invalid_aggregate_argument(format!("failed to serialize group key: {e}"))
        })?;
        groups
            .entry(map_key)
            .or_insert_with(|| (key, key_fields, Vec::new()))
            .2
            .push(entity);
    }

    let mut group_values = Vec::with_capacity(groups.len());
    for (_, (key, key_fields, group_entities)) in groups {
        let count = group_entities.len();
        let mut values = Vec::with_capacity(specs.len());
        for spec in specs {
            values.push(aggregate_spec_value(spec, &group_entities)?);
        }
        group_values.push(json!({
            "key": key,
            "keyFields": key_fields,
            "count": count,
            "values": values,
        }));
    }

    Ok(json!({
        "totalCount": total_count,
        "groups": group_values,
    }))
}

fn aggregate_group_key(entity: &Entity, group_by: &[String]) -> (Value, Value) {
    if group_by.is_empty() {
        return (Value::Null, json!({}));
    }

    let mut key_fields = serde_json::Map::new();
    for field in group_by {
        key_fields.insert(
            field.clone(),
            entity_field_value(entity, field).unwrap_or(Value::Null),
        );
    }

    let key = if group_by.len() == 1 {
        key_fields.get(&group_by[0]).cloned().unwrap_or(Value::Null)
    } else {
        Value::Object(key_fields.clone())
    };
    (key, Value::Object(key_fields))
}

fn aggregate_spec_value(
    spec: &GraphqlAggregationSpec,
    entities: &[&Entity],
) -> Result<Value, GqlError> {
    if matches!(spec.function, GraphqlAggregateFunction::Count) {
        return Ok(json!({
            "function": spec.function.as_str(),
            "field": Value::Null,
            "value": entities.len(),
            "count": entities.len(),
        }));
    }

    let field = spec
        .field
        .as_deref()
        .ok_or_else(|| invalid_aggregate_argument("numeric aggregations require a field"))?;
    let mut numbers = Vec::new();
    for entity in entities {
        match entity_field_value(entity, field) {
            Some(value) if value.is_number() => {
                if let Some(number) = value.as_f64() {
                    numbers.push(number);
                }
            }
            Some(Value::Null) | None => {}
            Some(_) => {
                return Err(
                    invalid_aggregate_argument(format!("field '{field}' is not numeric"))
                        .extend_with(|_err, ext| {
                            ext.set("field", field);
                            ext.set("function", spec.function.as_str());
                        }),
                );
            }
        }
    }

    let value = if numbers.is_empty() {
        Value::Null
    } else {
        json!(compute_graphql_aggregate(spec.function, &numbers))
    };
    Ok(json!({
        "function": spec.function.as_str(),
        "field": field,
        "value": value,
        "count": numbers.len(),
    }))
}

fn compute_graphql_aggregate(function: GraphqlAggregateFunction, values: &[f64]) -> f64 {
    match function {
        GraphqlAggregateFunction::Count => len_as_f64(values.len()),
        GraphqlAggregateFunction::Sum => values.iter().sum(),
        GraphqlAggregateFunction::Avg => values.iter().sum::<f64>() / len_as_f64(values.len()),
        GraphqlAggregateFunction::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
        GraphqlAggregateFunction::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

fn len_as_f64(len: usize) -> f64 {
    u32::try_from(len).map_or_else(|_| f64::from(u32::MAX), f64::from)
}

fn entity_field_value(entity: &Entity, field: &str) -> Option<Value> {
    match field {
        "id" => Some(Value::String(entity.id.to_string())),
        "version" => Some(json!(entity.version)),
        "createdAt" => entity.created_at_ns.map(|ns| Value::String(format_ns(ns))),
        "updatedAt" => entity.updated_at_ns.map(|ns| Value::String(format_ns(ns))),
        _ => {
            let mut value = &entity.data;
            for part in field.split('.') {
                value = value.get(part)?;
            }
            Some(value.clone())
        }
    }
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
    schema: Option<&CollectionSchema>,
) -> FieldValue<'static> {
    let edges: Vec<Value> = entities
        .iter()
        .map(|entity| {
            json!({
                "cursor": entity.id.to_string(),
                "node": if generic_node {
                    entity_to_generic_json_with_schema(entity, schema)
                } else {
                    entity_to_typed_json_with_schema(entity, schema)
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

fn unsupported_audit_filter_arg(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
) -> Option<&'static str> {
    ["metadataPath", "metadataEq", "dataAfterPath", "dataAfterEq"]
        .into_iter()
        .find(|name| ctx.args.try_get(name).is_ok())
}

fn unsupported_audit_filter_error(filter: &'static str) -> GqlError {
    GqlError::new(format!("unsupported audit filter: {filter}")).extend_with(move |_err, ext| {
        ext.set("code", "UNSUPPORTED_AUDIT_FILTER");
        ext.set("filter", filter);
    })
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
        .field(json_object_field("lifecycles", TypeRef::named("JSON")))
        .field(
            Field::new(
                "validTransitions",
                TypeRef::named_nn_list_nn(TypeRef::STRING),
                |ctx| FieldFuture::new(async move { lifecycle_valid_transitions_from_parent(ctx) }),
            )
            .argument(InputValue::new(
                "lifecycleName",
                TypeRef::named_nn(TypeRef::STRING),
            )),
        )
}

fn add_entity_lifecycle_fields(obj: Object) -> Object {
    obj.field(json_object_field("lifecycles", TypeRef::named("JSON")))
        .field(
            Field::new(
                "validTransitions",
                TypeRef::named_nn_list_nn(TypeRef::STRING),
                |ctx| FieldFuture::new(async move { lifecycle_valid_transitions_from_parent(ctx) }),
            )
            .argument(InputValue::new(
                "lifecycleName",
                TypeRef::named_nn(TypeRef::STRING),
            )),
        )
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

fn relationship_edge_object(edge_type: &str, node_type: &str) -> Object {
    Object::new(edge_type)
        .field(json_object_field(
            "cursor",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("node", TypeRef::named_nn(node_type)))
        .field(json_object_field(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("metadata", TypeRef::named("JSON")))
        .field(json_object_field(
            "sourceCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "sourceId",
            TypeRef::named_nn(TypeRef::ID),
        ))
        .field(json_object_field(
            "targetCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "targetId",
            TypeRef::named_nn(TypeRef::ID),
        ))
}

fn relationship_connection_value(
    entities: &[Entity],
    links: &[Link],
    schema: &CollectionSchema,
    limit: Option<usize>,
    after: Option<&str>,
) -> Result<FieldValue<'static>, GqlError> {
    let pairs: Vec<(&Entity, &Link)> = entities.iter().zip(links.iter()).collect();
    let start_index = match after {
        Some(cursor) => pairs
            .iter()
            .position(|(entity, _)| entity.id.as_str() == cursor)
            .map(|index| index + 1)
            .ok_or_else(|| {
                GqlError::new(format!("relationship cursor '{cursor}' was not found")).extend_with(
                    |_err, ext| {
                        ext.set("code", "INVALID_ARGUMENT");
                    },
                )
            })?,
        None => 0,
    };
    let page_limit = limit.unwrap_or(100);
    let page: Vec<(&Entity, &Link)> = pairs
        .iter()
        .skip(start_index)
        .take(page_limit)
        .copied()
        .collect();
    let has_next_page = start_index + page.len() < pairs.len();
    let edges: Vec<Value> = page
        .iter()
        .map(|(entity, link)| {
            json!({
                "cursor": entity.id.to_string(),
                "node": entity_to_typed_json_with_schema(entity, Some(schema)),
                "linkType": link.link_type,
                "metadata": link.metadata,
                "sourceCollection": link.source_collection.to_string(),
                "sourceId": link.source_id.to_string(),
                "targetCollection": link.target_collection.to_string(),
                "targetId": link.target_id.to_string(),
            })
        })
        .collect();
    let start_cursor = page.first().map(|(entity, _)| entity.id.to_string());
    let end_cursor = page.last().map(|(entity, _)| entity.id.to_string());

    Ok(json_to_field_value(json!({
        "edges": edges,
        "pageInfo": page_info_json(
            start_cursor,
            end_cursor,
            has_next_page,
            after.is_some(),
        ),
        "totalCount": pairs.len(),
    })))
}

fn parent_id_arg(ctx: &async_graphql::dynamic::ResolverContext<'_>) -> Result<String, GqlError> {
    match ctx.parent_value.try_to_value() {
        Ok(GqlValue::Object(map)) => map
            .get(&async_graphql::Name::new("id"))
            .and_then(|value| value.clone().into_json().ok())
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .ok_or_else(|| GqlError::new("parent entity id is missing")),
        _ => Err(GqlError::new(
            "relationship parent must be an entity object",
        )),
    }
}

fn parse_relationship_limit(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
) -> Result<Option<usize>, GqlError> {
    match ctx.args.try_get("limit") {
        Ok(value) => {
            let limit = value.i64()?;
            if limit < 0 {
                return Err(GqlError::new("limit must be non-negative").extend_with(
                    |_err, ext| {
                        ext.set("code", "INVALID_ARGUMENT");
                    },
                ));
            }
            Ok(Some(limit as usize))
        }
        Err(_) => Ok(None),
    }
}

fn parse_relationship_after(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
) -> Result<Option<String>, GqlError> {
    if let Ok(value) = ctx.args.try_get("after") {
        return Ok(Some(value.string()?.to_owned()));
    }
    if let Ok(value) = ctx.args.try_get("afterId") {
        return Ok(Some(value.string()?.to_owned()));
    }
    Ok(None)
}

#[derive(Clone)]
struct RelationshipFieldSpec {
    collection: String,
    link_type: String,
    direction: TraverseDirection,
    expected_source_collection: String,
    expected_target_collection: String,
    node_schema: CollectionSchema,
}

fn relationship_field<S: StorageAdapter + 'static>(
    field_name: &str,
    connection_type: &str,
    filter_input_type: &str,
    handler: SharedHandler<S>,
    spec: RelationshipFieldSpec,
) -> Field {
    let connection_type_ref = connection_type.to_owned();
    let filter_input_type_ref = filter_input_type.to_owned();
    Field::new(
        field_name,
        TypeRef::named_nn(&connection_type_ref),
        move |ctx| {
            let handler = Arc::clone(&handler);
            let spec = spec.clone();
            FieldFuture::new(async move {
                let parent_id = parent_id_arg(&ctx)?;
                let limit = parse_relationship_limit(&ctx)?;
                let after = parse_relationship_after(&ctx)?;
                let hop_filter = ctx
                    .args
                    .try_get("filter")
                    .ok()
                    .map(|value| parse_graphql_filter_arg(value.as_value()))
                    .transpose()?;

                let guard = handler.lock().await;
                let response = guard.traverse(TraverseRequest {
                    collection: CollectionId::new(spec.collection.clone()),
                    id: EntityId::new(parent_id),
                    link_type: Some(spec.link_type.clone()),
                    max_depth: Some(1),
                    direction: spec.direction.clone(),
                    hop_filter,
                });
                drop(guard);

                let response = response.map_err(axon_error_to_gql)?;
                let pairs: Vec<(Entity, Link)> = response
                    .entities
                    .into_iter()
                    .zip(response.links)
                    .filter(|(_, link)| {
                        link.source_collection.as_str() == spec.expected_source_collection
                            && link.target_collection.as_str() == spec.expected_target_collection
                    })
                    .collect();
                let entities: Vec<Entity> =
                    pairs.iter().map(|(entity, _)| entity.clone()).collect();
                let links: Vec<Link> = pairs.iter().map(|(_, link)| link.clone()).collect();

                relationship_connection_value(
                    &entities,
                    &links,
                    &spec.node_schema,
                    limit,
                    after.as_deref(),
                )
                .map(Some)
            })
        },
    )
    .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
    .argument(InputValue::new("after", TypeRef::named(TypeRef::ID)))
    .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID)))
    .argument(InputValue::new(
        "filter",
        TypeRef::named(&filter_input_type_ref),
    ))
}

fn entity_matches_search(entity: &Entity, search: &str) -> bool {
    let needle = search.to_ascii_lowercase();
    if entity.id.as_str().to_ascii_lowercase().contains(&needle) {
        return true;
    }
    serde_json::to_string(&entity.data)
        .map(|data| data.to_ascii_lowercase().contains(&needle))
        .unwrap_or(false)
}

fn link_candidates_value(
    response: axon_api::response::FindLinkCandidatesResponse,
    schema: Option<&CollectionSchema>,
    search: Option<&str>,
    limit: Option<usize>,
) -> FieldValue<'static> {
    let mut candidates = response.candidates;
    if let Some(search) = search.filter(|search| !search.is_empty()) {
        candidates.retain(|candidate| entity_matches_search(&candidate.entity, search));
    }
    let limit = limit.unwrap_or(50);
    let candidates: Vec<Value> = candidates
        .into_iter()
        .take(limit)
        .map(|candidate| {
            json!({
                "alreadyLinked": candidate.already_linked,
                "entity": entity_to_generic_json_with_schema(&candidate.entity, schema),
            })
        })
        .collect();

    json_to_field_value(json!({
        "targetCollection": response.target_collection,
        "linkType": response.link_type,
        "cardinality": response.cardinality,
        "existingLinkCount": response.existing_link_count,
        "candidates": candidates,
    }))
}

#[derive(Clone)]
struct NeighborEdgePayload {
    entity: Entity,
    link: Link,
    direction: String,
}

impl NeighborEdgePayload {
    fn cursor(&self) -> String {
        format!(
            "{}:{}:{}/{}/{}/{}",
            self.direction,
            self.link.link_type,
            self.link.source_collection,
            self.link.source_id,
            self.link.target_collection,
            self.link.target_id,
        )
    }
}

fn parse_neighbor_direction(direction: &str) -> Result<TraverseDirection, GqlError> {
    match direction.to_ascii_lowercase().as_str() {
        "forward" | "outbound" => Ok(TraverseDirection::Forward),
        "reverse" | "inbound" => Ok(TraverseDirection::Reverse),
        other => Err(GqlError::new(format!(
            "direction must be forward/outbound or reverse/inbound, got '{other}'"
        ))
        .extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })),
    }
}

fn neighbor_connection_value(
    edges: &[NeighborEdgePayload],
    schemas: &HashMap<String, Option<CollectionSchema>>,
    limit: Option<usize>,
    after: Option<&str>,
) -> Result<FieldValue<'static>, GqlError> {
    let start_index = match after {
        Some(cursor) => edges
            .iter()
            .position(|edge| edge.cursor() == cursor)
            .map(|index| index + 1)
            .ok_or_else(|| {
                GqlError::new(format!("neighbor cursor '{cursor}' was not found")).extend_with(
                    |_err, ext| {
                        ext.set("code", "INVALID_ARGUMENT");
                    },
                )
            })?,
        None => 0,
    };
    let page_limit = limit.unwrap_or(100);
    let page: Vec<&NeighborEdgePayload> = edges.iter().skip(start_index).take(page_limit).collect();
    let mut group_totals: BTreeMap<(String, String), usize> = BTreeMap::new();
    for edge in edges {
        *group_totals
            .entry((edge.link.link_type.clone(), edge.direction.clone()))
            .or_default() += 1;
    }

    let mut groups: BTreeMap<(String, String), Vec<Value>> = BTreeMap::new();
    for edge in &page {
        let collection = edge.entity.collection.to_string();
        let schema = schemas.get(&collection).and_then(Option::as_ref);
        groups
            .entry((edge.link.link_type.clone(), edge.direction.clone()))
            .or_default()
            .push(json!({
                "cursor": edge.cursor(),
                "node": entity_to_generic_json_with_schema(&edge.entity, schema),
                "linkType": edge.link.link_type.clone(),
                "direction": edge.direction.clone(),
                "metadata": edge.link.metadata.clone(),
                "sourceCollection": edge.link.source_collection.to_string(),
                "sourceId": edge.link.source_id.to_string(),
                "targetCollection": edge.link.target_collection.to_string(),
                "targetId": edge.link.target_id.to_string(),
            }));
    }

    let groups: Vec<Value> = groups
        .into_iter()
        .map(|((link_type, direction), edges)| {
            let total_count = group_totals
                .get(&(link_type.clone(), direction.clone()))
                .copied()
                .unwrap_or(edges.len());
            json!({
                "linkType": link_type,
                "direction": direction,
                "edges": edges,
                "totalCount": total_count,
            })
        })
        .collect();

    let start_cursor = page.first().map(|edge| edge.cursor());
    let end_cursor = page.last().map(|edge| edge.cursor());
    Ok(json_to_field_value(json!({
        "groups": groups,
        "pageInfo": page_info_json(
            start_cursor,
            end_cursor,
            start_index + page.len() < edges.len(),
            after.is_some(),
        ),
        "totalCount": edges.len(),
    })))
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

fn link_candidate_object() -> Object {
    Object::new(LINK_CANDIDATE_TYPE)
        .field(json_object_field(
            "alreadyLinked",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field("entity", TypeRef::named_nn(ENTITY_TYPE)))
}

fn link_candidates_payload_object() -> Object {
    Object::new(LINK_CANDIDATES_PAYLOAD_TYPE)
        .field(json_object_field(
            "targetCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "cardinality",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "existingLinkCount",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "candidates",
            TypeRef::named_nn_list_nn(LINK_CANDIDATE_TYPE),
        ))
}

fn neighbor_edge_object() -> Object {
    Object::new(NEIGHBOR_EDGE_TYPE)
        .field(json_object_field(
            "cursor",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("node", TypeRef::named_nn(ENTITY_TYPE)))
        .field(json_object_field(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "direction",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("metadata", TypeRef::named("JSON")))
        .field(json_object_field(
            "sourceCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "sourceId",
            TypeRef::named_nn(TypeRef::ID),
        ))
        .field(json_object_field(
            "targetCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "targetId",
            TypeRef::named_nn(TypeRef::ID),
        ))
}

fn neighbor_group_object() -> Object {
    Object::new(NEIGHBOR_GROUP_TYPE)
        .field(json_object_field(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "direction",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "edges",
            TypeRef::named_nn_list_nn(NEIGHBOR_EDGE_TYPE),
        ))
        .field(json_object_field(
            "totalCount",
            TypeRef::named_nn(TypeRef::INT),
        ))
}

fn neighbors_connection_object() -> Object {
    Object::new(NEIGHBORS_CONNECTION_TYPE)
        .field(json_object_field(
            "groups",
            TypeRef::named_nn_list_nn(NEIGHBOR_GROUP_TYPE),
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

fn transaction_operation_result_object() -> Object {
    Object::new(TRANSACTION_OPERATION_RESULT)
        .field(json_object_field("index", TypeRef::named_nn(TypeRef::INT)))
        .field(json_object_field(
            "success",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "collection",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field("id", TypeRef::named(TypeRef::ID)))
        .field(json_object_field("entity", TypeRef::named(ENTITY_TYPE)))
        .field(json_object_field("link", TypeRef::named("JSON")))
}

fn commit_transaction_payload_object() -> Object {
    Object::new(COMMIT_TRANSACTION_PAYLOAD)
        .field(json_object_field(
            "transactionId",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "replayHit",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "results",
            TypeRef::named_nn_list_nn(TRANSACTION_OPERATION_RESULT),
        ))
}

fn drop_collection_payload_object() -> Object {
    Object::new(DROP_COLLECTION_PAYLOAD)
        .field(json_object_field(
            "name",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "entitiesRemoved",
            TypeRef::named_nn(TypeRef::INT),
        ))
}

fn put_schema_payload_object() -> Object {
    Object::new(PUT_SCHEMA_PAYLOAD)
        .field(json_object_field("schema", TypeRef::named_nn("JSON")))
        .field(json_object_field("compatibility", TypeRef::named("JSON")))
        .field(json_object_field("diff", TypeRef::named("JSON")))
        .field(json_object_field(
            "dryRun",
            TypeRef::named_nn(TypeRef::BOOLEAN),
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
        .register(audit_connection_object())
        .register(link_candidate_object())
        .register(link_candidates_payload_object())
        .register(neighbor_edge_object())
        .register(neighbor_group_object())
        .register(neighbors_connection_object())
        .register(transaction_operation_result_object())
        .register(commit_transaction_payload_object())
        .register(drop_collection_payload_object())
        .register(put_schema_payload_object());
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
                let collection_id = CollectionId::new(collection);
                let guard = handler.lock().await;
                let schema = guard
                    .get_schema(&collection_id)
                    .map_err(axon_error_to_gql)?;
                match guard.get_entity(GetEntityRequest {
                    collection: collection_id,
                    id: EntityId::new(id),
                }) {
                    Ok(resp) => Ok(Some(json_to_field_value(
                        entity_to_generic_json_with_schema(&resp.entity, schema.as_ref()),
                    ))),
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

                    let collection_id = CollectionId::new(collection);
                    let guard = handler.lock().await;
                    let schema = guard
                        .get_schema(&collection_id)
                        .map_err(axon_error_to_gql)?;
                    match guard.query_entities(QueryEntitiesRequest {
                        collection: collection_id,
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
                            schema.as_ref(),
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

    let handler_link_candidates = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "linkCandidates",
            TypeRef::named_nn(LINK_CANDIDATES_PAYLOAD_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_link_candidates);
                FieldFuture::new(async move {
                    let source_collection =
                        ctx.args.try_get("sourceCollection")?.string()?.to_owned();
                    let source_id = ctx.args.try_get("sourceId")?.string()?.to_owned();
                    let link_type = ctx.args.try_get("linkType")?.string()?.to_owned();
                    let search = ctx
                        .args
                        .try_get("search")
                        .ok()
                        .map(|value| value.string().map(ToOwned::to_owned))
                        .transpose()?;
                    let filter = ctx
                        .args
                        .try_get("filter")
                        .ok()
                        .map(|value| parse_graphql_filter_arg(value.as_value()))
                        .transpose()?;
                    let limit = parse_relationship_limit(&ctx)?;
                    let request_limit = if search.as_deref().is_some_and(|s| !s.is_empty()) {
                        Some(usize::MAX)
                    } else {
                        limit
                    };

                    let guard = handler.lock().await;
                    let response = guard
                        .find_link_candidates(FindLinkCandidatesRequest {
                            source_collection: CollectionId::new(source_collection),
                            source_id: EntityId::new(source_id),
                            link_type,
                            filter,
                            limit: request_limit,
                        })
                        .map_err(axon_error_to_gql)?;
                    let target_collection = CollectionId::new(&response.target_collection);
                    let schema = guard
                        .get_schema(&target_collection)
                        .map_err(axon_error_to_gql)?;
                    Ok(Some(link_candidates_value(
                        response,
                        schema.as_ref(),
                        search.as_deref(),
                        limit,
                    )))
                })
            },
        )
        .argument(InputValue::new(
            "sourceCollection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("sourceId", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "linkType",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("search", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT))),
    );

    let handler_neighbors = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "neighbors",
            TypeRef::named_nn(NEIGHBORS_CONNECTION_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_neighbors);
                FieldFuture::new(async move {
                    let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                    let id = ctx.args.try_get("id")?.string()?.to_owned();
                    let link_type = ctx
                        .args
                        .try_get("linkType")
                        .ok()
                        .map(|value| value.string().map(ToOwned::to_owned))
                        .transpose()?;
                    let direction = ctx
                        .args
                        .try_get("direction")
                        .ok()
                        .map(|value| parse_neighbor_direction(value.string()?))
                        .transpose()?;
                    let limit = parse_relationship_limit(&ctx)?;
                    let after = parse_relationship_after(&ctx)?;
                    let collection_id = CollectionId::new(&collection);
                    let entity_id = EntityId::new(&id);
                    let directions = match direction {
                        Some(TraverseDirection::Forward) => {
                            vec![(TraverseDirection::Forward, "outbound")]
                        }
                        Some(TraverseDirection::Reverse) => {
                            vec![(TraverseDirection::Reverse, "inbound")]
                        }
                        None => vec![
                            (TraverseDirection::Forward, "outbound"),
                            (TraverseDirection::Reverse, "inbound"),
                        ],
                    };

                    let guard = handler.lock().await;
                    guard
                        .get_entity(GetEntityRequest {
                            collection: collection_id.clone(),
                            id: entity_id.clone(),
                        })
                        .map_err(axon_error_to_gql)?;

                    let mut edges = Vec::new();
                    for (direction, label) in directions {
                        let response = guard
                            .traverse(TraverseRequest {
                                collection: collection_id.clone(),
                                id: entity_id.clone(),
                                link_type: link_type.clone(),
                                max_depth: Some(1),
                                direction,
                                hop_filter: None,
                            })
                            .map_err(axon_error_to_gql)?;
                        edges.extend(response.entities.into_iter().zip(response.links).map(
                            |(entity, link)| NeighborEdgePayload {
                                entity,
                                link,
                                direction: label.to_owned(),
                            },
                        ));
                    }

                    let mut schemas = HashMap::new();
                    for edge in &edges {
                        let collection = edge.entity.collection.to_string();
                        if let std::collections::hash_map::Entry::Vacant(entry) =
                            schemas.entry(collection)
                        {
                            let schema = guard
                                .get_schema(&edge.entity.collection)
                                .map_err(axon_error_to_gql)?;
                            entry.insert(schema);
                        }
                    }

                    neighbor_connection_value(&edges, &schemas, limit, after.as_deref()).map(Some)
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new("linkType", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new(
            "direction",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::ID)))
        .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID))),
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
                            .map(|meta| {
                                let schema = guard
                                    .get_schema(&CollectionId::new(&meta.name))
                                    .ok()
                                    .flatten();
                                json_to_field_value(collection_meta_json(meta, schema))
                            })
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
                    if let Some(filter) = unsupported_audit_filter_arg(&ctx) {
                        return Err(unsupported_audit_filter_error(filter));
                    }

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
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new(
            "metadataPath",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "metadataEq",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "dataAfterPath",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "dataAfterEq",
            TypeRef::named(TypeRef::STRING),
        )),
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
                    Ok(Some(entity_connection_value(
                        &[],
                        0,
                        None,
                        false,
                        true,
                        None,
                    )))
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
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new(
            "metadataPath",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "metadataEq",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "dataAfterPath",
            TypeRef::named(TypeRef::STRING),
        ))
        .argument(InputValue::new(
            "dataAfterEq",
            TypeRef::named(TypeRef::STRING),
        )),
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

fn validate_graphql_idempotency_key(key: &str) -> Result<(), GqlError> {
    if key.is_empty() || key.len() > 128 {
        return Err(
            GqlError::new("idempotencyKey length must be 1..128 characters").extend_with(
                |_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                },
            ),
        );
    }
    if !key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
    {
        return Err(
            GqlError::new("idempotencyKey must use ASCII [A-Za-z0-9_.:-] characters").extend_with(
                |_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                },
            ),
        );
    }
    Ok(())
}

fn graphql_idempotency_scope(ctx: &async_graphql::dynamic::ResolverContext<'_>) -> String {
    ctx.data::<GraphqlIdempotencyScope>()
        .map(|scope| scope.0.clone())
        .unwrap_or_else(|_| "default:default".to_string())
}

fn idempotency_cached(scope: &str, key: &str) -> Option<Value> {
    let now = Instant::now();
    let mut cache = graphql_idempotency_cache()
        .lock()
        .expect("graphql idempotency cache mutex poisoned");
    cache.retain(|_, entry| entry.expires_at > now);
    cache
        .get(&(scope.to_string(), key.to_string()))
        .map(|entry| entry.response.clone())
}

fn idempotency_store(scope: &str, key: &str, response: Value) {
    let mut cache = graphql_idempotency_cache()
        .lock()
        .expect("graphql idempotency cache mutex poisoned");
    cache.insert(
        (scope.to_string(), key.to_string()),
        IdempotencyEntry {
            response,
            expires_at: Instant::now() + IDEMPOTENCY_TTL,
        },
    );
}

fn json_merge_patch(target: &mut Value, patch: &Value) {
    if let Value::Object(patch_map) = patch {
        if !target.is_object() {
            *target = Value::Object(serde_json::Map::new());
        }
        if let Value::Object(target_map) = target {
            for (key, value) in patch_map {
                if value.is_null() {
                    target_map.remove(key);
                } else {
                    let entry = target_map.entry(key.clone()).or_insert(Value::Null);
                    json_merge_patch(entry, value);
                }
            }
        }
    } else {
        *target = patch.clone();
    }
}

fn required_object<'a>(
    value: &'a Value,
    name: &str,
    op_index: usize,
) -> Result<&'a serde_json::Map<String, Value>, GqlError> {
    value.as_object().ok_or_else(|| {
        GqlError::new(format!("{name} must be an object")).extend_with(move |_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
            ext.set("operationIndex", op_index as i32);
        })
    })
}

fn required_str(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    op_index: usize,
) -> Result<String, GqlError> {
    obj.get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            GqlError::new(format!("{field} must be a string")).extend_with(move |_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
                ext.set("operationIndex", op_index as i32);
            })
        })
}

fn required_u64(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    op_index: usize,
) -> Result<u64, GqlError> {
    obj.get(field).and_then(Value::as_u64).ok_or_else(|| {
        GqlError::new(format!("{field} must be an unsigned integer")).extend_with(
            move |_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
                ext.set("operationIndex", op_index as i32);
            },
        )
    })
}

fn input_object<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a serde_json::Map<String, Value>, GqlError> {
    value.as_object().ok_or_else(|| {
        GqlError::new(format!("{name} must be an object")).extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })
    })
}

fn input_string(obj: &serde_json::Map<String, Value>, field: &str) -> Result<String, GqlError> {
    obj.get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            GqlError::new(format!("{field} must be a string")).extend_with(|_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
            })
        })
}

fn input_bool(obj: &serde_json::Map<String, Value>, field: &str, default: bool) -> bool {
    obj.get(field).and_then(Value::as_bool).unwrap_or(default)
}

fn get_schema_field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    snake_case: &str,
    camel_case: &str,
) -> Option<&'a Value> {
    obj.get(camel_case).or_else(|| obj.get(snake_case))
}

fn optional_schema_field<T: DeserializeOwned>(
    obj: &serde_json::Map<String, Value>,
    snake_case: &str,
    camel_case: &str,
) -> Result<Option<T>, GqlError> {
    match get_schema_field(obj, snake_case, camel_case) {
        Some(value) if !value.is_null() => {
            serde_json::from_value(value.clone())
                .map(Some)
                .map_err(|e| {
                    GqlError::new(format!("invalid {camel_case}: {e}")).extend_with(|_err, ext| {
                        ext.set("code", "INVALID_ARGUMENT");
                    })
                })
        }
        _ => Ok(None),
    }
}

fn collection_schema_from_json(
    collection: &CollectionId,
    value: &Value,
) -> Result<CollectionSchema, GqlError> {
    let obj = input_object(value, "schema")?;
    if let Some(schema_collection) =
        get_schema_field(obj, "collection", "collection").and_then(Value::as_str)
    {
        if schema_collection != collection.as_str() {
            return Err(GqlError::new(format!(
                "schema.collection '{schema_collection}' does not match collection name '{collection}'"
            ))
            .extend_with(|_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
            }));
        }
    }

    let version = get_schema_field(obj, "version", "version")
        .and_then(Value::as_u64)
        .map(u32::try_from)
        .transpose()
        .map_err(|_| {
            GqlError::new("version must fit in u32").extend_with(|_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
            })
        })?
        .unwrap_or(1);

    Ok(CollectionSchema {
        collection: collection.clone(),
        description: optional_schema_field(obj, "description", "description")?,
        version,
        entity_schema: get_schema_field(obj, "entity_schema", "entitySchema").cloned(),
        link_types: optional_schema_field(obj, "link_types", "linkTypes")?.unwrap_or_default(),
        gates: optional_schema_field(obj, "gates", "gates")?.unwrap_or_default(),
        validation_rules: optional_schema_field(obj, "validation_rules", "validationRules")?
            .unwrap_or_default(),
        indexes: optional_schema_field(obj, "indexes", "indexes")?.unwrap_or_default(),
        compound_indexes: optional_schema_field(obj, "compound_indexes", "compoundIndexes")?
            .unwrap_or_default(),
        lifecycles: optional_schema_field(obj, "lifecycles", "lifecycles")?.unwrap_or_default(),
    })
}

fn put_schema_payload_value(resp: axon_api::response::PutSchemaResponse) -> Value {
    json!({
        "schema": resp.schema,
        "compatibility": resp.compatibility,
        "diff": resp.diff,
        "dryRun": resp.dry_run,
    })
}

fn op_error(err: GqlError, op_index: usize) -> GqlError {
    err.extend_with(move |_err, ext| {
        ext.set("operationIndex", op_index as i32);
    })
}

fn transaction_payload_value(tx_id: &str, written: &[Entity], replay_hit: bool) -> Value {
    let results: Vec<Value> = written
        .iter()
        .enumerate()
        .map(|(index, entity)| {
            let is_link = entity.collection == Link::links_collection();
            json!({
                "index": index,
                "success": true,
                "collection": entity.collection.to_string(),
                "id": entity.id.to_string(),
                "entity": if is_link { Value::Null } else { entity_to_generic_json(entity) },
                "link": if is_link { entity.data.clone() } else { Value::Null },
            })
        })
        .collect();

    json!({
        "transactionId": tx_id,
        "replayHit": replay_hit,
        "results": results,
    })
}

async fn commit_transaction_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    use axon_api::transaction::Transaction;

    let input = ctx.args.try_get("input")?.as_value();
    let input_json = gql_input_to_json(input)?;
    let input_obj = input_json
        .as_object()
        .ok_or_else(|| GqlError::new("input must be an object"))?;
    let operations = input_obj
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| GqlError::new("operations must be a list"))?;

    if operations.len() > 100 {
        return Err(axon_error_to_gql(AxonError::InvalidArgument(
            "transaction exceeds maximum of 100 operations".into(),
        )));
    }

    let idempotency_key = input_obj
        .get("idempotencyKey")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let scope = graphql_idempotency_scope(&ctx);
    if let Some(ref key) = idempotency_key {
        validate_graphql_idempotency_key(key)?;
        if let Some(mut cached) = idempotency_cached(&scope, key) {
            if let Some(obj) = cached.as_object_mut() {
                obj.insert("replayHit".into(), Value::Bool(true));
            }
            return Ok(Some(json_to_field_value(cached)));
        }
    }

    let mut tx = Transaction::new();

    for (index, op) in operations.iter().enumerate() {
        let obj = required_object(op, "operation", index)?;
        let variants: Vec<(&str, &Value)> = obj
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(key, value)| (key.as_str(), value))
            .collect();
        if variants.len() != 1 {
            return Err(
                GqlError::new("operation must set exactly one variant").extend_with(
                    move |_err, ext| {
                        ext.set("code", "INVALID_ARGUMENT");
                        ext.set("operationIndex", index as i32);
                    },
                ),
            );
        }

        let (variant, payload) = variants[0];
        let payload = required_object(payload, variant, index)?;
        let stage_result = match variant {
            "createEntity" => {
                let collection = required_str(payload, "collection", index)?;
                let id = required_str(payload, "id", index)?;
                let data = payload.get("data").cloned().unwrap_or(Value::Null);
                tx.create(Entity::new(
                    CollectionId::new(collection),
                    EntityId::new(id),
                    data,
                ))
            }
            "updateEntity" => {
                let collection = required_str(payload, "collection", index)?;
                let id = required_str(payload, "id", index)?;
                let expected_version = required_u64(payload, "expectedVersion", index)?;
                let data = payload.get("data").cloned().unwrap_or(Value::Null);
                let guard = handler.lock().await;
                let data_before = guard
                    .get_entity(GetEntityRequest {
                        collection: CollectionId::new(&collection),
                        id: EntityId::new(&id),
                    })
                    .ok()
                    .map(|resp| resp.entity.data);
                drop(guard);
                tx.update(
                    Entity::new(CollectionId::new(collection), EntityId::new(id), data),
                    expected_version,
                    data_before,
                )
            }
            "patchEntity" => {
                let collection = required_str(payload, "collection", index)?;
                let id = required_str(payload, "id", index)?;
                let expected_version = required_u64(payload, "expectedVersion", index)?;
                let patch = payload.get("patch").cloned().unwrap_or(Value::Null);
                let guard = handler.lock().await;
                let existing = match guard.get_entity(GetEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                }) {
                    Ok(resp) => resp.entity,
                    Err(err) => return Err(op_error(axon_error_to_gql(err), index)),
                };
                drop(guard);
                let mut merged = existing.data.clone();
                json_merge_patch(&mut merged, &patch);
                tx.update(
                    Entity::new(CollectionId::new(collection), EntityId::new(id), merged),
                    expected_version,
                    Some(existing.data),
                )
            }
            "deleteEntity" => {
                let collection = required_str(payload, "collection", index)?;
                let id = required_str(payload, "id", index)?;
                let expected_version = required_u64(payload, "expectedVersion", index)?;
                let guard = handler.lock().await;
                let data_before = guard
                    .get_entity(GetEntityRequest {
                        collection: CollectionId::new(&collection),
                        id: EntityId::new(&id),
                    })
                    .ok()
                    .map(|resp| resp.entity.data);
                drop(guard);
                tx.delete(
                    CollectionId::new(collection),
                    EntityId::new(id),
                    expected_version,
                    data_before,
                )
            }
            "createLink" => {
                let source_collection = required_str(payload, "sourceCollection", index)?;
                let source_id = required_str(payload, "sourceId", index)?;
                let target_collection = required_str(payload, "targetCollection", index)?;
                let target_id = required_str(payload, "targetId", index)?;
                let link_type = required_str(payload, "linkType", index)?;
                let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
                tx.create_link(Link {
                    source_collection: CollectionId::new(source_collection),
                    source_id: EntityId::new(source_id),
                    target_collection: CollectionId::new(target_collection),
                    target_id: EntityId::new(target_id),
                    link_type,
                    metadata,
                })
            }
            "deleteLink" => {
                let source_collection = required_str(payload, "sourceCollection", index)?;
                let source_id = required_str(payload, "sourceId", index)?;
                let target_collection = required_str(payload, "targetCollection", index)?;
                let target_id = required_str(payload, "targetId", index)?;
                let link_type = required_str(payload, "linkType", index)?;
                tx.delete_link(Link {
                    source_collection: CollectionId::new(source_collection),
                    source_id: EntityId::new(source_id),
                    target_collection: CollectionId::new(target_collection),
                    target_id: EntityId::new(target_id),
                    link_type,
                    metadata: Value::Null,
                })
            }
            other => {
                return Err(
                    GqlError::new(format!("unsupported transaction operation '{other}'"))
                        .extend_with(move |_err, ext| {
                            ext.set("code", "INVALID_ARGUMENT");
                            ext.set("operationIndex", index as i32);
                        }),
                );
            }
        };

        if let Err(err) = stage_result {
            return Err(op_error(axon_error_to_gql(err), index));
        }
    }

    let tx_id = tx.id.clone();
    let mut guard = handler.lock().await;
    let written = guard
        .commit_transaction_with_caller(tx, &caller, None)
        .map_err(axon_error_to_gql)?;
    drop(guard);

    let payload = transaction_payload_value(&tx_id, &written, false);
    if let Some(ref key) = idempotency_key {
        idempotency_store(&scope, key, payload.clone());
    }
    Ok(Some(json_to_field_value(payload)))
}

async fn create_collection_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Admin).map_err(axon_error_to_gql)?;
    let input_json = gql_input_to_json(ctx.args.try_get("input")?.as_value())?;
    let input = input_object(&input_json, "input")?;
    let name = input_string(input, "name")?;
    let schema_value = input.get("schema").ok_or_else(|| {
        GqlError::new("schema is required").extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })
    })?;
    let collection = CollectionId::new(name);
    let schema = collection_schema_from_json(&collection, schema_value)?;

    let mut guard = handler.lock().await;
    guard
        .create_collection(CreateCollectionRequest {
            name: collection.clone(),
            schema,
            actor: Some(caller.actor),
        })
        .map_err(axon_error_to_gql)?;
    let description = guard
        .describe_collection(DescribeCollectionRequest { name: collection })
        .map_err(axon_error_to_gql)?;

    Ok(Some(json_to_field_value(described_collection_json(
        &description,
    ))))
}

async fn drop_collection_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Admin).map_err(axon_error_to_gql)?;
    let input_json = gql_input_to_json(ctx.args.try_get("input")?.as_value())?;
    let input = input_object(&input_json, "input")?;
    let name = input_string(input, "name")?;
    let confirm = input_bool(input, "confirm", false);
    let resp = handler
        .lock()
        .await
        .drop_collection(DropCollectionRequest {
            name: CollectionId::new(name),
            actor: Some(caller.actor),
            confirm,
        })
        .map_err(axon_error_to_gql)?;

    Ok(Some(json_to_field_value(json!({
        "name": resp.name,
        "entitiesRemoved": resp.entities_removed,
    }))))
}

async fn put_schema_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Admin).map_err(axon_error_to_gql)?;
    let input_json = gql_input_to_json(ctx.args.try_get("input")?.as_value())?;
    let input = input_object(&input_json, "input")?;
    let collection = CollectionId::new(input_string(input, "collection")?);
    let schema_value = input.get("schema").ok_or_else(|| {
        GqlError::new("schema is required").extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })
    })?;
    let schema = collection_schema_from_json(&collection, schema_value)?;
    let force = input_bool(input, "force", false);
    let dry_run = input_bool(input, "dryRun", false);

    let resp = handler
        .lock()
        .await
        .handle_put_schema(PutSchemaRequest {
            schema,
            actor: Some(caller.actor),
            force,
            dry_run,
        })
        .map_err(axon_error_to_gql)?;

    Ok(Some(json_to_field_value(put_schema_payload_value(resp))))
}

// ── Schema builders ─────────────────────────────────────────────────────────

/// Build a dynamic GraphQL schema from the given collection schemas, wired
/// to a live `AxonHandler` for real CRUD operations.
///
/// Each collection produces:
/// - A query field `<collection>(id: ID!): <CollectionType>`
/// - A query field `<collection>s(limit: Int, afterId: ID): [<CollectionType>]`
/// - A mutation field `create<Collection>(id: ID!, input: Create<Collection>Input): Create<Collection>Payload!`
/// - A mutation field `update<Collection>(id: ID!, version: Int!, input: Update<Collection>Input): Update<Collection>Payload!`
/// - A mutation field `patch<Collection>(id: ID!, version: Int!, patch: JSON): Patch<Collection>Payload!`
/// - A mutation field `delete<Collection>(id: ID!): Delete<Collection>Payload!`
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
    let mut input_objects = Vec::new();
    let mut enum_objects = Vec::new();
    let schemas_by_collection: HashMap<String, CollectionSchema> = collections
        .iter()
        .map(|schema| (schema.collection.to_string(), schema.clone()))
        .collect();
    let mut incoming_links: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut relationship_type_names = HashSet::new();

    for source_schema in collections {
        let source_collection = source_schema.collection.to_string();
        for (link_type, link_def) in &source_schema.link_types {
            if schemas_by_collection.contains_key(&link_def.target_collection) {
                incoming_links
                    .entry(link_def.target_collection.clone())
                    .or_default()
                    .push((source_collection.clone(), link_type.clone()));
            }
        }
    }
    for links in incoming_links.values_mut() {
        links.sort();
        links.dedup();
    }

    query = add_handler_root_query_fields(query, Arc::clone(&handler));

    for schema in collections {
        let collection_name = schema.collection.as_str();
        let type_name = pascal_case(collection_name);
        let edge_type_name = format!("{type_name}Edge");
        let connection_type_name = format!("{type_name}Connection");
        let filter_input_name = format!("{type_name}Filter");
        let sort_field_enum_name = format!("{type_name}SortField");
        let sort_input_name = format!("{type_name}Sort");
        let aggregate_input_name = format!("{type_name}Aggregation");
        let aggregate_group_name = format!("{type_name}AggregateGroup");
        let aggregate_result_name = format!("{type_name}Aggregate");
        let create_input_name = format!("Create{type_name}Input");
        let update_input_name = format!("Update{type_name}Input");
        let patch_input_name = format!("Patch{type_name}Input");
        let delete_input_name = format!("Delete{type_name}Input");
        let create_payload_name = format!("Create{type_name}Payload");
        let update_payload_name = format!("Update{type_name}Payload");
        let patch_payload_name = format!("Patch{type_name}Payload");
        let delete_payload_name = format!("Delete{type_name}Payload");
        let get_field_name = collection_field_name(collection_name);
        let list_field_name = collection_list_field_name(collection_name);
        let fields = extract_fields(schema);
        let data_fields: Vec<(String, String, bool)> = fields
            .iter()
            .filter(|(field_name, _, _)| !is_system_entity_field(field_name))
            .cloned()
            .collect();
        input_objects.push(typed_filter_input_object(&filter_input_name, &fields));
        enum_objects.push(typed_sort_field_enum(&sort_field_enum_name, &data_fields));
        input_objects.push(typed_sort_input_object(
            &sort_input_name,
            &sort_field_enum_name,
        ));
        input_objects.push(aggregate_input_object(
            &aggregate_input_name,
            &sort_field_enum_name,
        ));
        input_objects.push(typed_entity_input_object(
            &create_input_name,
            &data_fields,
            true,
        ));
        input_objects.push(typed_entity_input_object(
            &update_input_name,
            &data_fields,
            false,
        ));
        input_objects.push(patch_entity_input_object(&patch_input_name));
        input_objects.push(delete_entity_input_object(&delete_input_name));

        // ── Build the GraphQL object type ────────────────────────────────
        let mut obj = Object::new(&type_name);
        let mut object_field_names: HashSet<String> = HashSet::new();
        for (field_name, gql_type, _required) in &fields {
            object_field_names.insert(field_name.clone());
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
        for (link_type, link_def) in &schema.link_types {
            let Some(target_schema) = schemas_by_collection.get(&link_def.target_collection) else {
                continue;
            };
            let relationship_field_name = collection_field_name(link_type);
            if !object_field_names.insert(relationship_field_name.clone()) {
                continue;
            }
            let target_type_name = pascal_case(target_schema.collection.as_str());
            let type_stem = format!("{type_name}{}Relationship", pascal_case(link_type));
            let relationship_edge_name = format!("{type_stem}Edge");
            let relationship_connection_name = format!("{type_stem}Connection");
            if relationship_type_names.insert(relationship_edge_name.clone()) {
                type_objects.push(relationship_edge_object(
                    &relationship_edge_name,
                    &target_type_name,
                ));
            }
            if relationship_type_names.insert(relationship_connection_name.clone()) {
                type_objects.push(typed_connection_object(
                    &relationship_connection_name,
                    &relationship_edge_name,
                ));
            }
            obj = obj.field(relationship_field(
                &relationship_field_name,
                &relationship_connection_name,
                &format!("{target_type_name}Filter"),
                Arc::clone(&handler),
                RelationshipFieldSpec {
                    collection: collection_name.to_owned(),
                    link_type: link_type.clone(),
                    direction: TraverseDirection::Forward,
                    expected_source_collection: collection_name.to_owned(),
                    expected_target_collection: target_schema.collection.to_string(),
                    node_schema: target_schema.clone(),
                },
            ));
        }
        if let Some(links) = incoming_links.get(collection_name) {
            for (source_collection, link_type) in links {
                let Some(source_schema) = schemas_by_collection.get(source_collection) else {
                    continue;
                };
                let relationship_field_name =
                    format!("{}Inbound", collection_field_name(link_type));
                if !object_field_names.insert(relationship_field_name.clone()) {
                    continue;
                }
                let source_type_name = pascal_case(source_schema.collection.as_str());
                let type_stem = format!("{type_name}{}InboundRelationship", pascal_case(link_type));
                let relationship_edge_name = format!("{type_stem}Edge");
                let relationship_connection_name = format!("{type_stem}Connection");
                if relationship_type_names.insert(relationship_edge_name.clone()) {
                    type_objects.push(relationship_edge_object(
                        &relationship_edge_name,
                        &source_type_name,
                    ));
                }
                if relationship_type_names.insert(relationship_connection_name.clone()) {
                    type_objects.push(typed_connection_object(
                        &relationship_connection_name,
                        &relationship_edge_name,
                    ));
                }
                obj = obj.field(relationship_field(
                    &relationship_field_name,
                    &relationship_connection_name,
                    &format!("{source_type_name}Filter"),
                    Arc::clone(&handler),
                    RelationshipFieldSpec {
                        collection: collection_name.to_owned(),
                        link_type: link_type.clone(),
                        direction: TraverseDirection::Reverse,
                        expected_source_collection: source_schema.collection.to_string(),
                        expected_target_collection: collection_name.to_owned(),
                        node_schema: source_schema.clone(),
                    },
                ));
            }
        }
        obj = add_entity_lifecycle_fields(obj);
        type_objects.push(obj);
        type_objects.push(typed_edge_object(&edge_type_name, &type_name));
        type_objects.push(typed_connection_object(
            &connection_type_name,
            &edge_type_name,
        ));
        type_objects.push(aggregate_group_object(&aggregate_group_name));
        type_objects.push(aggregate_result_object(
            &aggregate_result_name,
            &aggregate_group_name,
        ));
        type_objects.push(typed_entity_payload_object(
            &create_payload_name,
            &type_name,
            &data_fields,
        ));
        type_objects.push(typed_entity_payload_object(
            &update_payload_name,
            &type_name,
            &data_fields,
        ));
        type_objects.push(typed_entity_payload_object(
            &patch_payload_name,
            &type_name,
            &data_fields,
        ));
        type_objects.push(delete_entity_payload_object(
            &delete_payload_name,
            &type_name,
        ));

        // ── Query: get by ID ─────────────────────────────────────────────
        let col_id = CollectionId::new(collection_name);
        let handler_get = Arc::clone(&handler);
        let col_for_get = col_id.clone();
        let schema_for_get = schema.clone();
        let get_field = Field::new(&get_field_name, TypeRef::named(&type_name), move |ctx| {
            let handler = Arc::clone(&handler_get);
            let col = col_for_get.clone();
            let schema = schema_for_get.clone();
            FieldFuture::new(async move {
                let id_str = ctx.args.try_get("id")?.string()?;

                let guard = handler.lock().await;
                match guard.get_entity(GetEntityRequest {
                    collection: col.clone(),
                    id: EntityId::new(id_str),
                }) {
                    Ok(resp) => Ok(Some(entity_to_field_value_with_schema(
                        &resp.entity,
                        Some(&schema),
                    ))),
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
        let schema_for_list = schema.clone();
        let type_name_list = type_name.clone();
        let list_field = Field::new(
            &list_field_name,
            TypeRef::named_list(&type_name_list),
            move |ctx| {
                let handler = Arc::clone(&handler_list);
                let col = col_for_list.clone();
                let schema = schema_for_list.clone();
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
                                .map(|e| entity_to_field_value_with_schema(e, Some(&schema)))
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
        .argument(InputValue::new(
            "filter",
            TypeRef::named(&filter_input_name),
        ))
        .argument(InputValue::new(
            "sort",
            TypeRef::named_nn_list(&sort_input_name),
        ));
        query = query.field(list_field);

        let list_connection_field_name = format!("{list_field_name}Connection");
        let handler_list_connection = Arc::clone(&handler);
        let col_for_list_connection = col_id.clone();
        let schema_for_list_connection = schema.clone();
        let connection_type_name_ref = connection_type_name.clone();
        let list_connection_field = Field::new(
            &list_connection_field_name,
            TypeRef::named_nn(&connection_type_name_ref),
            move |ctx| {
                let handler = Arc::clone(&handler_list_connection);
                let col = col_for_list_connection.clone();
                let schema = schema_for_list_connection.clone();
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
                            Some(&schema),
                        ))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID)))
        .argument(InputValue::new(
            "filter",
            TypeRef::named(&filter_input_name),
        ))
        .argument(InputValue::new(
            "sort",
            TypeRef::named_nn_list(&sort_input_name),
        ));
        query = query.field(list_connection_field);

        // ── Query: aggregate ─────────────────────────────────────────────
        let aggregate_field_name = format!("{}Aggregate", get_field_name);
        let handler_aggregate = Arc::clone(&handler);
        let col_for_aggregate = col_id.clone();
        let aggregate_result_name_ref = aggregate_result_name.clone();
        let filter_input_name_ref = filter_input_name.clone();
        let sort_field_enum_name_ref = sort_field_enum_name.clone();
        let aggregate_input_name_ref = aggregate_input_name.clone();
        let aggregate_field = Field::new(
            &aggregate_field_name,
            TypeRef::named_nn(&aggregate_result_name_ref),
            move |ctx| {
                let handler = Arc::clone(&handler_aggregate);
                let col = col_for_aggregate.clone();
                FieldFuture::new(async move {
                    let filter = match ctx.args.try_get("filter") {
                        Ok(value) => Some(parse_graphql_filter_arg(value.as_value())?),
                        Err(_) => None,
                    };
                    let group_by = match ctx.args.try_get("groupBy") {
                        Ok(value) => parse_graphql_group_by_arg(value.as_value())?,
                        Err(_) => Vec::new(),
                    };
                    let aggregations = parse_graphql_aggregations_arg(
                        ctx.args.try_get("aggregations")?.as_value(),
                    )?;

                    let guard = handler.lock().await;
                    let response = guard.query_entities(QueryEntitiesRequest {
                        collection: col.clone(),
                        filter,
                        sort: Vec::new(),
                        limit: None,
                        after_id: None,
                        count_only: false,
                    })?;
                    let payload = graphql_aggregate_response(
                        &response.entities,
                        response.total_count,
                        &group_by,
                        &aggregations,
                    )?;
                    Ok(Some(json_to_field_value(payload)))
                })
            },
        )
        .argument(InputValue::new(
            "filter",
            TypeRef::named(&filter_input_name_ref),
        ))
        .argument(InputValue::new(
            "groupBy",
            TypeRef::named_nn_list(&sort_field_enum_name_ref),
        ))
        .argument(InputValue::new(
            "aggregations",
            TypeRef::named_nn_list(&aggregate_input_name_ref),
        ));
        query = query.field(aggregate_field);

        // ── Mutation: create ─────────────────────────────────────────────
        let create_field_name = format!("create{type_name}");
        let handler_create = Arc::clone(&handler);
        let col_for_create = col_id.clone();
        let schema_for_create = schema.clone();
        let create_payload_name_ref = create_payload_name.clone();
        let create_input_name_ref = create_input_name.clone();
        let create_field = Field::new(
            &create_field_name,
            TypeRef::named_nn(&create_payload_name_ref),
            move |ctx| {
                let handler = Arc::clone(&handler_create);
                let col = col_for_create.clone();
                let schema = schema_for_create.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;

                    let data = mutation_data_arg(&ctx, "input", "legacyInput")?;

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
                        Ok(resp) => Ok(Some(json_to_field_value(typed_entity_payload_value(
                            &resp.entity,
                            Some(&schema),
                        )))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "input",
            TypeRef::named(&create_input_name_ref),
        ))
        .argument(InputValue::new("legacyInput", TypeRef::named("JSON")));
        mutation = mutation.field(create_field);

        // ── Mutation: update ─────────────────────────────────────────────
        let update_field_name = format!("update{type_name}");
        let handler_update = Arc::clone(&handler);
        let col_for_update = col_id.clone();
        let schema_for_update = schema.clone();
        let update_payload_name_ref = update_payload_name.clone();
        let update_input_name_ref = update_input_name.clone();
        let update_field = Field::new(
            &update_field_name,
            TypeRef::named_nn(&update_payload_name_ref),
            move |ctx| {
                let handler = Arc::clone(&handler_update);
                let col = col_for_update.clone();
                let schema = schema_for_update.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;
                    let version = ctx.args.try_get("version")?.i64()? as u64;

                    let data = mutation_data_arg(&ctx, "input", "legacyInput")?;

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
                        Ok(resp) => Ok(Some(json_to_field_value(typed_entity_payload_value(
                            &resp.entity,
                            Some(&schema),
                        )))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new("version", TypeRef::named_nn(TypeRef::INT)))
        .argument(InputValue::new(
            "input",
            TypeRef::named(&update_input_name_ref),
        ))
        .argument(InputValue::new("legacyInput", TypeRef::named("JSON")));
        mutation = mutation.field(update_field);

        // ── Mutation: patch ──────────────────────────────────────────────
        let patch_field_name = format!("patch{type_name}");
        let handler_patch = Arc::clone(&handler);
        let col_for_patch = col_id.clone();
        let schema_for_patch = schema.clone();
        let patch_payload_name_ref = patch_payload_name.clone();
        let patch_input_name_ref = patch_input_name.clone();
        let patch_field = Field::new(
            &patch_field_name,
            TypeRef::named_nn(&patch_payload_name_ref),
            move |ctx| {
                let handler = Arc::clone(&handler_patch);
                let col = col_for_patch.clone();
                let schema = schema_for_patch.clone();
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let id_str = ctx.args.try_get("id")?.string()?;
                    let version = ctx.args.try_get("version")?.i64()? as u64;

                    let patch = if let Ok(input) = ctx.args.try_get("typedInput") {
                        let input = gql_input_to_json(input.as_value())?;
                        input
                            .get("patch")
                            .cloned()
                            .ok_or_else(|| GqlError::new("typedInput.patch is required"))?
                    } else {
                        gql_json_or_legacy_string_arg(
                            ctx.args.try_get("patch")?.as_value(),
                            "patch",
                        )?
                    };

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
                        Ok(resp) => Ok(Some(json_to_field_value(typed_entity_payload_value(
                            &resp.entity,
                            Some(&schema),
                        )))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new("version", TypeRef::named_nn(TypeRef::INT)))
        .argument(InputValue::new("patch", TypeRef::named("JSON")))
        .argument(InputValue::new(
            "typedInput",
            TypeRef::named(&patch_input_name_ref),
        ));
        mutation = mutation.field(patch_field);

        // ── Mutation: delete ─────────────────────────────────────────────
        let delete_field_name = format!("delete{type_name}");
        let handler_delete = Arc::clone(&handler);
        let col_for_delete = col_id.clone();
        let delete_payload_name_ref = delete_payload_name.clone();
        let delete_input_name_ref = delete_input_name.clone();
        let delete_field = Field::new(
            &delete_field_name,
            TypeRef::named_nn(&delete_payload_name_ref),
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
                        Ok(_) => Ok(Some(json_to_field_value(json!({
                            "deleted": true,
                            "id": id_str,
                            "entity": Value::Null,
                        })))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .argument(InputValue::new(
            "typedInput",
            TypeRef::named(&delete_input_name_ref),
        ));
        mutation = mutation.field(delete_field);

        // ── Mutation: transition<Collection>Lifecycle ────────────────────
        let transition_field_name = format!("transition{type_name}Lifecycle");
        let handler_transition = Arc::clone(&handler);
        let col_for_transition = col_id.clone();
        let schema_for_transition = schema.clone();
        let type_name_transition = type_name.clone();
        let transition_field = Field::new(
            &transition_field_name,
            TypeRef::named(&type_name_transition),
            move |ctx| {
                let handler = Arc::clone(&handler_transition);
                let col = col_for_transition.clone();
                let schema = schema_for_transition.clone();
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
                        Ok(resp) => Ok(Some(entity_to_field_value_with_schema(
                            &resp.entity,
                            Some(&schema),
                        ))),
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

    // ── Collection and schema administration mutations ──────────────────────
    {
        let handler_create_collection = Arc::clone(&handler);
        let create_collection_field = Field::new(
            "createCollection",
            TypeRef::named_nn(COLLECTION_META_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_create_collection);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(
                    async move { create_collection_resolver(ctx, handler, caller).await },
                )
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(CREATE_COLLECTION_INPUT),
        ));
        mutation = mutation.field(create_collection_field);

        let handler_drop_collection = Arc::clone(&handler);
        let drop_collection_field = Field::new(
            "dropCollection",
            TypeRef::named_nn(DROP_COLLECTION_PAYLOAD),
            move |ctx| {
                let handler = Arc::clone(&handler_drop_collection);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(
                    async move { drop_collection_resolver(ctx, handler, caller).await },
                )
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(DROP_COLLECTION_INPUT),
        ));
        mutation = mutation.field(drop_collection_field);

        let handler_put_schema = Arc::clone(&handler);
        let put_schema_field = Field::new(
            "putSchema",
            TypeRef::named_nn(PUT_SCHEMA_PAYLOAD),
            move |ctx| {
                let handler = Arc::clone(&handler_put_schema);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move { put_schema_resolver(ctx, handler, caller).await })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(PUT_SCHEMA_INPUT),
        ));
        mutation = mutation.field(put_schema_field);
    }

    // ── Global transaction mutation ──────────────────────────────────────────
    {
        let handler_commit_transaction = Arc::clone(&handler);
        let commit_transaction_field = Field::new(
            "commitTransaction",
            TypeRef::named_nn(COMMIT_TRANSACTION_PAYLOAD),
            move |ctx| {
                let handler = Arc::clone(&handler_commit_transaction);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(
                    async move { commit_transaction_resolver(ctx, handler, caller).await },
                )
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(COMMIT_TRANSACTION_INPUT),
        ));
        mutation = mutation.field(commit_transaction_field);
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
    .register(create_collection_input_object())
    .register(drop_collection_input_object())
    .register(put_schema_input_object())
    .register(commit_transaction_input_object())
    .register(transaction_operation_input_object())
    .register(create_entity_transaction_input_object())
    .register(update_entity_transaction_input_object())
    .register(patch_entity_transaction_input_object())
    .register(delete_entity_transaction_input_object())
    .register(create_link_transaction_input_object())
    .register(delete_link_transaction_input_object())
    .register(query)
    .register(mutation);

    for input in scalar_filter_input_objects() {
        schema_builder = schema_builder.register(input);
    }
    schema_builder = schema_builder
        .register(aggregate_function_enum())
        .register(aggregate_value_object());
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
    for input in input_objects {
        schema_builder = schema_builder.register(input);
    }
    for enum_obj in enum_objects {
        schema_builder = schema_builder.register(enum_obj);
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
        obj = add_entity_lifecycle_fields(obj);
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
                        Ok(Some(entity_connection_value(
                            &[],
                            0,
                            None,
                            false,
                            false,
                            None,
                        )))
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

    mutation = mutation.field(
        Field::new(
            "createCollection",
            TypeRef::named_nn(COLLECTION_META_TYPE),
            |_ctx| FieldFuture::new(async move { Ok(Some(FieldValue::NULL)) }),
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(CREATE_COLLECTION_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new(
            "dropCollection",
            TypeRef::named_nn(DROP_COLLECTION_PAYLOAD),
            |_ctx| {
                FieldFuture::new(async move {
                    Ok(Some(json_to_field_value(json!({
                        "name": "",
                        "entitiesRemoved": 0,
                    }))))
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(DROP_COLLECTION_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new("putSchema", TypeRef::named_nn(PUT_SCHEMA_PAYLOAD), |_ctx| {
            FieldFuture::new(async move {
                Ok(Some(json_to_field_value(json!({
                    "schema": {},
                    "compatibility": Value::Null,
                    "diff": Value::Null,
                    "dryRun": false,
                }))))
            })
        })
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(PUT_SCHEMA_INPUT),
        )),
    );

    mutation = mutation.field(
        Field::new(
            "commitTransaction",
            TypeRef::named_nn(COMMIT_TRANSACTION_PAYLOAD),
            |_ctx| {
                FieldFuture::new(async move {
                    Ok(Some(json_to_field_value(transaction_payload_value(
                        "tx-stub",
                        &[],
                        false,
                    ))))
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(COMMIT_TRANSACTION_INPUT),
        )),
    );

    let mut schema_builder = Schema::build(query.type_name(), Some(mutation.type_name()), None)
        .limit_depth(max_graphql_depth())
        .limit_complexity(max_graphql_complexity())
        .register(Scalar::new("JSON"))
        .register(filter_input_object())
        .register(sort_input_object())
        .register(create_collection_input_object())
        .register(drop_collection_input_object())
        .register(put_schema_input_object())
        .register(commit_transaction_input_object())
        .register(transaction_operation_input_object())
        .register(create_entity_transaction_input_object())
        .register(update_entity_transaction_input_object())
        .register(patch_entity_transaction_input_object())
        .register(delete_entity_transaction_input_object())
        .register(create_link_transaction_input_object())
        .register(delete_link_transaction_input_object())
        .register(query)
        .register(mutation);

    for input in scalar_filter_input_objects() {
        schema_builder = schema_builder.register(input);
    }
    schema_builder = schema_builder
        .register(aggregate_function_enum())
        .register(aggregate_value_object());
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
                r#"mutation { createTasks(id: "t1", input: { title: "New" }) { id version title } }"#,
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
                r#"mutation { updateTasks(id: "t1", version: 1, input: { title: "Updated" }) { id version title } }"#,
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
                r#"mutation { updateTasks(id: "t1", version: 1, input: { title: "Stale" }) { id version } }"#,
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
            .execute(r#"mutation { deleteTasks(id: "t1") { deleted } }"#)
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        assert_eq!(data["deleteTasks"]["deleted"], true);

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
