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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
use axon_api::intent::{
    canonicalize_intent_operation, ApprovalState, CanonicalOperationMetadata,
    MutationApprovalRoute, MutationIntent, MutationIntentCommitValidationAuditRequest,
    MutationIntentCommitValidationContext, MutationIntentCommitValidationError,
    MutationIntentDecision, MutationIntentLifecycleService, MutationIntentReviewMetadata,
    MutationIntentScopeBinding, MutationIntentSubjectBinding, MutationIntentToken,
    MutationIntentTokenLookupError, MutationIntentTokenSigner,
    MutationIntentTransactionCommitRequest, MutationOperationKind, MutationReviewSummary,
    PreImageBinding,
};
use axon_api::request::{
    CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest,
    DeleteCollectionTemplateRequest, DeleteEntityRequest, DeleteLinkRequest,
    DescribeCollectionRequest, DropCollectionRequest, ExplainActorOverride, ExplainPolicyRequest,
    FieldFilter, FilterNode, FilterOp, FindLinkCandidatesRequest, GateFilter,
    GetCollectionTemplateRequest, GetEntityRequest, ListCollectionsRequest, PatchEntityRequest,
    PutCollectionTemplateRequest, PutSchemaRequest, QueryAuditRequest, QueryEntitiesRequest,
    RevertEntityRequest, RollbackEntityRequest, RollbackEntityTarget, SortDirection, SortField,
    TransitionLifecycleRequest, TraverseDirection, TraverseRequest, UpdateEntityRequest,
};
use axon_api::transaction::Transaction;
use axon_api::PolicySubjectSnapshot;
use axon_audit::entry::compute_diff;
use axon_audit::log::AuditLog;
use axon_core::auth::{CallerIdentity, Operation};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, LinkId};
use axon_core::types::{Entity, Link};
use axon_schema::access_control::AccessControlPolicy;
use axon_schema::policy::{compile_policy_catalog, PolicyCompileError, PolicyPlan};
use axon_schema::schema::{CollectionSchema, CollectionView};
use axon_schema::validation::validate;
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
const COLLECTION_TEMPLATE_TYPE: &str = "CollectionTemplate";
const EFFECTIVE_POLICY_TYPE: &str = "EffectiveCollectionPolicy";
const EXPLAIN_POLICY_INPUT: &str = "ExplainPolicyInput";
const POLICY_EXPLANATION_TYPE: &str = "PolicyExplanation";
const POLICY_RULE_MATCH_TYPE: &str = "PolicyRuleMatch";
const POLICY_APPROVAL_ENVELOPE_TYPE: &str = "PolicyApprovalEnvelope";
const RENDERED_ENTITY_TYPE: &str = "RenderedEntity";
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
const PUT_COLLECTION_TEMPLATE_INPUT: &str = "PutCollectionTemplateInput";
const DELETE_COLLECTION_TEMPLATE_PAYLOAD: &str = "DeleteCollectionTemplatePayload";
const REVERT_AUDIT_ENTRY_PAYLOAD: &str = "RevertAuditEntryPayload";
const DROP_COLLECTION_PAYLOAD: &str = "DropCollectionPayload";
const PUT_SCHEMA_PAYLOAD: &str = "PutSchemaPayload";
const ROLLBACK_ENTITY_INPUT: &str = "RollbackEntityInput";
const ROLLBACK_ENTITY_PAYLOAD: &str = "RollbackEntityPayload";
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
const CANONICAL_OPERATION_INPUT: &str = "CanonicalOperationInput";
const CANONICAL_OPERATION_TYPE: &str = "CanonicalOperation";
const MUTATION_PREVIEW_INPUT: &str = "MutationPreviewInput";
const APPROVE_INTENT_INPUT: &str = "ApproveIntentInput";
const REJECT_INTENT_INPUT: &str = "RejectIntentInput";
const COMMIT_INTENT_INPUT: &str = "CommitIntentInput";
const MUTATION_INTENT_FILTER_INPUT: &str = "MutationIntentFilter";
const MUTATION_PREVIEW_RESULT_TYPE: &str = "MutationPreviewResult";
const MUTATION_INTENT_TYPE: &str = "MutationIntent";
const MUTATION_APPROVAL_ROUTE_TYPE: &str = "MutationApprovalRoute";
const MUTATION_INTENT_PRE_IMAGE_TYPE: &str = "MutationIntentPreImage";
const MUTATION_INTENT_STALE_DIMENSION_TYPE: &str = "MutationIntentStaleDimension";
const MUTATION_INTENT_EDGE_TYPE: &str = "MutationIntentEdge";
const MUTATION_INTENT_CONNECTION_TYPE: &str = "MutationIntentConnection";
const COMMIT_INTENT_RESULT_TYPE: &str = "CommitIntentResult";
const DEFAULT_MAX_GRAPHQL_DEPTH: usize = 10;
const DEFAULT_MAX_GRAPHQL_COMPLEXITY: usize = 256;
const MAX_DEPTH_ENV: &str = "AXON_GRAPHQL_MAX_DEPTH";
const MAX_COMPLEXITY_ENV: &str = "AXON_GRAPHQL_MAX_COMPLEXITY";
const IDEMPOTENCY_TTL: Duration = Duration::from_secs(5 * 60);

static GRAPHQL_IDEMPOTENCY_CACHE: OnceLock<StdMutex<HashMap<(String, String), IdempotencyEntry>>> =
    OnceLock::new();
static GRAPHQL_INTENT_COUNTER: AtomicU64 = AtomicU64::new(1);
const GRAPHQL_INTENT_TOKEN_SECRET: &[u8] = b"axon-graphql-mutation-intents-v1";
const INTENT_AUTHORIZATION_FAILED_CODE: &str = "intent_authorization_failed";

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

fn effective_policy_object() -> Object {
    Object::new(EFFECTIVE_POLICY_TYPE)
        .field(json_object_field(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "canRead",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "canCreate",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "canUpdate",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "canDelete",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "redactedFields",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "deniedFields",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "policyVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
}

fn policy_rule_match_object() -> Object {
    Object::new(POLICY_RULE_MATCH_TYPE)
        .field(json_object_field(
            "ruleId",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("name", TypeRef::named(TypeRef::STRING)))
        .field(json_object_field(
            "kind",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "fieldPath",
            TypeRef::named(TypeRef::STRING),
        ))
}

fn policy_approval_envelope_object() -> Object {
    Object::new(POLICY_APPROVAL_ENVELOPE_TYPE)
        .field(json_object_field(
            "policyId",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("name", TypeRef::named(TypeRef::STRING)))
        .field(json_object_field(
            "decision",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("role", TypeRef::named(TypeRef::STRING)))
        .field(json_object_field(
            "reasonRequired",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "deadlineSeconds",
            TypeRef::named(TypeRef::INT),
        ))
        .field(json_object_field(
            "separationOfDuties",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
}

fn policy_explanation_object() -> Object {
    Object::new(POLICY_EXPLANATION_TYPE)
        .field(json_object_field(
            "operation",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "collection",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field("entityId", TypeRef::named(TypeRef::ID)))
        .field(json_object_field(
            "operationIndex",
            TypeRef::named(TypeRef::INT),
        ))
        .field(json_object_field(
            "decision",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "reason",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "policyVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "ruleIds",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "policyIds",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "fieldPaths",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "deniedFields",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "rules",
            TypeRef::named_nn_list_nn(POLICY_RULE_MATCH_TYPE),
        ))
        .field(json_object_field(
            "approval",
            TypeRef::named(POLICY_APPROVAL_ENVELOPE_TYPE),
        ))
        .field(json_object_field(
            "operations",
            TypeRef::named_nn_list_nn(POLICY_EXPLANATION_TYPE),
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
    policy_nullable_fields: &HashSet<String>,
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
    for (field_name, gql_type, required) in fields {
        let type_ref = output_type_ref_for_field(
            gql_type,
            *required,
            policy_nullable_fields.contains(field_name),
        );
        obj = obj.field(json_object_field(field_name, type_ref));
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

fn output_type_ref_for_field(gql_type: &str, required: bool, policy_nullable: bool) -> TypeRef {
    let base = gql_type.trim_end_matches('!');
    if required && !policy_nullable {
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
        .field(InputValue::new(
            "explainInputs",
            TypeRef::named_nn_list(EXPLAIN_POLICY_INPUT),
        ))
}

fn put_collection_template_input_object() -> InputObject {
    InputObject::new(PUT_COLLECTION_TEMPLATE_INPUT)
        .field(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new(
            "template",
            TypeRef::named_nn(TypeRef::STRING),
        ))
}

fn rollback_entity_input_object() -> InputObject {
    InputObject::new(ROLLBACK_ENTITY_INPUT)
        .field(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("id", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "toVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(InputValue::new(
            "expectedVersion",
            TypeRef::named(TypeRef::INT),
        ))
        .field(InputValue::new("dryRun", TypeRef::named(TypeRef::BOOLEAN)))
}

fn explain_policy_input_object() -> InputObject {
    InputObject::new(EXPLAIN_POLICY_INPUT)
        .field(InputValue::new(
            "operation",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new(
            "collection",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(InputValue::new("entityId", TypeRef::named(TypeRef::ID)))
        .field(InputValue::new(
            "expectedVersion",
            TypeRef::named(TypeRef::INT),
        ))
        .field(InputValue::new("data", TypeRef::named("JSON")))
        .field(InputValue::new("patch", TypeRef::named("JSON")))
        .field(InputValue::new(
            "lifecycleName",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(InputValue::new(
            "targetState",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(InputValue::new("toVersion", TypeRef::named(TypeRef::INT)))
        .field(InputValue::new(
            "operations",
            TypeRef::named_nn_list(TRANSACTION_OPERATION_INPUT),
        ))
        // Synthetic actor used by the `putSchema` dry-run fixture path so the
        // admin UI can preview decisions for a different subject. The active
        // `explainPolicy` query ignores this and continues to evaluate as the
        // authenticated caller.
        .field(InputValue::new("actor", TypeRef::named("JSON")))
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

fn canonical_operation_input_object() -> InputObject {
    InputObject::new(CANONICAL_OPERATION_INPUT)
        .field(InputValue::new(
            "operationKind",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new(
            "operationHash",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(InputValue::new("operation", TypeRef::named_nn("JSON")))
}

fn mutation_preview_input_object() -> InputObject {
    InputObject::new(MUTATION_PREVIEW_INPUT)
        .field(InputValue::new(
            "operation",
            TypeRef::named_nn(CANONICAL_OPERATION_INPUT),
        ))
        .field(InputValue::new("subject", TypeRef::named("JSON")))
        .field(InputValue::new(
            "expiresInSeconds",
            TypeRef::named(TypeRef::INT),
        ))
        .field(InputValue::new("reason", TypeRef::named(TypeRef::STRING)))
}

fn approve_intent_input_object() -> InputObject {
    InputObject::new(APPROVE_INTENT_INPUT)
        .field(InputValue::new("intentId", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new("reason", TypeRef::named(TypeRef::STRING)))
}

fn reject_intent_input_object() -> InputObject {
    InputObject::new(REJECT_INTENT_INPUT)
        .field(InputValue::new("intentId", TypeRef::named_nn(TypeRef::ID)))
        .field(InputValue::new(
            "reason",
            TypeRef::named_nn(TypeRef::STRING),
        ))
}

fn commit_intent_input_object() -> InputObject {
    InputObject::new(COMMIT_INTENT_INPUT)
        .field(InputValue::new(
            "intentToken",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(InputValue::new("intentId", TypeRef::named(TypeRef::ID)))
        .field(InputValue::new(
            "operation",
            TypeRef::named(CANONICAL_OPERATION_INPUT),
        ))
}

fn mutation_intent_filter_input_object() -> InputObject {
    InputObject::new(MUTATION_INTENT_FILTER_INPUT)
        .field(InputValue::new("status", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new(
            "statuses",
            TypeRef::named_nn_list(TypeRef::STRING),
        ))
        .field(InputValue::new("decision", TypeRef::named(TypeRef::STRING)))
        .field(InputValue::new(
            "includeExpired",
            TypeRef::named(TypeRef::BOOLEAN),
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
        Ok(value) if value.is_null() => Ok(None),
        Ok(value) => value
            .string()
            .map_err(|_| GqlError::new(format!("{name} must be a stringified unsigned integer")))?
            .parse::<u64>()
            .map(Some)
            .map_err(|e| GqlError::new(format!("invalid {name}: {e}"))),
        Err(_) => Ok(None),
    }
}

fn parse_optional_string_arg(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
    name: &str,
) -> Result<Option<String>, GqlError> {
    match ctx.args.try_get(name) {
        Ok(value) if value.is_null() => Ok(None),
        Ok(value) => value
            .string()
            .map(|value| Some(value.to_owned()))
            .map_err(|_| GqlError::new(format!("{name} must be a string"))),
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
            let caller = caller_from_ctx(&ctx);
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
                let response = guard.traverse_with_caller(
                    TraverseRequest {
                        collection: CollectionId::new(spec.collection.clone()),
                        id: EntityId::new(parent_id),
                        link_type: Some(spec.link_type.clone()),
                        max_depth: Some(1),
                        direction: spec.direction.clone(),
                        hop_filter,
                    },
                    &caller,
                    None,
                );
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

fn collection_template_json(view: &CollectionView, warnings: &[String]) -> Value {
    json!({
        "collection": view.collection.to_string(),
        "template": view.markdown_template.clone(),
        "version": view.version,
        "updatedAtNs": view.updated_at_ns.map(|value| value.to_string()),
        "updatedBy": view.updated_by.clone(),
        "warnings": warnings,
    })
}

fn collection_template_object() -> Object {
    Object::new(COLLECTION_TEMPLATE_TYPE)
        .field(json_object_field(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "template",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "version",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "updatedAtNs",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "updatedBy",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "warnings",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
}

fn rendered_entity_object() -> Object {
    Object::new(RENDERED_ENTITY_TYPE)
        .field(json_object_field("entity", TypeRef::named_nn(ENTITY_TYPE)))
        .field(json_object_field(
            "markdown",
            TypeRef::named_nn(TypeRef::STRING),
        ))
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

fn delete_collection_template_payload_object() -> Object {
    Object::new(DELETE_COLLECTION_TEMPLATE_PAYLOAD)
        .field(json_object_field(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "deleted",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
}

fn revert_audit_entry_payload_object() -> Object {
    Object::new(REVERT_AUDIT_ENTRY_PAYLOAD)
        .field(json_object_field("entity", TypeRef::named_nn(ENTITY_TYPE)))
        .field(json_object_field(
            "auditEntry",
            TypeRef::named_nn(AUDIT_ENTRY_TYPE),
        ))
}

fn put_schema_payload_object() -> Object {
    Object::new(PUT_SCHEMA_PAYLOAD)
        .field(json_object_field("schema", TypeRef::named_nn("JSON")))
        .field(json_object_field("compatibility", TypeRef::named("JSON")))
        .field(json_object_field("diff", TypeRef::named("JSON")))
        .field(json_object_field(
            "policyCompileReport",
            TypeRef::named("JSON"),
        ))
        .field(json_object_field(
            "dryRunExplanations",
            TypeRef::named("JSON"),
        ))
        .field(json_object_field(
            "dryRun",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
}

fn rollback_entity_payload_object() -> Object {
    Object::new(ROLLBACK_ENTITY_PAYLOAD)
        .field(json_object_field(
            "dryRun",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field("current", TypeRef::named(ENTITY_TYPE)))
        .field(json_object_field("target", TypeRef::named_nn(ENTITY_TYPE)))
        .field(json_object_field("diff", TypeRef::named_nn("JSON")))
        .field(json_object_field("entity", TypeRef::named(ENTITY_TYPE)))
        .field(json_object_field(
            "auditEntry",
            TypeRef::named(AUDIT_ENTRY_TYPE),
        ))
}

fn canonical_operation_object() -> Object {
    Object::new(CANONICAL_OPERATION_TYPE)
        .field(json_object_field(
            "operationKind",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "operationHash",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("operation", TypeRef::named("JSON")))
}

fn mutation_approval_route_object() -> Object {
    Object::new(MUTATION_APPROVAL_ROUTE_TYPE)
        .field(json_object_field("role", TypeRef::named(TypeRef::STRING)))
        .field(json_object_field(
            "reasonRequired",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "deadlineSeconds",
            TypeRef::named(TypeRef::INT),
        ))
        .field(json_object_field(
            "separationOfDuties",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
}

fn mutation_intent_pre_image_object() -> Object {
    Object::new(MUTATION_INTENT_PRE_IMAGE_TYPE)
        .field(json_object_field(
            "kind",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("id", TypeRef::named(TypeRef::ID)))
        .field(json_object_field("version", TypeRef::named(TypeRef::INT)))
}

fn mutation_intent_object() -> Object {
    Object::new(MUTATION_INTENT_TYPE)
        .field(json_object_field("id", TypeRef::named_nn(TypeRef::ID)))
        .field(json_object_field(
            "tenantId",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "databaseId",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field("subject", TypeRef::named_nn("JSON")))
        .field(json_object_field(
            "schemaVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "policyVersion",
            TypeRef::named_nn(TypeRef::INT),
        ))
        .field(json_object_field(
            "operation",
            TypeRef::named_nn(CANONICAL_OPERATION_TYPE),
        ))
        .field(json_object_field(
            "operationHash",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "preImages",
            TypeRef::named_nn_list_nn(MUTATION_INTENT_PRE_IMAGE_TYPE),
        ))
        .field(json_object_field(
            "decision",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "approvalState",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "approvalRoute",
            TypeRef::named(MUTATION_APPROVAL_ROUTE_TYPE),
        ))
        .field(json_object_field(
            "expiresAtNs",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "reviewSummary",
            TypeRef::named_nn("JSON"),
        ))
}

fn mutation_preview_result_object() -> Object {
    Object::new(MUTATION_PREVIEW_RESULT_TYPE)
        .field(json_object_field(
            "decision",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "intent",
            TypeRef::named(MUTATION_INTENT_TYPE),
        ))
        .field(json_object_field(
            "intentToken",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "canonicalOperation",
            TypeRef::named_nn(CANONICAL_OPERATION_TYPE),
        ))
        .field(json_object_field("diff", TypeRef::named_nn("JSON")))
        .field(json_object_field(
            "affectedRecords",
            TypeRef::named_nn_list_nn(MUTATION_INTENT_PRE_IMAGE_TYPE),
        ))
        .field(json_object_field(
            "affectedFields",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "approvalRoute",
            TypeRef::named(MUTATION_APPROVAL_ROUTE_TYPE),
        ))
        .field(json_object_field(
            "policyExplanation",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
        ))
}

fn mutation_intent_stale_dimension_object() -> Object {
    Object::new(MUTATION_INTENT_STALE_DIMENSION_TYPE)
        .field(json_object_field(
            "dimension",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "expected",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field("actual", TypeRef::named(TypeRef::STRING)))
        .field(json_object_field("path", TypeRef::named(TypeRef::STRING)))
}

fn mutation_intent_edge_object() -> Object {
    Object::new(MUTATION_INTENT_EDGE_TYPE)
        .field(json_object_field(
            "cursor",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .field(json_object_field(
            "node",
            TypeRef::named_nn(MUTATION_INTENT_TYPE),
        ))
}

fn mutation_intent_connection_object() -> Object {
    Object::new(MUTATION_INTENT_CONNECTION_TYPE)
        .field(json_object_field(
            "edges",
            TypeRef::named_nn_list_nn(MUTATION_INTENT_EDGE_TYPE),
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

fn commit_intent_result_object() -> Object {
    Object::new(COMMIT_INTENT_RESULT_TYPE)
        .field(json_object_field(
            "committed",
            TypeRef::named_nn(TypeRef::BOOLEAN),
        ))
        .field(json_object_field(
            "intent",
            TypeRef::named(MUTATION_INTENT_TYPE),
        ))
        .field(json_object_field(
            "transactionId",
            TypeRef::named(TypeRef::STRING),
        ))
        .field(json_object_field(
            "auditEntry",
            TypeRef::named(AUDIT_ENTRY_TYPE),
        ))
        .field(json_object_field(
            "stale",
            TypeRef::named_nn_list_nn(MUTATION_INTENT_STALE_DIMENSION_TYPE),
        ))
        .field(json_object_field(
            "errorCode",
            TypeRef::named(TypeRef::STRING),
        ))
}

fn empty_mutation_intent_connection_value() -> FieldValue<'static> {
    json_to_field_value(json!({
        "edges": [],
        "pageInfo": {
            "hasNextPage": false,
            "hasPreviousPage": false,
            "startCursor": Value::Null,
            "endCursor": Value::Null,
        },
        "totalCount": 0,
    }))
}

fn mutation_intents_not_implemented() -> GqlError {
    GqlError::new("mutation intent GraphQL execution is not implemented yet").extend_with(
        |_err, ext| {
            ext.set("code", "INTENT_GRAPHQL_NOT_IMPLEMENTED");
        },
    )
}

fn add_intent_root_query_fields(mut query: Object) -> Object {
    query = query.field(
        Field::new(
            "mutationIntent",
            TypeRef::named(MUTATION_INTENT_TYPE),
            |_ctx| FieldFuture::new(async move { Ok(None::<FieldValue>) }),
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID))),
    );
    query = query.field(
        Field::new(
            "pendingMutationIntents",
            TypeRef::named_nn(MUTATION_INTENT_CONNECTION_TYPE),
            |_ctx| {
                FieldFuture::new(async move { Ok(Some(empty_mutation_intent_connection_value())) })
            },
        )
        .argument(InputValue::new(
            "filter",
            TypeRef::named(MUTATION_INTENT_FILTER_INPUT),
        ))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING))),
    );
    query
}

fn add_handler_intent_root_query_fields<S: StorageAdapter + 'static>(
    mut query: Object,
    handler: SharedHandler<S>,
) -> Object {
    let lookup_handler = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "mutationIntent",
            TypeRef::named(MUTATION_INTENT_TYPE),
            move |ctx| {
                let handler = Arc::clone(&lookup_handler);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(
                    async move { mutation_intent_resolver(ctx, handler, caller).await },
                )
            },
        )
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID))),
    );
    let inbox_handler = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "pendingMutationIntents",
            TypeRef::named_nn(MUTATION_INTENT_CONNECTION_TYPE),
            move |ctx| {
                let handler = Arc::clone(&inbox_handler);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    pending_mutation_intents_resolver(ctx, handler, caller).await
                })
            },
        )
        .argument(InputValue::new(
            "filter",
            TypeRef::named(MUTATION_INTENT_FILTER_INPUT),
        ))
        .argument(InputValue::new("limit", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING))),
    );
    query
}

fn add_intent_root_mutation_fields(mut mutation: Object) -> Object {
    mutation = mutation.field(
        Field::new(
            "previewMutation",
            TypeRef::named_nn(MUTATION_PREVIEW_RESULT_TYPE),
            |_ctx| {
                FieldFuture::new(async move {
                    Err::<Option<FieldValue<'static>>, GqlError>(mutation_intents_not_implemented())
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(MUTATION_PREVIEW_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new(
            "approveMutationIntent",
            TypeRef::named_nn(MUTATION_INTENT_TYPE),
            |_ctx| {
                FieldFuture::new(async move {
                    Err::<Option<FieldValue<'static>>, GqlError>(mutation_intents_not_implemented())
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(APPROVE_INTENT_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new(
            "rejectMutationIntent",
            TypeRef::named_nn(MUTATION_INTENT_TYPE),
            |_ctx| {
                FieldFuture::new(async move {
                    Err::<Option<FieldValue<'static>>, GqlError>(mutation_intents_not_implemented())
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(REJECT_INTENT_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new(
            "commitMutationIntent",
            TypeRef::named_nn(COMMIT_INTENT_RESULT_TYPE),
            |_ctx| {
                FieldFuture::new(async move {
                    Err::<Option<FieldValue<'static>>, GqlError>(mutation_intents_not_implemented())
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(COMMIT_INTENT_INPUT),
        )),
    );
    mutation
}

fn add_handler_intent_root_mutation_fields<S: StorageAdapter + 'static>(
    mut mutation: Object,
    handler: SharedHandler<S>,
) -> Object {
    let preview_handler = Arc::clone(&handler);
    mutation = mutation.field(
        Field::new(
            "previewMutation",
            TypeRef::named_nn(MUTATION_PREVIEW_RESULT_TYPE),
            move |ctx| {
                let handler = Arc::clone(&preview_handler);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(
                    async move { preview_mutation_resolver(ctx, handler, caller).await },
                )
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(MUTATION_PREVIEW_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new(
            "approveMutationIntent",
            TypeRef::named_nn(MUTATION_INTENT_TYPE),
            {
                let handler = Arc::clone(&handler);
                move |ctx| {
                    let handler = Arc::clone(&handler);
                    let caller = caller_from_ctx(&ctx);
                    FieldFuture::new(async move {
                        approve_mutation_intent_resolver(ctx, handler, caller).await
                    })
                }
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(APPROVE_INTENT_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new(
            "rejectMutationIntent",
            TypeRef::named_nn(MUTATION_INTENT_TYPE),
            {
                let handler = Arc::clone(&handler);
                move |ctx| {
                    let handler = Arc::clone(&handler);
                    let caller = caller_from_ctx(&ctx);
                    FieldFuture::new(async move {
                        reject_mutation_intent_resolver(ctx, handler, caller).await
                    })
                }
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(REJECT_INTENT_INPUT),
        )),
    );
    mutation = mutation.field(
        Field::new(
            "commitMutationIntent",
            TypeRef::named_nn(COMMIT_INTENT_RESULT_TYPE),
            {
                let handler = Arc::clone(&handler);
                move |ctx| {
                    let handler = Arc::clone(&handler);
                    let caller = caller_from_ctx(&ctx);
                    FieldFuture::new(async move {
                        commit_mutation_intent_resolver(ctx, handler, caller).await
                    })
                }
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(COMMIT_INTENT_INPUT),
        )),
    );
    mutation
}

fn register_root_objects(mut schema_builder: SchemaBuilder) -> SchemaBuilder {
    schema_builder = schema_builder
        .register(page_info_object())
        .register(generic_entity_object())
        .register(entity_edge_object())
        .register(entity_connection_object())
        .register(collection_meta_object())
        .register(collection_template_object())
        .register(effective_policy_object())
        .register(policy_rule_match_object())
        .register(policy_approval_envelope_object())
        .register(policy_explanation_object())
        .register(rendered_entity_object())
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
        .register(delete_collection_template_payload_object())
        .register(revert_audit_entry_payload_object())
        .register(put_schema_payload_object())
        .register(rollback_entity_payload_object())
        .register(canonical_operation_object())
        .register(mutation_approval_route_object())
        .register(mutation_intent_pre_image_object())
        .register(mutation_intent_object())
        .register(mutation_preview_result_object())
        .register(mutation_intent_stale_dimension_object())
        .register(mutation_intent_edge_object())
        .register(mutation_intent_connection_object())
        .register(commit_intent_result_object());
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
            let caller = caller_from_ctx(&ctx);
            FieldFuture::new(async move {
                let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                let id = ctx.args.try_get("id")?.string()?.to_owned();
                let collection_id = CollectionId::new(collection);
                let guard = handler.lock().await;
                let schema = guard
                    .get_schema(&collection_id)
                    .map_err(axon_error_to_gql)?;
                match guard.get_entity_with_caller(
                    GetEntityRequest {
                        collection: collection_id,
                        id: EntityId::new(id),
                    },
                    &caller,
                    None,
                ) {
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

    let handler_effective_policy = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "effectivePolicy",
            TypeRef::named_nn(EFFECTIVE_POLICY_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_effective_policy);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                    let entity_id = ctx
                        .args
                        .try_get("entityId")
                        .ok()
                        .and_then(|value| value.string().ok())
                        .map(EntityId::new);
                    let policy_override = policy_override_json_arg(&ctx)?;
                    let guard = handler.lock().await;
                    let collection_id = CollectionId::new(collection);
                    let override_plan =
                        policy_override_plan_from_arg(&guard, &collection_id, policy_override)?;
                    let policy = match override_plan {
                        Some(override_plan) => guard.effective_policy_with_plan(
                            collection_id.clone(),
                            entity_id.clone(),
                            &caller,
                            None,
                            &override_plan.schema,
                            &override_plan.plan,
                            &override_plan.plans,
                        ),
                        None => guard.effective_policy_with_caller(
                            collection_id,
                            entity_id,
                            &caller,
                            None,
                        ),
                    }
                    .map_err(axon_error_to_gql)?;
                    Ok(Some(json_to_field_value(json!({
                        "collection": policy.collection,
                        "canRead": policy.can_read,
                        "canCreate": policy.can_create,
                        "canUpdate": policy.can_update,
                        "canDelete": policy.can_delete,
                        "redactedFields": policy.redacted_fields,
                        "deniedFields": policy.denied_fields,
                        "policyVersion": policy.policy_version,
                    }))))
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("entityId", TypeRef::named(TypeRef::ID)))
        .argument(InputValue::new("policyOverride", TypeRef::named("JSON"))),
    );

    let handler_explain_policy = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "explainPolicy",
            TypeRef::named_nn(POLICY_EXPLANATION_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_explain_policy);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let input = explain_policy_request_from_value(&gql_input_to_json(
                        ctx.args.try_get("input")?.as_value(),
                    )?)?;
                    let policy_override = policy_override_json_arg(&ctx)?;
                    let has_policy_override = policy_override.is_some();
                    let guard = handler.lock().await;
                    let explanation = if let Some(collection) =
                        explain_policy_override_collection(&input)
                    {
                        match policy_override_plan_from_arg(&guard, &collection, policy_override)? {
                            Some(override_plan) => guard.explain_policy_with_plan(
                                input,
                                &caller,
                                None,
                                &override_plan.schema,
                                &override_plan.plan,
                                &override_plan.plans,
                                None,
                            ),
                            None => guard.explain_policy_with_caller(input, &caller, None),
                        }
                    } else if has_policy_override {
                        Err(AxonError::InvalidArgument(
                            "collection is required when policyOverride is supplied".into(),
                        ))
                    } else {
                        guard.explain_policy_with_caller(input, &caller, None)
                    }
                    .map_err(axon_error_to_gql)?;
                    Ok(Some(json_to_field_value(policy_explanation_json(
                        &explanation,
                    ))))
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(EXPLAIN_POLICY_INPUT),
        ))
        .argument(InputValue::new("policyOverride", TypeRef::named("JSON"))),
    );

    let handler_entities = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "entities",
            TypeRef::named_nn(ENTITY_CONNECTION_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_entities);
                let caller = caller_from_ctx(&ctx);
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
                    match guard.query_entities_with_caller(
                        QueryEntitiesRequest {
                            collection: collection_id,
                            filter,
                            sort,
                            limit,
                            after_id,
                            count_only: false,
                        },
                        &caller,
                        None,
                    ) {
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
                let caller = caller_from_ctx(&ctx);
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
                        .find_link_candidates_with_caller(
                            FindLinkCandidatesRequest {
                                source_collection: CollectionId::new(source_collection),
                                source_id: EntityId::new(source_id),
                                link_type,
                                filter,
                                limit: request_limit,
                            },
                            &caller,
                            None,
                        )
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
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                    let id = ctx.args.try_get("id")?.string()?.to_owned();
                    let link_type = parse_optional_string_arg(&ctx, "linkType")?;
                    let direction = parse_optional_string_arg(&ctx, "direction")?
                        .as_deref()
                        .map(parse_neighbor_direction)
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
                        .get_entity_with_caller(
                            GetEntityRequest {
                                collection: collection_id.clone(),
                                id: entity_id.clone(),
                            },
                            &caller,
                            None,
                        )
                        .map_err(axon_error_to_gql)?;

                    let mut edges = Vec::new();
                    for (direction, label) in directions {
                        let response = guard
                            .traverse_with_caller(
                                TraverseRequest {
                                    collection: collection_id.clone(),
                                    id: entity_id.clone(),
                                    link_type: link_type.clone(),
                                    max_depth: Some(1),
                                    direction,
                                    hop_filter: None,
                                },
                                &caller,
                                None,
                            )
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

    let handler_collection_template = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "collectionTemplate",
            TypeRef::named(COLLECTION_TEMPLATE_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_collection_template);
                FieldFuture::new(async move {
                    let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                    let guard = handler.lock().await;
                    match guard.get_collection_template(GetCollectionTemplateRequest {
                        collection: CollectionId::new(collection),
                    }) {
                        Ok(resp) => Ok(Some(json_to_field_value(collection_template_json(
                            &resp.view,
                            &[],
                        )))),
                        Err(AxonError::NotFound(_)) => Ok(None),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        )),
    );

    let handler_rendered_entity = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "renderedEntity",
            TypeRef::named(RENDERED_ENTITY_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_rendered_entity);
                FieldFuture::new(async move {
                    let collection = ctx.args.try_get("collection")?.string()?.to_owned();
                    let id = ctx.args.try_get("id")?.string()?.to_owned();
                    let collection_id = CollectionId::new(collection);
                    let entity_id = EntityId::new(id);
                    let guard = handler.lock().await;
                    let schema = guard
                        .get_schema(&collection_id)
                        .map_err(axon_error_to_gql)?;
                    match guard.get_entity_markdown(&collection_id, &entity_id) {
                        Ok(axon_api::response::GetEntityMarkdownResponse::Rendered {
                            entity,
                            rendered_markdown,
                        }) => Ok(Some(json_to_field_value(json!({
                            "entity": entity_to_generic_json_with_schema(&entity, schema.as_ref()),
                            "markdown": rendered_markdown,
                        })))),
                        Ok(axon_api::response::GetEntityMarkdownResponse::RenderFailed {
                            detail,
                            ..
                        }) => Err(axon_error_to_gql(AxonError::InvalidOperation(detail))),
                        Err(e) => Err(axon_error_to_gql(e)),
                    }
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID))),
    );

    let handler_audit = Arc::clone(&handler);
    query = query.field(
        Field::new(
            "auditLog",
            TypeRef::named_nn(AUDIT_CONNECTION_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_audit);
                let caller = caller_from_ctx(&ctx);
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
                    match guard.query_audit_with_caller(
                        QueryAuditRequest {
                            database: None,
                            collection,
                            collection_ids: Vec::new(),
                            entity_id,
                            actor,
                            operation,
                            intent_id: None,
                            approval_id: None,
                            since_ns,
                            until_ns,
                            after_id,
                            limit,
                        },
                        &caller,
                        None,
                    ) {
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
            "collectionTemplate",
            TypeRef::named(COLLECTION_TEMPLATE_TYPE),
            |_ctx| FieldFuture::new(async move { Ok(None::<FieldValue>) }),
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        )),
    );
    query = query.field(
        Field::new(
            "renderedEntity",
            TypeRef::named(RENDERED_ENTITY_TYPE),
            |_ctx| FieldFuture::new(async move { Ok(None::<FieldValue>) }),
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ))
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID))),
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
                if let Ok(field_errors) = GqlValue::from_json(json!(
                    axon_core::error::schema_validation_field_errors(&detail)
                )) {
                    ext.set("fieldErrors", field_errors);
                }
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
        AxonError::PolicyDenied(denial) => {
            let code = if denial.is_policy_filter_unindexed() {
                "POLICY_FILTER_UNINDEXED"
            } else {
                "forbidden"
            };
            let detail = denial.detail();
            GqlError::new(denial.to_string()).extend_with(move |_err, ext| {
                ext.set("code", code);
                if let Ok(value) = GqlValue::from_json(detail.clone()) {
                    ext.set("detail", value);
                }
            })
        }
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
        access_control: optional_schema_field(obj, "access_control", "accessControl")?,
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
        "policyCompileReport": resp.policy_compile_report,
        "dryRunExplanations": resp.dry_run_explanations,
        "dryRun": resp.dry_run,
    })
}

fn empty_explain_policy_request(operation: impl Into<String>) -> ExplainPolicyRequest {
    ExplainPolicyRequest {
        operation: operation.into(),
        collection: None,
        entity_id: None,
        expected_version: None,
        data: None,
        patch: None,
        lifecycle_name: None,
        target_state: None,
        to_version: None,
        operations: Vec::new(),
        actor_override: None,
    }
}

fn explain_policy_request_from_value(value: &Value) -> Result<ExplainPolicyRequest, GqlError> {
    let input = input_object(value, "input")?;
    let mut request = empty_explain_policy_request(input_string(input, "operation")?);
    request.collection = input
        .get("collection")
        .and_then(Value::as_str)
        .map(CollectionId::new);
    request.entity_id = input
        .get("entityId")
        .and_then(Value::as_str)
        .map(EntityId::new);
    request.expected_version = input.get("expectedVersion").and_then(Value::as_u64);
    request.data = input.get("data").cloned();
    request.patch = input.get("patch").cloned();
    request.lifecycle_name = input
        .get("lifecycleName")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    request.target_state = input
        .get("targetState")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    request.to_version = input.get("toVersion").and_then(Value::as_u64);
    request.operations = input
        .get("operations")
        .map(explain_transaction_operations_from_value)
        .transpose()?
        .unwrap_or_default();
    request.actor_override = input
        .get("actor")
        .filter(|v| !v.is_null())
        .map(explain_actor_override_from_value)
        .transpose()?;
    Ok(request)
}

#[derive(Debug, Clone)]
struct PolicyOverridePlan {
    schema: CollectionSchema,
    plan: PolicyPlan,
    plans: HashMap<String, PolicyPlan>,
}

fn policy_override_json_arg(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
) -> Result<Option<Value>, GqlError> {
    match ctx.args.try_get("policyOverride") {
        Ok(value) if !matches!(value.as_value(), GqlValue::Null) => {
            gql_input_to_json(value.as_value()).map(Some)
        }
        _ => Ok(None),
    }
}

fn explain_policy_override_collection(req: &ExplainPolicyRequest) -> Option<CollectionId> {
    req.collection.clone().or_else(|| {
        req.operations
            .iter()
            .find_map(explain_policy_override_collection)
    })
}

fn policy_override_plan_from_arg<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &CollectionId,
    value: Option<Value>,
) -> Result<Option<PolicyOverridePlan>, GqlError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let access_control = serde_json::from_value::<AccessControlPolicy>(value).map_err(|err| {
        invalid_policy_override_diagnostics(
            collection,
            vec![json!({
                "code": "invalid_policy_override",
                "message": format!("invalid policyOverride: {err}"),
                "collection": collection.as_str(),
                "path": "policyOverride",
            })],
        )
    })?;
    let mut schema = handler
        .get_schema(collection)
        .map_err(axon_error_to_gql)?
        .ok_or_else(|| axon_error_to_gql(AxonError::NotFound(collection.to_string())))?;
    schema.access_control = Some(access_control);

    let schemas = handler
        .policy_catalog_schemas(&schema)
        .map_err(axon_error_to_gql)?;
    let catalog = compile_policy_catalog(&schemas)
        .map_err(|err| invalid_policy_override_compile_error(collection, &err))?;
    let plan = catalog
        .plans
        .get(collection.as_str())
        .cloned()
        .ok_or_else(|| {
            invalid_policy_override_diagnostics(
                collection,
                vec![json!({
                    "code": "invalid_policy_override",
                    "message": "policyOverride did not compile a plan for the requested collection",
                    "collection": collection.as_str(),
                    "path": "policyOverride",
                })],
            )
        })?;

    Ok(Some(PolicyOverridePlan {
        schema,
        plan,
        plans: catalog.plans,
    }))
}

fn invalid_policy_override_compile_error(
    collection: &CollectionId,
    err: &PolicyCompileError,
) -> GqlError {
    let report = axon_schema::PolicyCompileReport::from_compile_error(err);
    let diagnostics = report
        .errors
        .into_iter()
        .map(|mut diagnostic| {
            diagnostic.code = "invalid_policy_override".into();
            if diagnostic.collection.is_none() {
                diagnostic.collection = Some(collection.to_string());
            }
            serde_json::to_value(diagnostic).unwrap_or_else(|_| {
                json!({
                    "code": "invalid_policy_override",
                    "message": err.message(),
                    "collection": collection.as_str(),
                })
            })
        })
        .collect();
    invalid_policy_override_diagnostics(collection, diagnostics)
}

fn invalid_policy_override_diagnostics(
    collection: &CollectionId,
    diagnostics: Vec<Value>,
) -> GqlError {
    let collection = collection.to_string();
    GqlError::new("invalid policyOverride").extend_with(move |_err, ext| {
        ext.set("code", "invalid_policy_override");
        ext.set("collection", collection.as_str());
        if let Ok(value) = GqlValue::from_json(Value::Array(diagnostics.clone())) {
            ext.set("diagnostics", value);
        }
    })
}

fn explain_actor_override_from_value(value: &Value) -> Result<ExplainActorOverride, GqlError> {
    let obj = value.as_object().ok_or_else(|| {
        GqlError::new("actor must be a JSON object").extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })
    })?;
    let mut subject = HashMap::new();
    if let Some(subject_value) = obj.get("subject") {
        if !subject_value.is_null() {
            let subject_obj = subject_value.as_object().ok_or_else(|| {
                GqlError::new("actor.subject must be a JSON object").extend_with(|_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                })
            })?;
            for (k, v) in subject_obj {
                subject.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(ExplainActorOverride {
        actor: obj
            .get("actor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        role: obj
            .get("role")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        subject,
    })
}

fn explain_transaction_operations_from_value(
    value: &Value,
) -> Result<Vec<ExplainPolicyRequest>, GqlError> {
    let operations = value.as_array().ok_or_else(|| {
        GqlError::new("operations must be a list").extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })
    })?;
    operations
        .iter()
        .enumerate()
        .map(explain_transaction_operation_from_value)
        .collect()
}

fn explain_transaction_operation_from_value(
    (index, op): (usize, &Value),
) -> Result<ExplainPolicyRequest, GqlError> {
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
    let mut request = match variant {
        "createEntity" => {
            let mut request = empty_explain_policy_request("create");
            request.collection = Some(CollectionId::new(required_str(
                payload,
                "collection",
                index,
            )?));
            request.entity_id = Some(EntityId::new(required_str(payload, "id", index)?));
            request.data = payload.get("data").cloned();
            request
        }
        "updateEntity" => {
            let mut request = empty_explain_policy_request("update");
            request.collection = Some(CollectionId::new(required_str(
                payload,
                "collection",
                index,
            )?));
            request.entity_id = Some(EntityId::new(required_str(payload, "id", index)?));
            request.expected_version = Some(required_u64(payload, "expectedVersion", index)?);
            request.data = payload.get("data").cloned();
            request
        }
        "patchEntity" => {
            let mut request = empty_explain_policy_request("patch");
            request.collection = Some(CollectionId::new(required_str(
                payload,
                "collection",
                index,
            )?));
            request.entity_id = Some(EntityId::new(required_str(payload, "id", index)?));
            request.expected_version = Some(required_u64(payload, "expectedVersion", index)?);
            request.patch = payload.get("patch").cloned();
            request
        }
        "deleteEntity" => {
            let mut request = empty_explain_policy_request("delete");
            request.collection = Some(CollectionId::new(required_str(
                payload,
                "collection",
                index,
            )?));
            request.entity_id = Some(EntityId::new(required_str(payload, "id", index)?));
            request.expected_version = Some(required_u64(payload, "expectedVersion", index)?);
            request
        }
        "createLink" => empty_explain_policy_request("create_link"),
        "deleteLink" => empty_explain_policy_request("delete_link"),
        other => {
            return Err(
                GqlError::new(format!("unsupported transaction operation '{other}'")).extend_with(
                    move |_err, ext| {
                        ext.set("code", "INVALID_ARGUMENT");
                        ext.set("operationIndex", index as i32);
                    },
                ),
            );
        }
    };
    request.operation = request.operation.trim().to_ascii_lowercase();
    Ok(request)
}

fn policy_explanation_json(response: &axon_api::response::PolicyExplanationResponse) -> Value {
    json!({
        "operation": response.operation,
        "collection": response.collection,
        "entityId": response.entity_id,
        "operationIndex": response.operation_index,
        "decision": response.decision,
        "reason": response.reason,
        "policyVersion": response.policy_version,
        "ruleIds": response.rule_ids,
        "policyIds": response.policy_ids,
        "fieldPaths": response.field_paths,
        "deniedFields": response.denied_fields,
        "rules": response.rules.iter().map(policy_rule_match_json).collect::<Vec<_>>(),
        "approval": response.approval.as_ref().map(policy_approval_envelope_json),
        "operations": response
            .operations
            .iter()
            .map(policy_explanation_json)
            .collect::<Vec<_>>(),
    })
}

fn policy_rule_match_json(rule: &axon_api::response::PolicyRuleMatch) -> Value {
    json!({
        "ruleId": rule.rule_id,
        "name": rule.name,
        "kind": rule.kind,
        "fieldPath": rule.field_path,
    })
}

fn policy_approval_envelope_json(
    approval: &axon_api::response::PolicyApprovalEnvelopeSummary,
) -> Value {
    json!({
        "policyId": approval.policy_id,
        "name": approval.name,
        "decision": approval.decision,
        "role": approval.role,
        "reasonRequired": approval.reason_required,
        "deadlineSeconds": approval.deadline_seconds,
        "separationOfDuties": approval.separation_of_duties,
    })
}

#[derive(Debug, Clone)]
struct MutationPreviewComputation {
    explain_request: ExplainPolicyRequest,
    pre_images: Vec<PreImageBinding>,
    diff: Value,
    affected_fields: Vec<String>,
    schema_version: u32,
}

#[derive(Debug, Clone)]
struct MutationIntentQueryFilter {
    states: Vec<ApprovalState>,
    decision: Option<MutationIntentDecision>,
    include_expired: bool,
}

async fn mutation_intent_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Read).map_err(axon_error_to_gql)?;
    let intent_id = ctx.args.try_get("id")?.string()?.to_owned();
    let scope = graphql_intent_scope(&ctx);
    let now_ns = current_time_ns();
    let service = graphql_intent_lifecycle_service();

    let mut guard = handler.lock().await;
    service
        .expire_due(guard.storage_mut(), &scope, now_ns, None)
        .map_err(mutation_intent_lifecycle_error_to_gql)?;
    let intent = guard
        .storage_ref()
        .get_mutation_intent(&scope.tenant_id, &scope.database_id, &intent_id)
        .map_err(axon_error_to_gql)?;

    match intent {
        Some(mut intent) => {
            guard
                .redact_mutation_intent_for_read(&mut intent, &caller, None)
                .map_err(axon_error_to_gql)?;
            Ok(Some(json_to_field_value(mutation_intent_json(&intent))))
        }
        None => Ok(None),
    }
}

async fn pending_mutation_intents_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Read).map_err(axon_error_to_gql)?;
    let filter = mutation_intent_query_filter(&ctx)?;
    let limit = parse_optional_limit_arg(&ctx, "limit")?;
    let after = parse_optional_string_arg(&ctx, "after")?;
    let scope = graphql_intent_scope(&ctx);
    let now_ns = current_time_ns();
    let service = graphql_intent_lifecycle_service();

    let mut guard = handler.lock().await;
    let mut intents = if filter.states.is_empty() {
        let mut pending = service
            .list_pending(guard.storage_mut(), &scope, now_ns, None)
            .map_err(mutation_intent_lifecycle_error_to_gql)?;
        if filter.include_expired {
            pending.extend(
                service
                    .list_by_state(
                        guard.storage_mut(),
                        &scope,
                        ApprovalState::Expired,
                        now_ns,
                        None,
                    )
                    .map_err(mutation_intent_lifecycle_error_to_gql)?,
            );
        }
        pending
    } else {
        let mut by_state = Vec::new();
        for state in &filter.states {
            by_state.extend(
                service
                    .list_by_state(guard.storage_mut(), &scope, state.clone(), now_ns, None)
                    .map_err(mutation_intent_lifecycle_error_to_gql)?,
            );
        }
        by_state
    };

    if let Some(decision) = filter.decision {
        intents.retain(|intent| intent.decision == decision);
    }
    sort_mutation_intents(&mut intents);
    let total_count = intents.len();
    let (mut page, has_previous_page, has_next_page) =
        paginate_mutation_intents(intents, after.as_deref(), limit)?;
    for intent in &mut page {
        guard
            .redact_mutation_intent_for_read(intent, &caller, None)
            .map_err(axon_error_to_gql)?;
    }

    Ok(Some(mutation_intent_connection_value(
        &page,
        total_count,
        has_next_page,
        has_previous_page,
    )))
}

async fn approve_mutation_intent_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    review_mutation_intent(ctx, handler, caller, true).await
}

async fn reject_mutation_intent_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    review_mutation_intent(ctx, handler, caller, false).await
}

async fn review_mutation_intent<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
    approve: bool,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller
        .check(Operation::Write)
        .map_err(|error| mutation_intent_authorization_error(error.to_string()))?;
    let input = gql_input_to_json(ctx.args.try_get("input")?.as_value())?;
    let input_obj = input_object(&input, "input")?;
    let intent_id = input_string(input_obj, "intentId")?;
    let metadata = MutationIntentReviewMetadata {
        actor: Some(caller.actor.clone()),
        reason: string_member(Some(input_obj), "reason"),
    };
    let scope = graphql_intent_scope(&ctx);
    let now_ns = current_time_ns();
    let service = graphql_intent_lifecycle_service();

    let mut guard = handler.lock().await;
    service
        .expire_due(guard.storage_mut(), &scope, now_ns, None)
        .map_err(mutation_intent_lifecycle_error_to_gql)?;
    let intent = guard
        .storage_ref()
        .get_mutation_intent(&scope.tenant_id, &scope.database_id, &intent_id)
        .map_err(axon_error_to_gql)?
        .ok_or_else(|| {
            mutation_intent_lifecycle_error_to_gql(
                axon_api::intent::MutationIntentLifecycleError::NotFound {
                    intent_id: intent_id.clone(),
                },
            )
        })?;
    authorize_mutation_intent_review(&guard, &caller, &intent)?;

    let (storage, audit) = guard.storage_and_audit_mut();
    let intent = if approve {
        service.approve_with_audit(storage, audit, &scope, &intent_id, metadata, now_ns)
    } else {
        service.reject_with_audit(storage, audit, &scope, &intent_id, metadata, now_ns)
    }
    .map_err(mutation_intent_lifecycle_error_to_gql)?;

    Ok(Some(json_to_field_value(mutation_intent_json(&intent))))
}

fn authorize_mutation_intent_review<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    caller: &CallerIdentity,
    intent: &MutationIntent,
) -> Result<(), GqlError> {
    if intent.decision != MutationIntentDecision::NeedsApproval
        || intent.approval_state != ApprovalState::Pending
    {
        return Ok(());
    }

    let route = intent.approval_route.as_ref().ok_or_else(|| {
        mutation_intent_authorization_error(format!(
            "mutation intent '{}' does not include an approval route",
            intent.intent_id
        ))
    })?;
    let required_role = route
        .role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .ok_or_else(|| {
            mutation_intent_authorization_error(format!(
                "mutation intent '{}' does not include a required approver role",
                intent.intent_id
            ))
        })?;

    if !caller_has_required_approval_role(handler, caller, intent, required_role)? {
        return Err(mutation_intent_authorization_error(format!(
            "caller '{}' does not satisfy required approver role '{}'",
            caller.actor, required_role
        )));
    }

    if route.separation_of_duties && caller_matches_intent_subject(caller, &intent.subject) {
        return Err(mutation_intent_authorization_error(format!(
            "caller '{}' cannot review their own mutation intent",
            caller.actor
        )));
    }

    Ok(())
}

fn caller_has_required_approval_role<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    caller: &CallerIdentity,
    intent: &MutationIntent,
    required_role: &str,
) -> Result<bool, GqlError> {
    let mut saw_policy_role = false;
    for collection in mutation_intent_policy_collections(intent) {
        let Some(subject) = handler
            .policy_subject_snapshot_with_caller(&collection, caller, None)
            .map_err(axon_error_to_gql)?
        else {
            continue;
        };
        let roles = policy_subject_role_values(&subject);
        if roles.is_empty() {
            continue;
        }
        saw_policy_role = true;
        if roles.iter().any(|role| role == required_role) {
            return Ok(true);
        }
    }

    if saw_policy_role {
        return Ok(false);
    }

    Ok(caller.role.to_string() == required_role)
}

fn policy_subject_role_values(subject: &PolicySubjectSnapshot) -> Vec<String> {
    let mut roles = Vec::new();
    if let Some(value) = subject.bindings.get("role") {
        push_role_values(&mut roles, value);
    }
    for (name, value) in &subject.attributes {
        if name == "role" || name.ends_with("_role") {
            push_role_values(&mut roles, value);
        }
    }
    roles.sort();
    roles.dedup();
    roles
}

fn push_role_values(roles: &mut Vec<String>, value: &Value) {
    match value {
        Value::String(role) if !role.trim().is_empty() => roles.push(role.trim().to_string()),
        Value::Array(values) => {
            for value in values {
                push_role_values(roles, value);
            }
        }
        _ => {}
    }
}

fn caller_matches_intent_subject(
    caller: &CallerIdentity,
    subject: &MutationIntentSubjectBinding,
) -> bool {
    [
        subject.user_id.as_deref(),
        subject.delegated_by.as_deref(),
        subject.agent_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|principal| principal == caller.actor)
}

fn mutation_intent_authorization_error(message: impl Into<String>) -> GqlError {
    GqlError::new(message.into()).extend_with(|_err, ext| {
        ext.set("code", INTENT_AUTHORIZATION_FAILED_CODE);
    })
}

fn mutation_intent_policy_collections(intent: &MutationIntent) -> Vec<CollectionId> {
    let mut collections = Vec::new();
    let mut seen = HashSet::new();
    if let Some(payload) = intent.operation.canonical_operation.as_ref() {
        append_operation_collections(
            &mut collections,
            &mut seen,
            &intent.operation.operation_kind,
            payload,
        );
    }
    for pre_image in &intent.pre_images {
        if let PreImageBinding::Entity { collection, .. } = pre_image {
            push_unique_collection(&mut collections, &mut seen, collection.as_str());
        }
    }
    collections
}

fn append_operation_collections(
    collections: &mut Vec<CollectionId>,
    seen: &mut HashSet<String>,
    operation_kind: &MutationOperationKind,
    payload: &Value,
) {
    match operation_kind {
        MutationOperationKind::CreateEntity
        | MutationOperationKind::UpdateEntity
        | MutationOperationKind::PatchEntity
        | MutationOperationKind::DeleteEntity
        | MutationOperationKind::Transition
        | MutationOperationKind::Rollback => {
            push_unique_collection(
                collections,
                seen,
                payload
                    .get("collection")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
        }
        MutationOperationKind::CreateLink | MutationOperationKind::DeleteLink => {
            push_unique_collection(
                collections,
                seen,
                payload
                    .get("source_collection")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
            push_unique_collection(
                collections,
                seen,
                payload
                    .get("target_collection")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
        }
        MutationOperationKind::Transaction => {
            if let Some(operations) = payload.get("operations").and_then(Value::as_array) {
                for operation in operations {
                    append_transaction_operation_collections(collections, seen, operation);
                }
            }
        }
        MutationOperationKind::Revert => {}
    }
}

fn append_transaction_operation_collections(
    collections: &mut Vec<CollectionId>,
    seen: &mut HashSet<String>,
    operation: &Value,
) {
    let Some(obj) = operation.as_object() else {
        return;
    };
    for (variant, payload) in obj {
        if payload.is_null() {
            continue;
        }
        match variant.as_str() {
            "createEntity" | "updateEntity" | "patchEntity" | "deleteEntity" => {
                push_unique_collection(
                    collections,
                    seen,
                    payload
                        .get("collection")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                );
            }
            "createLink" | "deleteLink" => {
                push_unique_collection(
                    collections,
                    seen,
                    payload
                        .get("sourceCollection")
                        .or_else(|| payload.get("source_collection"))
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                );
                push_unique_collection(
                    collections,
                    seen,
                    payload
                        .get("targetCollection")
                        .or_else(|| payload.get("target_collection"))
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                );
            }
            _ => {}
        }
    }
}

fn push_unique_collection(
    collections: &mut Vec<CollectionId>,
    seen: &mut HashSet<String>,
    collection: &str,
) {
    let collection = collection.trim();
    if collection.is_empty() || !seen.insert(collection.to_string()) {
        return;
    }
    collections.push(CollectionId::new(collection));
}

async fn commit_mutation_intent_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    let input = gql_input_to_json(ctx.args.try_get("input")?.as_value())?;
    let input_obj = input_object(&input, "input")?;
    let token = MutationIntentToken::new(input_string(input_obj, "intentToken")?);
    let token_intent_id = graphql_intent_token_signer()
        .verify(&token)
        .map_err(mutation_intent_token_error_to_gql)?;
    if let Some(intent_id) = input_obj.get("intentId").and_then(Value::as_str) {
        if intent_id != token_intent_id {
            return Err(
                GqlError::new("intentId does not match intentToken").extend_with(|_err, ext| {
                    ext.set("code", "intent_token_invalid");
                }),
            );
        }
    }
    let supplied_operation = input_obj
        .get("operation")
        .filter(|value| !value.is_null())
        .map(canonical_operation_from_input)
        .transpose()?;
    let scope = graphql_intent_scope(&ctx);
    let now_ns = current_time_ns();
    let service = graphql_intent_lifecycle_service();

    let mut guard = handler.lock().await;
    let stored_intent = guard
        .storage_ref()
        .get_mutation_intent(&scope.tenant_id, &scope.database_id, &token_intent_id)
        .map_err(axon_error_to_gql)?
        .ok_or_else(|| {
            mutation_intent_commit_error_to_gql(MutationIntentCommitValidationError::Token(
                MutationIntentTokenLookupError::NotFound,
            ))
        })?;
    let operation = supplied_operation.unwrap_or_else(|| stored_intent.operation.clone());
    let operation_payload = canonical_operation_payload(&operation)?;
    let schema_version = schema_version_for_intent_operation(
        &guard,
        &operation.operation_kind,
        operation_payload,
        0,
    )?;
    let current = MutationIntentCommitValidationContext {
        subject: stored_intent.subject.clone(),
        schema_version,
        policy_version: schema_version,
        operation_hash: operation.operation_hash.clone(),
        caller_authorized: caller.check(Operation::Write).is_ok(),
    };
    {
        let (storage, audit) = guard.storage_and_audit_mut();
        service
            .validate_commit_bindings_with_audit(
                storage,
                audit,
                MutationIntentCommitValidationAuditRequest {
                    scope: &scope,
                    token: &token,
                    current: &current,
                    now_ns,
                    actor: Some(&caller.actor),
                },
            )
            .map_err(mutation_intent_commit_error_to_gql)?;
    }
    let transaction = transaction_from_intent_operation(&guard, &operation)?;
    let (storage, audit) = guard.storage_and_audit_mut();
    let result = service
        .commit_transaction_intent(
            storage,
            audit,
            MutationIntentTransactionCommitRequest {
                scope,
                token,
                transaction,
                canonical_operation: Some(operation),
                current,
                now_ns,
                actor: Some(caller.actor.clone()),
                attribution: None,
            },
        )
        .map_err(mutation_intent_commit_error_to_gql)?;

    Ok(Some(json_to_field_value(json!({
        "committed": true,
        "intent": mutation_intent_json(&result.intent),
        "transactionId": result.transaction_id,
        "auditEntry": Value::Null,
        "stale": [],
        "errorCode": Value::Null,
    }))))
}

fn canonical_operation_from_input(value: &Value) -> Result<CanonicalOperationMetadata, GqlError> {
    let operation_obj = input_object(value, "operation")?;
    let operation_kind =
        parse_mutation_operation_kind(input_string(operation_obj, "operationKind")?)?;
    let operation_payload = operation_obj
        .get("operation")
        .cloned()
        .ok_or_else(|| GqlError::new("operation.operation is required"))?;
    let canonical_operation = canonicalize_intent_operation(operation_kind, operation_payload);
    if let Some(expected_hash) = operation_obj.get("operationHash").and_then(Value::as_str) {
        if expected_hash != canonical_operation.operation_hash {
            return Err(
                GqlError::new("operationHash does not match canonical operation").extend_with(
                    |_err, ext| {
                        ext.set("code", "intent_mismatch");
                    },
                ),
            );
        }
    }
    Ok(canonical_operation)
}

fn canonical_operation_payload(operation: &CanonicalOperationMetadata) -> Result<&Value, GqlError> {
    operation.canonical_operation.as_ref().ok_or_else(|| {
        GqlError::new("canonical operation payload is required").extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })
    })
}

async fn preview_mutation_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    let input = gql_input_to_json(ctx.args.try_get("input")?.as_value())?;
    let input_obj = input_object(&input, "input")?;
    let operation_input = input_obj
        .get("operation")
        .ok_or_else(|| GqlError::new("operation is required"))?;
    let canonical_operation = canonical_operation_from_input(operation_input)?;
    let operation_kind = canonical_operation.operation_kind.clone();
    let operation_payload = canonical_operation
        .canonical_operation
        .clone()
        .unwrap_or(Value::Null);

    let expires_in_seconds = input_obj
        .get("expiresInSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(3600);
    let subject = mutation_intent_subject_from_input(input_obj.get("subject"), &caller);
    let scope = graphql_intent_scope(&ctx);
    let now_ns = current_time_ns();
    let expires_at = now_ns.saturating_add(expires_in_seconds.saturating_mul(1_000_000_000));

    let mut guard = handler.lock().await;
    let preview = mutation_preview_computation(&guard, &operation_kind, &operation_payload)?;
    let policy = guard
        .explain_policy_with_caller(preview.explain_request, &caller, None)
        .map_err(axon_error_to_gql)?;
    let decision = mutation_intent_decision(&policy.decision);
    let approval_route = policy
        .approval
        .as_ref()
        .map(mutation_approval_route_from_policy);
    let policy_version = policy.policy_version.max(preview.schema_version);
    let policy_explanation = mutation_preview_policy_lines(&policy);
    let review_summary = MutationReviewSummary {
        title: Some(format!(
            "{} preview",
            operation_kind_label(&canonical_operation.operation_kind)
        )),
        summary: policy.reason.clone(),
        risk: (decision == MutationIntentDecision::NeedsApproval).then(|| "needs_approval".into()),
        affected_records: preview.pre_images.clone(),
        affected_fields: preview.affected_fields.clone(),
        diff: preview.diff.clone(),
        policy_explanation,
    };
    let intent = MutationIntent {
        intent_id: next_graphql_intent_id(now_ns),
        scope: scope.clone(),
        subject,
        schema_version: preview.schema_version.max(policy_version),
        policy_version,
        operation: canonical_operation.clone(),
        pre_images: preview.pre_images.clone(),
        decision: decision.clone(),
        approval_state: preview_state_for_decision(&decision),
        approval_route: approval_route.clone(),
        expires_at,
        review_summary,
    };
    let service = graphql_intent_lifecycle_service();
    let record = service
        .create_preview_record(guard.storage_mut(), intent)
        .map_err(mutation_intent_lifecycle_error_to_gql)?;
    let intent_json = mutation_intent_json(&record.intent);
    let decision_name = record.intent.decision.as_str();
    let policy_lines = record.intent.review_summary.policy_explanation.clone();
    let result = json!({
        "decision": decision_name,
        "intent": intent_json,
        "intentToken": record.intent_token.map(|token| token.as_str().to_string()),
        "canonicalOperation": canonical_operation_json(&canonical_operation),
        "diff": preview.diff,
        "affectedRecords": preview.pre_images.iter().map(pre_image_json).collect::<Vec<_>>(),
        "affectedFields": preview.affected_fields,
        "approvalRoute": approval_route.as_ref().map(mutation_approval_route_json),
        "policyExplanation": policy_lines,
    });
    Ok(Some(json_to_field_value(result)))
}

fn mutation_preview_computation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation_kind: &MutationOperationKind,
    operation: &Value,
) -> Result<MutationPreviewComputation, GqlError> {
    let obj = input_object(operation, "operation")?;
    match operation_kind {
        MutationOperationKind::CreateEntity => preview_create_entity(handler, obj),
        MutationOperationKind::UpdateEntity => preview_update_entity(handler, obj),
        MutationOperationKind::PatchEntity => preview_patch_entity(handler, obj),
        MutationOperationKind::DeleteEntity => preview_delete_entity(handler, obj),
        MutationOperationKind::CreateLink => preview_create_link(handler, obj),
        MutationOperationKind::DeleteLink => preview_delete_link(handler, obj),
        MutationOperationKind::Transition => preview_transition(handler, obj),
        MutationOperationKind::Rollback => preview_rollback(handler, obj),
        MutationOperationKind::Revert => preview_revert(handler, obj),
        MutationOperationKind::Transaction => preview_transaction(handler, obj),
    }
}

fn preview_create_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", 0)?);
    let id = EntityId::new(required_str(operation, "id", 0)?);
    let data = operation.get("data").cloned().unwrap_or(Value::Null);
    let schema = required_schema(handler, &collection)?;
    validate(&schema, &data).map_err(axon_error_to_gql)?;
    if handler
        .storage_ref()
        .get(&collection, &id)
        .map_err(axon_error_to_gql)?
        .is_some()
    {
        return Err(axon_error_to_gql(AxonError::AlreadyExists(format!(
            "{}/{}",
            collection, id
        ))));
    }
    let diff = diff_value(&json!({}), &data);
    let mut request = empty_explain_policy_request("create");
    request.collection = Some(collection);
    request.entity_id = Some(id);
    request.data = Some(data);
    Ok(preview_result(request, Vec::new(), diff, schema.version))
}

fn preview_update_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", 0)?);
    let id = EntityId::new(required_str(operation, "id", 0)?);
    let expected_version = operation.get("expected_version").and_then(Value::as_u64);
    let data = operation.get("data").cloned().unwrap_or(Value::Null);
    let schema = required_schema(handler, &collection)?;
    validate(&schema, &data).map_err(axon_error_to_gql)?;
    let current = required_entity(handler, &collection, &id)?;
    check_expected_version(&current, expected_version)?;
    let diff = diff_value(&current.data, &data);
    let mut request = empty_explain_policy_request("update");
    request.collection = Some(collection);
    request.entity_id = Some(id);
    request.expected_version = expected_version;
    request.data = Some(data);
    Ok(preview_result(
        request,
        vec![entity_pre_image(&current)],
        diff,
        schema.version,
    ))
}

fn preview_patch_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", 0)?);
    let id = EntityId::new(required_str(operation, "id", 0)?);
    let expected_version = operation.get("expected_version").and_then(Value::as_u64);
    let patch = operation.get("patch").cloned().unwrap_or(Value::Null);
    let schema = required_schema(handler, &collection)?;
    let current = required_entity(handler, &collection, &id)?;
    check_expected_version(&current, expected_version)?;
    let mut merged = current.data.clone();
    json_merge_patch(&mut merged, &patch);
    validate(&schema, &merged).map_err(axon_error_to_gql)?;
    let diff = diff_value(&current.data, &merged);
    let mut request = empty_explain_policy_request("patch");
    request.collection = Some(collection);
    request.entity_id = Some(id);
    request.expected_version = expected_version;
    request.patch = Some(patch);
    Ok(preview_result(
        request,
        vec![entity_pre_image(&current)],
        diff,
        schema.version,
    ))
}

fn preview_delete_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", 0)?);
    let id = EntityId::new(required_str(operation, "id", 0)?);
    let expected_version = operation.get("expected_version").and_then(Value::as_u64);
    let schema = required_schema(handler, &collection)?;
    let current = required_entity(handler, &collection, &id)?;
    check_expected_version(&current, expected_version)?;
    let diff = diff_value(&current.data, &json!({}));
    let mut request = empty_explain_policy_request("delete");
    request.collection = Some(collection);
    request.entity_id = Some(id);
    request.expected_version = expected_version;
    Ok(preview_result(
        request,
        vec![entity_pre_image(&current)],
        diff,
        schema.version,
    ))
}

fn preview_transition<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", 0)?);
    let id = EntityId::new(required_str(operation, "id", 0)?);
    let lifecycle_name = input_string(operation, "lifecycle_name")?;
    let target_state = input_string(operation, "target_state")?;
    let expected_version = operation.get("expected_version").and_then(Value::as_u64);
    let schema = required_schema(handler, &collection)?;
    let current = required_entity(handler, &collection, &id)?;
    check_expected_version(&current, expected_version)?;
    let lifecycle = schema.lifecycles.get(&lifecycle_name).ok_or_else(|| {
        axon_error_to_gql(AxonError::LifecycleNotFound {
            lifecycle_name: lifecycle_name.clone(),
        })
    })?;
    let mut candidate = current.data.clone();
    candidate[&lifecycle.field] = Value::String(target_state.clone());
    validate(&schema, &candidate).map_err(axon_error_to_gql)?;
    let diff = diff_value(&current.data, &candidate);
    let mut request = empty_explain_policy_request("transition");
    request.collection = Some(collection);
    request.entity_id = Some(id);
    request.expected_version = expected_version;
    request.lifecycle_name = Some(lifecycle_name);
    request.target_state = Some(target_state);
    Ok(preview_result(
        request,
        vec![entity_pre_image(&current)],
        diff,
        schema.version,
    ))
}

fn preview_create_link<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let link = link_from_operation(operation)?;
    let source = required_entity(handler, &link.source_collection, &link.source_id)?;
    let target = required_entity(handler, &link.target_collection, &link.target_id)?;
    let schema_version = max_schema_version_for_entities(handler, &[&source, &target])?;
    let link_id = Link::storage_id(
        &link.source_collection,
        &link.source_id,
        &link.link_type,
        &link.target_collection,
        &link.target_id,
    );
    if handler
        .storage_ref()
        .get(&Link::links_collection(), &link_id)
        .map_err(axon_error_to_gql)?
        .is_some()
    {
        return Err(axon_error_to_gql(AxonError::AlreadyExists(format!(
            "link {link_id}"
        ))));
    }
    let mut request = empty_explain_policy_request("create_link");
    request.collection = Some(link.source_collection.clone());
    request.entity_id = Some(link.source_id.clone());
    Ok(preview_result(
        request,
        vec![entity_pre_image(&source), entity_pre_image(&target)],
        diff_value(&Value::Null, &link.to_entity().data),
        schema_version,
    ))
}

fn preview_delete_link<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let link = link_from_operation(operation)?;
    let link_id = Link::storage_id(
        &link.source_collection,
        &link.source_id,
        &link.link_type,
        &link.target_collection,
        &link.target_id,
    );
    let link_entity = handler
        .storage_ref()
        .get(&Link::links_collection(), &link_id)
        .map_err(axon_error_to_gql)?
        .ok_or_else(|| axon_error_to_gql(AxonError::NotFound(format!("link {link_id}"))))?;
    let source = required_entity(handler, &link.source_collection, &link.source_id)?;
    let target = required_entity(handler, &link.target_collection, &link.target_id)?;
    let schema_version = max_schema_version_for_entities(handler, &[&source, &target])?;
    let mut request = empty_explain_policy_request("delete_link");
    request.collection = Some(link.source_collection);
    request.entity_id = Some(link.source_id);
    let pre_images = vec![
        PreImageBinding::Link {
            collection: Link::links_collection(),
            id: LinkId::new(link_id.to_string()),
            version: link_entity.version,
        },
        entity_pre_image(&source),
        entity_pre_image(&target),
    ];
    Ok(preview_result(
        request,
        pre_images,
        diff_value(&link_entity.data, &Value::Null),
        schema_version,
    ))
}

fn preview_rollback<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let scope = operation
        .get("rollback_scope")
        .and_then(Value::as_str)
        .unwrap_or("entity");
    if scope != "entity" {
        let mut request = empty_explain_policy_request("rollback");
        request.collection = operation
            .get("collection")
            .and_then(Value::as_str)
            .map(CollectionId::new);
        return Ok(preview_result(request, Vec::new(), json!({}), 0));
    }
    let collection = CollectionId::new(required_str(operation, "collection", 0)?);
    let id = EntityId::new(required_str(operation, "id", 0)?);
    let target = operation
        .get("target")
        .and_then(Value::as_object)
        .ok_or_else(|| GqlError::new("rollback target is required"))?;
    let to_version = target
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| GqlError::new("rollback target.version is required"))?;
    let schema = required_schema(handler, &collection)?;
    let current = handler
        .storage_ref()
        .get(&collection, &id)
        .map_err(axon_error_to_gql)?;
    let source = handler
        .audit_log()
        .query_paginated(axon_audit::log::AuditQuery {
            collection: Some(collection.clone()),
            entity_id: Some(id.clone()),
            until_ns: None,
            ..axon_audit::log::AuditQuery::default()
        })
        .map_err(axon_error_to_gql)?
        .entries
        .into_iter()
        .find(|entry| entry.version == to_version)
        .ok_or_else(|| {
            axon_error_to_gql(AxonError::NotFound(format!(
                "entity version {to_version} not found in audit log for {id}"
            )))
        })?;
    let target_data = source
        .data_after
        .clone()
        .ok_or_else(|| axon_error_to_gql(AxonError::NotFound(format!("rollback target {id}"))))?;
    validate(&schema, &target_data).map_err(axon_error_to_gql)?;
    let diff = current.as_ref().map_or_else(
        || diff_value(&json!({}), &target_data),
        |entity| diff_value(&entity.data, &target_data),
    );
    let mut request = empty_explain_policy_request("rollback");
    request.collection = Some(collection);
    request.entity_id = Some(id);
    request.to_version = Some(to_version);
    let pre_images = current.as_ref().map(entity_pre_image).into_iter().collect();
    Ok(preview_result(request, pre_images, diff, schema.version))
}

fn preview_revert<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let audit_entry_id = operation
        .get("audit_entry_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| GqlError::new("audit_entry_id is required"))?;
    let entry = handler
        .audit_log()
        .find_by_id(audit_entry_id)
        .map_err(axon_error_to_gql)?
        .ok_or_else(|| {
            axon_error_to_gql(AxonError::NotFound(format!("audit entry {audit_entry_id}")))
        })?;
    let target_data = entry.data_before.clone().ok_or_else(|| {
        axon_error_to_gql(AxonError::InvalidOperation(
            "audit entry has no before state".into(),
        ))
    })?;
    let schema = required_schema(handler, &entry.collection)?;
    validate(&schema, &target_data).map_err(axon_error_to_gql)?;
    let current = handler
        .storage_ref()
        .get(&entry.collection, &entry.entity_id)
        .map_err(axon_error_to_gql)?;
    let diff = current.as_ref().map_or_else(
        || diff_value(&json!({}), &target_data),
        |entity| diff_value(&entity.data, &target_data),
    );
    let mut request = empty_explain_policy_request("update");
    request.collection = Some(entry.collection);
    request.entity_id = Some(entry.entity_id);
    request.data = Some(target_data);
    let pre_images = current.as_ref().map(entity_pre_image).into_iter().collect();
    Ok(preview_result(request, pre_images, diff, schema.version))
}

fn preview_transaction<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<MutationPreviewComputation, GqlError> {
    let operations = operation
        .get("operations")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut request = empty_explain_policy_request("transaction");
    request.operations = explain_transaction_operations_from_value(&operations)?;
    let mut pre_images = Vec::new();
    let mut diffs = Vec::new();
    let mut schema_version = 0;
    for (index, operation) in operations.as_array().into_iter().flatten().enumerate() {
        let child = transaction_child_preview(handler, index, operation)?;
        schema_version = schema_version.max(child.schema_version);
        pre_images.extend(child.pre_images);
        diffs.push(json!({
            "operationIndex": index,
            "diff": child.diff,
        }));
    }
    Ok(preview_result(
        request,
        pre_images,
        json!(diffs),
        schema_version,
    ))
}

fn transaction_child_preview<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    index: usize,
    operation: &Value,
) -> Result<MutationPreviewComputation, GqlError> {
    let obj = required_object(operation, "operation", index)?;
    let variants: Vec<(&str, &Value)> = obj
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| (key.as_str(), value))
        .collect();
    if variants.len() != 1 {
        return Err(GqlError::new("operation must set exactly one variant"));
    }
    let (variant, payload) = variants[0];
    let payload = required_object(payload, variant, index)?;
    let normalized = transaction_payload_to_canonical_operation(variant, payload)?;
    let kind = match variant {
        "createEntity" => MutationOperationKind::CreateEntity,
        "updateEntity" => MutationOperationKind::UpdateEntity,
        "patchEntity" => MutationOperationKind::PatchEntity,
        "deleteEntity" => MutationOperationKind::DeleteEntity,
        "createLink" => MutationOperationKind::CreateLink,
        "deleteLink" => MutationOperationKind::DeleteLink,
        _ => return Err(GqlError::new("unsupported transaction operation")),
    };
    mutation_preview_computation(handler, &kind, &normalized)
}

fn transaction_payload_to_canonical_operation(
    variant: &str,
    payload: &serde_json::Map<String, Value>,
) -> Result<Value, GqlError> {
    let value = match variant {
        "createEntity" => json!({
            "collection": input_string(payload, "collection")?,
            "id": input_string(payload, "id")?,
            "data": payload.get("data").cloned().unwrap_or(Value::Null),
        }),
        "updateEntity" => json!({
            "collection": input_string(payload, "collection")?,
            "id": input_string(payload, "id")?,
            "data": payload.get("data").cloned().unwrap_or(Value::Null),
            "expected_version": payload.get("expectedVersion").and_then(Value::as_u64),
        }),
        "patchEntity" => json!({
            "collection": input_string(payload, "collection")?,
            "id": input_string(payload, "id")?,
            "patch": payload.get("patch").cloned().unwrap_or(Value::Null),
            "expected_version": payload.get("expectedVersion").and_then(Value::as_u64),
        }),
        "deleteEntity" => json!({
            "collection": input_string(payload, "collection")?,
            "id": input_string(payload, "id")?,
            "expected_version": payload.get("expectedVersion").and_then(Value::as_u64),
        }),
        "createLink" => json!({
            "source_collection": input_string(payload, "sourceCollection")?,
            "source_id": input_string(payload, "sourceId")?,
            "target_collection": input_string(payload, "targetCollection")?,
            "target_id": input_string(payload, "targetId")?,
            "link_type": input_string(payload, "linkType")?,
            "metadata": payload.get("metadata").cloned().unwrap_or(Value::Null),
        }),
        "deleteLink" => json!({
            "source_collection": input_string(payload, "sourceCollection")?,
            "source_id": input_string(payload, "sourceId")?,
            "target_collection": input_string(payload, "targetCollection")?,
            "target_id": input_string(payload, "targetId")?,
            "link_type": input_string(payload, "linkType")?,
        }),
        _ => {
            return Err(GqlError::new(format!(
                "unsupported transaction operation '{variant}'"
            )))
        }
    };
    Ok(value)
}

fn schema_version_for_intent_operation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    kind: &MutationOperationKind,
    operation: &Value,
    op_index: usize,
) -> Result<u32, GqlError> {
    let obj = required_object(operation, "operation", op_index)?;
    match kind {
        MutationOperationKind::CreateEntity
        | MutationOperationKind::UpdateEntity
        | MutationOperationKind::PatchEntity
        | MutationOperationKind::DeleteEntity
        | MutationOperationKind::Transition => {
            let collection = CollectionId::new(required_str(obj, "collection", op_index)?);
            Ok(required_schema(handler, &collection)?.version)
        }
        MutationOperationKind::CreateLink | MutationOperationKind::DeleteLink => {
            schema_version_for_link_operation(handler, obj)
        }
        MutationOperationKind::Transaction => {
            let operations = transaction_operations_array(obj, op_index)?;
            let mut schema_version = 0;
            for (index, child) in operations.iter().enumerate() {
                schema_version = schema_version
                    .max(schema_version_for_transaction_child(handler, index, child)?);
            }
            Ok(schema_version)
        }
        MutationOperationKind::Rollback | MutationOperationKind::Revert => {
            Err(unsupported_intent_commit(kind))
        }
    }
}

fn schema_version_for_transaction_child<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    index: usize,
    operation: &Value,
) -> Result<u32, GqlError> {
    let obj = required_object(operation, "operation", index)?;
    if let Some(op) = obj.get("op").and_then(Value::as_str) {
        let kind = parse_mutation_operation_kind(op.to_string())?;
        return schema_version_for_intent_operation(handler, &kind, operation, index);
    }

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
    let normalized = transaction_payload_to_canonical_operation(variant, payload)?;
    let kind = transaction_variant_kind(variant, index)?;
    schema_version_for_intent_operation(handler, &kind, &normalized, index)
}

fn schema_version_for_link_operation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &serde_json::Map<String, Value>,
) -> Result<u32, GqlError> {
    let link = link_from_operation(operation)?;
    let source_version = required_schema(handler, &link.source_collection)?.version;
    let target_version = required_schema(handler, &link.target_collection)?.version;
    Ok(source_version.max(target_version))
}

fn transaction_from_intent_operation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &CanonicalOperationMetadata,
) -> Result<Transaction, GqlError> {
    let mut transaction = Transaction::new();
    let payload = canonical_operation_payload(operation)?;
    stage_intent_operation(
        handler,
        &mut transaction,
        &operation.operation_kind,
        payload,
        0,
    )?;
    Ok(transaction)
}

fn stage_intent_operation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    kind: &MutationOperationKind,
    operation: &Value,
    op_index: usize,
) -> Result<(), GqlError> {
    let obj = required_object(operation, "operation", op_index)?;
    match kind {
        MutationOperationKind::CreateEntity => stage_create_entity(transaction, obj, op_index),
        MutationOperationKind::UpdateEntity => {
            stage_update_entity(handler, transaction, obj, op_index)
        }
        MutationOperationKind::PatchEntity => {
            stage_patch_entity(handler, transaction, obj, op_index)
        }
        MutationOperationKind::DeleteEntity => {
            stage_delete_entity(handler, transaction, obj, op_index)
        }
        MutationOperationKind::CreateLink => stage_create_link(transaction, obj, op_index),
        MutationOperationKind::DeleteLink => stage_delete_link(transaction, obj, op_index),
        MutationOperationKind::Transition => {
            stage_transition_entity(handler, transaction, obj, op_index)
        }
        MutationOperationKind::Transaction => stage_transaction(handler, transaction, obj),
        MutationOperationKind::Rollback | MutationOperationKind::Revert => {
            Err(unsupported_intent_commit(kind))
        }
    }
}

fn stage_create_entity(
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<(), GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", op_index)?);
    let id = EntityId::new(required_str(operation, "id", op_index)?);
    let data = operation.get("data").cloned().unwrap_or(Value::Null);
    transaction
        .create(Entity::new(collection, id, data))
        .map_err(|error| op_error(axon_error_to_gql(error), op_index))
}

fn stage_update_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<(), GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", op_index)?);
    let id = EntityId::new(required_str(operation, "id", op_index)?);
    let data = operation.get("data").cloned().unwrap_or(Value::Null);
    let schema = required_schema(handler, &collection)?;
    validate(&schema, &data).map_err(axon_error_to_gql)?;
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    transaction
        .update(
            Entity::new(collection, id, data),
            expected_version,
            Some(current.data),
        )
        .map_err(|error| op_error(axon_error_to_gql(error), op_index))
}

fn stage_patch_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<(), GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", op_index)?);
    let id = EntityId::new(required_str(operation, "id", op_index)?);
    let patch = operation.get("patch").cloned().unwrap_or(Value::Null);
    let schema = required_schema(handler, &collection)?;
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    let mut merged = current.data.clone();
    json_merge_patch(&mut merged, &patch);
    validate(&schema, &merged).map_err(axon_error_to_gql)?;
    transaction
        .update(
            Entity::new(collection, id, merged),
            expected_version,
            Some(current.data),
        )
        .map_err(|error| op_error(axon_error_to_gql(error), op_index))
}

fn stage_delete_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<(), GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", op_index)?);
    let id = EntityId::new(required_str(operation, "id", op_index)?);
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    transaction
        .delete(collection, id, expected_version, Some(current.data))
        .map_err(|error| op_error(axon_error_to_gql(error), op_index))
}

fn stage_transition_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<(), GqlError> {
    let collection = CollectionId::new(required_str(operation, "collection", op_index)?);
    let id = EntityId::new(required_str(operation, "id", op_index)?);
    let lifecycle_name = input_string(operation, "lifecycle_name")?;
    let target_state = input_string(operation, "target_state")?;
    let schema = required_schema(handler, &collection)?;
    let lifecycle = schema.lifecycles.get(&lifecycle_name).ok_or_else(|| {
        axon_error_to_gql(AxonError::LifecycleNotFound {
            lifecycle_name: lifecycle_name.clone(),
        })
    })?;
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    let mut candidate = current.data.clone();
    candidate[&lifecycle.field] = Value::String(target_state);
    validate(&schema, &candidate).map_err(axon_error_to_gql)?;
    transaction
        .update(
            Entity::new(collection, id, candidate),
            expected_version,
            Some(current.data),
        )
        .map_err(|error| op_error(axon_error_to_gql(error), op_index))
}

fn stage_create_link(
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<(), GqlError> {
    transaction
        .create_link(link_from_operation(operation)?)
        .map_err(|error| op_error(axon_error_to_gql(error), op_index))
}

fn stage_delete_link(
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<(), GqlError> {
    transaction
        .delete_link(link_from_operation(operation)?)
        .map_err(|error| op_error(axon_error_to_gql(error), op_index))
}

fn stage_transaction<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), GqlError> {
    for (index, child) in transaction_operations_array(operation, 0)?
        .iter()
        .enumerate()
    {
        stage_transaction_child(handler, transaction, index, child)?;
    }
    Ok(())
}

fn stage_transaction_child<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    index: usize,
    operation: &Value,
) -> Result<(), GqlError> {
    let obj = required_object(operation, "operation", index)?;
    if let Some(op) = obj.get("op").and_then(Value::as_str) {
        let kind = parse_mutation_operation_kind(op.to_string())?;
        return stage_intent_operation(handler, transaction, &kind, operation, index);
    }

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
    let normalized = transaction_payload_to_canonical_operation(variant, payload)?;
    let kind = transaction_variant_kind(variant, index)?;
    stage_intent_operation(handler, transaction, &kind, &normalized, index)
}

fn transaction_operations_array(
    operation: &serde_json::Map<String, Value>,
    op_index: usize,
) -> Result<&Vec<Value>, GqlError> {
    operation
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            GqlError::new("operations must be a list").extend_with(move |_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
                ext.set("operationIndex", op_index as i32);
            })
        })
}

fn transaction_variant_kind(
    variant: &str,
    op_index: usize,
) -> Result<MutationOperationKind, GqlError> {
    match variant {
        "createEntity" => Ok(MutationOperationKind::CreateEntity),
        "updateEntity" => Ok(MutationOperationKind::UpdateEntity),
        "patchEntity" => Ok(MutationOperationKind::PatchEntity),
        "deleteEntity" => Ok(MutationOperationKind::DeleteEntity),
        "createLink" => Ok(MutationOperationKind::CreateLink),
        "deleteLink" => Ok(MutationOperationKind::DeleteLink),
        other => Err(
            GqlError::new(format!("unsupported transaction operation '{other}'")).extend_with(
                move |_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                    ext.set("operationIndex", op_index as i32);
                },
            ),
        ),
    }
}

fn unsupported_intent_commit(kind: &MutationOperationKind) -> GqlError {
    GqlError::new(format!(
        "commitMutationIntent does not support {} operations",
        operation_kind_label(kind)
    ))
    .extend_with(|_err, ext| {
        ext.set("code", "INVALID_ARGUMENT");
    })
}

fn preview_result(
    explain_request: ExplainPolicyRequest,
    pre_images: Vec<PreImageBinding>,
    diff: Value,
    schema_version: u32,
) -> MutationPreviewComputation {
    let affected_fields = affected_fields_from_diff(&diff);
    MutationPreviewComputation {
        explain_request,
        pre_images,
        diff,
        affected_fields,
        schema_version,
    }
}

fn required_schema<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &CollectionId,
) -> Result<CollectionSchema, GqlError> {
    handler
        .get_schema(collection)
        .map_err(axon_error_to_gql)?
        .ok_or_else(|| axon_error_to_gql(AxonError::NotFound(collection.to_string())))
}

fn max_schema_version_for_entities<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    entities: &[&Entity],
) -> Result<u32, GqlError> {
    let mut schema_version = 0;
    for entity in entities {
        schema_version = schema_version.max(required_schema(handler, &entity.collection)?.version);
    }
    Ok(schema_version)
}

fn required_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &CollectionId,
    id: &EntityId,
) -> Result<Entity, GqlError> {
    handler
        .storage_ref()
        .get(collection, id)
        .map_err(axon_error_to_gql)?
        .ok_or_else(|| axon_error_to_gql(AxonError::NotFound(id.to_string())))
}

fn check_expected_version(entity: &Entity, expected_version: Option<u64>) -> Result<(), GqlError> {
    if let Some(expected) = expected_version {
        if entity.version != expected {
            return Err(axon_error_to_gql(AxonError::ConflictingVersion {
                expected,
                actual: entity.version,
                current_entity: Some(Box::new(entity.clone())),
            }));
        }
    }
    Ok(())
}

fn entity_pre_image(entity: &Entity) -> PreImageBinding {
    PreImageBinding::Entity {
        collection: entity.collection.clone(),
        id: entity.id.clone(),
        version: entity.version,
    }
}

fn link_from_operation(operation: &serde_json::Map<String, Value>) -> Result<Link, GqlError> {
    Ok(Link {
        source_collection: CollectionId::new(input_string(operation, "source_collection")?),
        source_id: EntityId::new(input_string(operation, "source_id")?),
        target_collection: CollectionId::new(input_string(operation, "target_collection")?),
        target_id: EntityId::new(input_string(operation, "target_id")?),
        link_type: input_string(operation, "link_type")?,
        metadata: operation.get("metadata").cloned().unwrap_or(Value::Null),
    })
}

fn diff_value(before: &Value, after: &Value) -> Value {
    serde_json::to_value(compute_diff(before, after)).unwrap_or_else(|_| json!({}))
}

fn affected_fields_from_diff(diff: &Value) -> Vec<String> {
    let mut fields = match diff {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(items) => items
            .iter()
            .flat_map(|item| {
                item.get("diff")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(|map| map.keys().cloned())
            })
            .collect(),
        _ => Vec::new(),
    };
    fields.sort();
    fields.dedup();
    fields
}

fn parse_mutation_operation_kind(value: String) -> Result<MutationOperationKind, GqlError> {
    match value.trim() {
        "create_entity" | "createEntity" | "create" => Ok(MutationOperationKind::CreateEntity),
        "update_entity" | "updateEntity" | "update" => Ok(MutationOperationKind::UpdateEntity),
        "patch_entity" | "patchEntity" | "patch" => Ok(MutationOperationKind::PatchEntity),
        "delete_entity" | "deleteEntity" | "delete" => Ok(MutationOperationKind::DeleteEntity),
        "create_link" | "createLink" => Ok(MutationOperationKind::CreateLink),
        "delete_link" | "deleteLink" => Ok(MutationOperationKind::DeleteLink),
        "transaction" => Ok(MutationOperationKind::Transaction),
        "transition" => Ok(MutationOperationKind::Transition),
        "rollback" => Ok(MutationOperationKind::Rollback),
        "revert" => Ok(MutationOperationKind::Revert),
        other => Err(
            GqlError::new(format!("unsupported mutation operation kind '{other}'")).extend_with(
                |_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                },
            ),
        ),
    }
}

fn operation_kind_label(kind: &MutationOperationKind) -> &'static str {
    match kind {
        MutationOperationKind::CreateEntity => "create_entity",
        MutationOperationKind::UpdateEntity => "update_entity",
        MutationOperationKind::PatchEntity => "patch_entity",
        MutationOperationKind::DeleteEntity => "delete_entity",
        MutationOperationKind::CreateLink => "create_link",
        MutationOperationKind::DeleteLink => "delete_link",
        MutationOperationKind::Transaction => "transaction",
        MutationOperationKind::Transition => "transition",
        MutationOperationKind::Rollback => "rollback",
        MutationOperationKind::Revert => "revert",
    }
}

fn mutation_intent_decision(decision: &str) -> MutationIntentDecision {
    match decision {
        "deny" => MutationIntentDecision::Deny,
        "needs_approval" => MutationIntentDecision::NeedsApproval,
        _ => MutationIntentDecision::Allow,
    }
}

fn preview_state_for_decision(decision: &MutationIntentDecision) -> ApprovalState {
    match decision {
        MutationIntentDecision::NeedsApproval => ApprovalState::Pending,
        MutationIntentDecision::Allow | MutationIntentDecision::Deny => ApprovalState::None,
    }
}

fn mutation_approval_route_from_policy(
    approval: &axon_api::response::PolicyApprovalEnvelopeSummary,
) -> MutationApprovalRoute {
    MutationApprovalRoute {
        role: approval.role.clone(),
        reason_required: approval.reason_required,
        deadline_seconds: approval.deadline_seconds,
        separation_of_duties: approval.separation_of_duties,
    }
}

fn mutation_preview_policy_lines(
    policy: &axon_api::response::PolicyExplanationResponse,
) -> Vec<String> {
    let mut lines = vec![format!("{}: {}", policy.decision, policy.reason)];
    lines.extend(policy.policy_ids.iter().map(|id| format!("policy:{id}")));
    lines.extend(policy.rule_ids.iter().map(|id| format!("rule:{id}")));
    for child in &policy.operations {
        lines.push(format!(
            "operation[{}]: {}: {}",
            child.operation_index.unwrap_or(0),
            child.decision,
            child.reason
        ));
    }
    lines
}

fn mutation_intent_subject_from_input(
    subject: Option<&Value>,
    caller: &CallerIdentity,
) -> MutationIntentSubjectBinding {
    let obj = subject.and_then(Value::as_object);
    let mut attributes = HashMap::new();
    if let Some(Value::Object(input_attributes)) = obj.and_then(|obj| obj.get("attributes")) {
        attributes.extend(
            input_attributes
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    MutationIntentSubjectBinding {
        user_id: string_member(obj, "userId")
            .or_else(|| string_member(obj, "user_id"))
            .or_else(|| Some(caller.actor.clone())),
        agent_id: string_member(obj, "agentId").or_else(|| string_member(obj, "agent_id")),
        delegated_by: string_member(obj, "delegatedBy")
            .or_else(|| string_member(obj, "delegated_by")),
        tenant_role: string_member(obj, "tenantRole")
            .or_else(|| string_member(obj, "tenant_role"))
            .or_else(|| Some(format!("{:?}", caller.role).to_ascii_lowercase())),
        credential_id: string_member(obj, "credentialId")
            .or_else(|| string_member(obj, "credential_id")),
        grant_version: obj
            .and_then(|obj| obj.get("grantVersion"))
            .or_else(|| obj.and_then(|obj| obj.get("grant_version")))
            .and_then(Value::as_u64),
        attributes,
    }
}

fn string_member(obj: Option<&serde_json::Map<String, Value>>, key: &str) -> Option<String> {
    obj.and_then(|obj| obj.get(key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn mutation_intent_query_filter(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
) -> Result<MutationIntentQueryFilter, GqlError> {
    let filter = match ctx.args.try_get("filter") {
        Ok(value) if value.is_null() => Value::Object(serde_json::Map::new()),
        Ok(value) => gql_input_to_json(value.as_value())?,
        Err(_) => Value::Object(serde_json::Map::new()),
    };
    let obj = input_object(&filter, "filter")?;
    let mut states = Vec::new();
    if let Some(status) = obj.get("status").and_then(Value::as_str) {
        push_intent_state_filter(&mut states, status)?;
    }
    if let Some(statuses) = obj.get("statuses") {
        let statuses = statuses.as_array().ok_or_else(|| {
            GqlError::new("statuses must be a list").extend_with(|_err, ext| {
                ext.set("code", "INVALID_ARGUMENT");
            })
        })?;
        for status_value in statuses {
            let status_name = status_value.as_str().ok_or_else(|| {
                GqlError::new("statuses entries must be strings").extend_with(|_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                })
            })?;
            push_intent_state_filter(&mut states, status_name)?;
        }
    }
    let decision = obj
        .get("decision")
        .and_then(Value::as_str)
        .map(parse_mutation_intent_decision_filter)
        .transpose()?;
    Ok(MutationIntentQueryFilter {
        states,
        decision,
        include_expired: obj
            .get("includeExpired")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn push_intent_state_filter(states: &mut Vec<ApprovalState>, value: &str) -> Result<(), GqlError> {
    let expanded = match value.trim() {
        "history" => vec![
            ApprovalState::Approved,
            ApprovalState::Rejected,
            ApprovalState::Expired,
            ApprovalState::Committed,
        ],
        "all" => vec![
            ApprovalState::None,
            ApprovalState::Pending,
            ApprovalState::Approved,
            ApprovalState::Rejected,
            ApprovalState::Expired,
            ApprovalState::Committed,
        ],
        other => vec![parse_approval_state_filter(other)?],
    };
    for state in expanded {
        if !states.iter().any(|existing| existing == &state) {
            states.push(state);
        }
    }
    Ok(())
}

fn parse_approval_state_filter(value: &str) -> Result<ApprovalState, GqlError> {
    match value.trim() {
        "none" | "allowed" | "allow" => Ok(ApprovalState::None),
        "pending" => Ok(ApprovalState::Pending),
        "approved" => Ok(ApprovalState::Approved),
        "rejected" => Ok(ApprovalState::Rejected),
        "expired" => Ok(ApprovalState::Expired),
        "committed" => Ok(ApprovalState::Committed),
        other => Err(
            GqlError::new(format!("unsupported mutation intent status '{other}'")).extend_with(
                |_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                },
            ),
        ),
    }
}

fn parse_mutation_intent_decision_filter(value: &str) -> Result<MutationIntentDecision, GqlError> {
    match value.trim() {
        "allow" | "allowed" => Ok(MutationIntentDecision::Allow),
        "needs_approval" | "needsApproval" => Ok(MutationIntentDecision::NeedsApproval),
        "deny" | "denied" => Ok(MutationIntentDecision::Deny),
        other => Err(
            GqlError::new(format!("unsupported mutation intent decision '{other}'")).extend_with(
                |_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                },
            ),
        ),
    }
}

fn parse_optional_limit_arg(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
    name: &str,
) -> Result<Option<usize>, GqlError> {
    match ctx.args.try_get(name) {
        Ok(value) if value.is_null() => Ok(None),
        Ok(value) => {
            let limit = value.i64()?;
            if limit < 0 {
                return Err(
                    GqlError::new(format!("{name} must be non-negative")).extend_with(
                        |_err, ext| {
                            ext.set("code", "INVALID_ARGUMENT");
                        },
                    ),
                );
            }
            Ok(Some(limit as usize))
        }
        Err(_) => Ok(None),
    }
}

fn sort_mutation_intents(intents: &mut [MutationIntent]) {
    intents.sort_by(|left, right| {
        left.expires_at
            .cmp(&right.expires_at)
            .then_with(|| left.intent_id.cmp(&right.intent_id))
    });
}

fn paginate_mutation_intents(
    intents: Vec<MutationIntent>,
    after: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<MutationIntent>, bool, bool), GqlError> {
    let start_index = match after {
        Some(cursor) => intents
            .iter()
            .position(|intent| intent.intent_id == cursor)
            .map(|index| index + 1)
            .ok_or_else(|| {
                GqlError::new("after cursor was not found").extend_with(|_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                })
            })?,
        None => 0,
    };
    let end_index = limit
        .map(|limit| start_index.saturating_add(limit))
        .unwrap_or(intents.len())
        .min(intents.len());
    let has_previous_page = start_index > 0;
    let has_next_page = end_index < intents.len();
    Ok((
        intents[start_index..end_index].to_vec(),
        has_previous_page,
        has_next_page,
    ))
}

fn mutation_intent_connection_value(
    intents: &[MutationIntent],
    total_count: usize,
    has_next_page: bool,
    has_previous_page: bool,
) -> FieldValue<'static> {
    let edges: Vec<Value> = intents
        .iter()
        .map(|intent| {
            json!({
                "cursor": intent.intent_id,
                "node": mutation_intent_json(intent),
            })
        })
        .collect();
    json_to_field_value(json!({
        "edges": edges,
        "pageInfo": page_info_json(
            intents.first().map(|intent| intent.intent_id.clone()),
            intents.last().map(|intent| intent.intent_id.clone()),
            has_next_page,
            has_previous_page,
        ),
        "totalCount": total_count,
    }))
}

fn graphql_intent_lifecycle_service() -> MutationIntentLifecycleService {
    MutationIntentLifecycleService::new(graphql_intent_token_signer())
}

fn graphql_intent_token_signer() -> MutationIntentTokenSigner {
    MutationIntentTokenSigner::new(GRAPHQL_INTENT_TOKEN_SECRET.to_vec())
}

fn graphql_intent_scope(
    ctx: &async_graphql::dynamic::ResolverContext<'_>,
) -> MutationIntentScopeBinding {
    let scope = ctx
        .data::<GraphqlIdempotencyScope>()
        .map(|scope| scope.0.clone())
        .unwrap_or_else(|_| "default:default".into());
    let (tenant_id, database_id) = scope
        .split_once(':')
        .map(|(tenant, database)| (tenant.to_string(), database.to_string()))
        .unwrap_or_else(|| ("default".into(), "default".into()));
    MutationIntentScopeBinding {
        tenant_id,
        database_id,
    }
}

fn current_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn next_graphql_intent_id(now_ns: u64) -> String {
    let sequence = GRAPHQL_INTENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("mint_gql_{now_ns}_{sequence}")
}

fn mutation_intent_lifecycle_error_to_gql(
    error: axon_api::intent::MutationIntentLifecycleError,
) -> GqlError {
    GqlError::new(error.to_string()).extend_with(move |_err, ext| {
        ext.set("code", error.error_code());
    })
}

fn mutation_intent_token_error_to_gql(error: MutationIntentTokenLookupError) -> GqlError {
    mutation_intent_commit_error_to_gql(MutationIntentCommitValidationError::Token(error))
}

fn mutation_intent_commit_error_to_gql(error: MutationIntentCommitValidationError) -> GqlError {
    let code = error.error_code();
    let stale = match &error {
        MutationIntentCommitValidationError::IntentStale { dimensions, .. } => {
            serde_json::to_value(dimensions).ok()
        }
        _ => None,
    };
    GqlError::new(error.to_string()).extend_with(move |_err, ext| {
        ext.set("code", code);
        if let Some(value) = &stale {
            if let Ok(gql_value) = GqlValue::from_json(value.clone()) {
                ext.set("stale", gql_value);
            }
        }
    })
}

fn canonical_operation_json(operation: &CanonicalOperationMetadata) -> Value {
    json!({
        "operationKind": operation_kind_label(&operation.operation_kind),
        "operationHash": operation.operation_hash,
        "operation": operation.canonical_operation.clone().unwrap_or(Value::Null),
    })
}

fn mutation_approval_route_json(route: &MutationApprovalRoute) -> Value {
    json!({
        "role": route.role,
        "reasonRequired": route.reason_required,
        "deadlineSeconds": route.deadline_seconds,
        "separationOfDuties": route.separation_of_duties,
    })
}

fn pre_image_json(pre_image: &PreImageBinding) -> Value {
    match pre_image {
        PreImageBinding::Entity {
            collection,
            id,
            version,
        } => json!({
            "kind": "entity",
            "collection": collection,
            "id": id,
            "version": version,
        }),
        PreImageBinding::Link {
            collection,
            id,
            version,
        } => json!({
            "kind": "link",
            "collection": collection,
            "id": id,
            "version": version,
        }),
    }
}

fn mutation_intent_json(intent: &MutationIntent) -> Value {
    json!({
        "id": intent.intent_id,
        "tenantId": intent.scope.tenant_id,
        "databaseId": intent.scope.database_id,
        "subject": serde_json::to_value(&intent.subject).unwrap_or_else(|_| json!({})),
        "schemaVersion": intent.schema_version,
        "policyVersion": intent.policy_version,
        "operation": canonical_operation_json(&intent.operation),
        "operationHash": intent.operation.operation_hash,
        "preImages": intent.pre_images.iter().map(pre_image_json).collect::<Vec<_>>(),
        "decision": intent.decision.as_str(),
        "approvalState": intent.approval_state.as_str(),
        "approvalRoute": intent.approval_route.as_ref().map(mutation_approval_route_json),
        "expiresAtNs": intent.expires_at.to_string(),
        "reviewSummary": serde_json::to_value(&intent.review_summary).unwrap_or_else(|_| json!({})),
    })
}

async fn rollback_entity_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    let input = ctx.args.try_get("input")?.as_value();
    let input_json = gql_input_to_json(input)?;
    let input_obj = input_json
        .as_object()
        .ok_or_else(|| GqlError::new("input must be an object"))?;

    let collection = input_obj
        .get("collection")
        .and_then(Value::as_str)
        .ok_or_else(|| GqlError::new("collection is required"))?;
    let id = input_obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| GqlError::new("id is required"))?;
    let to_version = input_obj
        .get("toVersion")
        .and_then(Value::as_i64)
        .filter(|value| *value >= 0)
        .map(|value| value as u64)
        .ok_or_else(|| GqlError::new("toVersion is required and must be non-negative"))?;
    let expected_version = input_obj
        .get("expectedVersion")
        .and_then(Value::as_i64)
        .filter(|value| *value >= 0)
        .map(|value| value as u64);
    let dry_run = input_obj
        .get("dryRun")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let collection_id = CollectionId::new(collection);
    let mut guard = handler.lock().await;
    let schema = guard
        .get_schema(&collection_id)
        .map_err(axon_error_to_gql)?;
    match guard.rollback_entity(RollbackEntityRequest {
        collection: collection_id,
        id: EntityId::new(id),
        target: RollbackEntityTarget::Version(to_version),
        expected_version,
        actor: Some(caller.actor),
        dry_run,
    }) {
        Ok(axon_api::response::RollbackEntityResponse::DryRun {
            current,
            target,
            diff,
        }) => Ok(Some(json_to_field_value(json!({
            "dryRun": true,
            "current": current
                .as_ref()
                .map(|entity| entity_to_generic_json_with_schema(entity, schema.as_ref())),
            "target": entity_to_generic_json_with_schema(&target, schema.as_ref()),
            "diff": diff,
            "entity": Value::Null,
            "auditEntry": Value::Null,
        })))),
        Ok(axon_api::response::RollbackEntityResponse::Applied {
            entity,
            audit_entry,
        }) => Ok(Some(json_to_field_value(json!({
            "dryRun": false,
            "current": Value::Null,
            "target": entity_to_generic_json_with_schema(&entity, schema.as_ref()),
            "diff": {},
            "entity": entity_to_generic_json_with_schema(&entity, schema.as_ref()),
            "auditEntry": audit_entry_json(&audit_entry),
        })))),
        Err(e) => Err(axon_error_to_gql(e)),
    }
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

async fn put_collection_template_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Admin).map_err(axon_error_to_gql)?;
    let input_json = gql_input_to_json(ctx.args.try_get("input")?.as_value())?;
    let input = input_object(&input_json, "input")?;
    let collection = CollectionId::new(input_string(input, "collection")?);
    let template = input_string(input, "template")?;
    let resp = handler
        .lock()
        .await
        .put_collection_template(PutCollectionTemplateRequest {
            collection,
            template,
            actor: Some(caller.actor),
        })
        .map_err(axon_error_to_gql)?;

    Ok(Some(json_to_field_value(collection_template_json(
        &resp.view,
        &resp.warnings,
    ))))
}

async fn delete_collection_template_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Admin).map_err(axon_error_to_gql)?;
    let collection = ctx.args.try_get("collection")?.string()?.to_owned();
    let resp = handler
        .lock()
        .await
        .delete_collection_template(DeleteCollectionTemplateRequest {
            collection: CollectionId::new(collection),
            actor: Some(caller.actor),
        })
        .map_err(axon_error_to_gql)?;

    Ok(Some(json_to_field_value(json!({
        "collection": resp.collection,
        "deleted": true,
    }))))
}

async fn revert_audit_entry_resolver<S: StorageAdapter + 'static>(
    ctx: async_graphql::dynamic::ResolverContext<'_>,
    handler: SharedHandler<S>,
    caller: CallerIdentity,
) -> Result<Option<FieldValue<'static>>, GqlError> {
    caller.check(Operation::Write).map_err(axon_error_to_gql)?;
    let audit_entry_id = ctx
        .args
        .try_get("auditEntryId")?
        .string()?
        .parse::<u64>()
        .map_err(|e| {
            GqlError::new(format!("auditEntryId must be an unsigned integer: {e}")).extend_with(
                |_err, ext| {
                    ext.set("code", "INVALID_ARGUMENT");
                },
            )
        })?;
    let force = ctx
        .args
        .try_get("force")
        .ok()
        .and_then(|value| value.boolean().ok())
        .unwrap_or(false);

    let mut guard = handler.lock().await;
    let resp = guard
        .revert_entity_to_audit_entry(RevertEntityRequest {
            audit_entry_id,
            actor: Some(caller.actor),
            force,
            attribution: None,
        })
        .map_err(axon_error_to_gql)?;
    let schema = guard
        .get_schema(&resp.entity.collection)
        .map_err(axon_error_to_gql)?;

    Ok(Some(json_to_field_value(json!({
        "entity": entity_to_generic_json_with_schema(&resp.entity, schema.as_ref()),
        "auditEntry": audit_entry_json(&resp.audit_entry),
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
    let explain_inputs = match input.get("explainInputs") {
        Some(value) if !value.is_null() => explain_policy_dry_run_inputs_from_value(value)?,
        _ => Vec::new(),
    };

    let resp = handler
        .lock()
        .await
        .handle_put_schema(PutSchemaRequest {
            schema,
            actor: Some(caller.actor),
            force,
            dry_run,
            explain_inputs,
        })
        .map_err(axon_error_to_gql)?;

    Ok(Some(json_to_field_value(put_schema_payload_value(resp))))
}

/// Parse a list of `ExplainPolicyInput` values from `putSchema(input.explainInputs)`.
///
/// `ExplainPolicyInput` shares its shape with the active `explainPolicy`
/// query input — the extractor expects the bare object (its `input_object`
/// helper just casts to JSON object for an error label, it does not
/// dereference an outer `input` field).
fn explain_policy_dry_run_inputs_from_value(
    value: &Value,
) -> Result<Vec<ExplainPolicyRequest>, GqlError> {
    let entries = value.as_array().ok_or_else(|| {
        GqlError::new("explainInputs must be a list").extend_with(|_err, ext| {
            ext.set("code", "INVALID_ARGUMENT");
        })
    })?;
    entries
        .iter()
        .map(explain_policy_request_from_value)
        .collect()
}

// ── Schema builders ─────────────────────────────────────────────────────────

fn policy_plans_by_collection(
    collections: &[CollectionSchema],
) -> Result<HashMap<String, PolicyPlan>, String> {
    compile_policy_catalog(collections)
        .map(|catalog| catalog.plans)
        .map_err(|e| format!("failed to compile access_control policies for GraphQL schema: {e}"))
}

fn policy_nullable_field_names(plan: Option<&PolicyPlan>) -> HashSet<String> {
    plan.map(|plan| {
        plan.report
            .nullable_fields
            .iter()
            .filter(|field| field.graphql_nullable)
            .map(|field| field.field.clone())
            .collect()
    })
    .unwrap_or_default()
}

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
    build_schema_with_handler_and_broker_scoped(collections, handler, broker, None)
}

/// Build a dynamic GraphQL schema with subscription events restricted to a
/// tenant/database scope when provided.
pub fn build_schema_with_handler_and_broker_scoped<S: StorageAdapter + 'static>(
    collections: &[CollectionSchema],
    handler: SharedHandler<S>,
    broker: Option<BroadcastBroker>,
    subscription_scope: Option<(String, String)>,
) -> Result<AxonSchema, String> {
    let subscription_scope =
        subscription_scope.map(|(tenant, database)| SubscriptionScope { tenant, database });
    let policy_plans = policy_plans_by_collection(collections)?;
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
    query = add_handler_intent_root_query_fields(query, Arc::clone(&handler));
    mutation = add_handler_intent_root_mutation_fields(mutation, Arc::clone(&handler));

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
        let policy_nullable_fields = policy_nullable_field_names(policy_plans.get(collection_name));
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
        for (field_name, gql_type, required) in &fields {
            object_field_names.insert(field_name.clone());
            let type_ref = output_type_ref_for_field(
                gql_type,
                *required,
                policy_nullable_fields.contains(field_name),
            );
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
            &policy_nullable_fields,
        ));
        type_objects.push(typed_entity_payload_object(
            &update_payload_name,
            &type_name,
            &data_fields,
            &policy_nullable_fields,
        ));
        type_objects.push(typed_entity_payload_object(
            &patch_payload_name,
            &type_name,
            &data_fields,
            &policy_nullable_fields,
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
            let caller = caller_from_ctx(&ctx);
            FieldFuture::new(async move {
                let id_str = ctx.args.try_get("id")?.string()?;

                let guard = handler.lock().await;
                match guard.get_entity_with_caller(
                    GetEntityRequest {
                        collection: col.clone(),
                        id: EntityId::new(id_str),
                    },
                    &caller,
                    None,
                ) {
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
                let caller = caller_from_ctx(&ctx);
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
                    match guard.query_entities_with_caller(
                        QueryEntitiesRequest {
                            collection: col.clone(),
                            filter,
                            sort,
                            limit,
                            after_id,
                            count_only: false,
                        },
                        &caller,
                        None,
                    ) {
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
                let caller = caller_from_ctx(&ctx);
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
                    match guard.query_entities_with_caller(
                        QueryEntitiesRequest {
                            collection: col.clone(),
                            filter,
                            sort,
                            limit,
                            after_id,
                            count_only: false,
                        },
                        &caller,
                        None,
                    ) {
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
                let caller = caller_from_ctx(&ctx);
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
                    let response = guard.query_entities_with_caller(
                        QueryEntitiesRequest {
                            collection: col.clone(),
                            filter,
                            sort: Vec::new(),
                            limit: None,
                            after_id: None,
                            count_only: false,
                        },
                        &caller,
                        None,
                    )?;
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

        let handler_put_collection_template = Arc::clone(&handler);
        let put_collection_template_field = Field::new(
            "putCollectionTemplate",
            TypeRef::named_nn(COLLECTION_TEMPLATE_TYPE),
            move |ctx| {
                let handler = Arc::clone(&handler_put_collection_template);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    put_collection_template_resolver(ctx, handler, caller).await
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(PUT_COLLECTION_TEMPLATE_INPUT),
        ));
        mutation = mutation.field(put_collection_template_field);

        let handler_delete_collection_template = Arc::clone(&handler);
        let delete_collection_template_field = Field::new(
            "deleteCollectionTemplate",
            TypeRef::named_nn(DELETE_COLLECTION_TEMPLATE_PAYLOAD),
            move |ctx| {
                let handler = Arc::clone(&handler_delete_collection_template);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(async move {
                    delete_collection_template_resolver(ctx, handler, caller).await
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        ));
        mutation = mutation.field(delete_collection_template_field);

        let handler_revert_audit_entry = Arc::clone(&handler);
        let revert_audit_entry_field = Field::new(
            "revertAuditEntry",
            TypeRef::named_nn(REVERT_AUDIT_ENTRY_PAYLOAD),
            move |ctx| {
                let handler = Arc::clone(&handler_revert_audit_entry);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(
                    async move { revert_audit_entry_resolver(ctx, handler, caller).await },
                )
            },
        )
        .argument(InputValue::new(
            "auditEntryId",
            TypeRef::named_nn(TypeRef::ID),
        ))
        .argument(InputValue::new("force", TypeRef::named(TypeRef::BOOLEAN)));
        mutation = mutation.field(revert_audit_entry_field);

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

        let handler_rollback_entity = Arc::clone(&handler);
        let rollback_entity_field = Field::new(
            "rollbackEntity",
            TypeRef::named_nn(ROLLBACK_ENTITY_PAYLOAD),
            move |ctx| {
                let handler = Arc::clone(&handler_rollback_entity);
                let caller = caller_from_ctx(&ctx);
                FieldFuture::new(
                    async move { rollback_entity_resolver(ctx, handler, caller).await },
                )
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(ROLLBACK_ENTITY_INPUT),
        ));
        mutation = mutation.field(rollback_entity_field);
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
    let subscription = broker
        .map(|broker| build_entity_changed_subscription(broker, collections, subscription_scope));

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
    .register(put_collection_template_input_object())
    .register(rollback_entity_input_object())
    .register(explain_policy_input_object())
    .register(commit_transaction_input_object())
    .register(transaction_operation_input_object())
    .register(create_entity_transaction_input_object())
    .register(update_entity_transaction_input_object())
    .register(patch_entity_transaction_input_object())
    .register(delete_entity_transaction_input_object())
    .register(create_link_transaction_input_object())
    .register(delete_link_transaction_input_object())
    .register(canonical_operation_input_object())
    .register(mutation_preview_input_object())
    .register(approve_intent_input_object())
    .register(reject_intent_input_object())
    .register(commit_intent_input_object())
    .register(mutation_intent_filter_input_object())
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
    let policy_plans = policy_plans_by_collection(collections)?;
    let mut query = Object::new("Query");
    let mut mutation = Object::new("Mutation");
    let mut type_objects = Vec::new();

    query = add_stub_root_query_fields(query);
    query = add_intent_root_query_fields(query);
    mutation = add_intent_root_mutation_fields(mutation);

    for schema in collections {
        let collection_name = schema.collection.as_str();
        let type_name = pascal_case(collection_name);
        let edge_type_name = format!("{type_name}Edge");
        let connection_type_name = format!("{type_name}Connection");
        let get_field_name = collection_field_name(collection_name);
        let list_field_name = collection_list_field_name(collection_name);
        let fields = extract_fields(schema);
        let policy_nullable_fields = policy_nullable_field_names(policy_plans.get(collection_name));

        // Build the GraphQL object type for this collection.
        let mut obj = Object::new(&type_name);
        for (field_name, gql_type, required) in &fields {
            let type_ref = output_type_ref_for_field(
                gql_type,
                *required,
                policy_nullable_fields.contains(field_name),
            );
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
            "putCollectionTemplate",
            TypeRef::named_nn(COLLECTION_TEMPLATE_TYPE),
            |_ctx| {
                FieldFuture::new(async move {
                    Ok(Some(json_to_field_value(json!({
                        "collection": "",
                        "template": "",
                        "version": 1,
                        "updatedAtNs": Value::Null,
                        "updatedBy": Value::Null,
                        "warnings": [],
                    }))))
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(PUT_COLLECTION_TEMPLATE_INPUT),
        )),
    );

    mutation = mutation.field(
        Field::new(
            "deleteCollectionTemplate",
            TypeRef::named_nn(DELETE_COLLECTION_TEMPLATE_PAYLOAD),
            |_ctx| {
                FieldFuture::new(async move {
                    Ok(Some(json_to_field_value(json!({
                        "collection": "",
                        "deleted": true,
                    }))))
                })
            },
        )
        .argument(InputValue::new(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
        )),
    );

    mutation = mutation.field(
        Field::new(
            "revertAuditEntry",
            TypeRef::named_nn(REVERT_AUDIT_ENTRY_PAYLOAD),
            |_ctx| {
                FieldFuture::new(async move {
                    Ok(Some(json_to_field_value(json!({
                        "entity": {
                            "id": "",
                            "collection": "",
                            "version": 0,
                            "data": {},
                            "lifecycles": {},
                        },
                        "auditEntry": {
                            "id": "0",
                            "timestampNs": "0",
                            "collection": "",
                            "entityId": "",
                            "version": 0,
                            "mutation": "entity.revert",
                            "dataBefore": Value::Null,
                            "dataAfter": {},
                            "actor": "anonymous",
                            "metadata": {},
                            "transactionId": Value::Null,
                        },
                    }))))
                })
            },
        )
        .argument(InputValue::new(
            "auditEntryId",
            TypeRef::named_nn(TypeRef::ID),
        ))
        .argument(InputValue::new("force", TypeRef::named(TypeRef::BOOLEAN))),
    );

    mutation = mutation.field(
        Field::new(
            "rollbackEntity",
            TypeRef::named_nn(ROLLBACK_ENTITY_PAYLOAD),
            |_ctx| {
                FieldFuture::new(async move {
                    Ok(Some(json_to_field_value(json!({
                        "dryRun": true,
                        "current": Value::Null,
                        "target": {
                            "id": "",
                            "collection": "",
                            "version": 0,
                            "data": {},
                            "lifecycles": {},
                        },
                        "diff": {},
                        "entity": Value::Null,
                        "auditEntry": Value::Null,
                    }))))
                })
            },
        )
        .argument(InputValue::new(
            "input",
            TypeRef::named_nn(ROLLBACK_ENTITY_INPUT),
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
        .register(put_collection_template_input_object())
        .register(rollback_entity_input_object())
        .register(explain_policy_input_object())
        .register(commit_transaction_input_object())
        .register(transaction_operation_input_object())
        .register(create_entity_transaction_input_object())
        .register(update_entity_transaction_input_object())
        .register(patch_entity_transaction_input_object())
        .register(delete_entity_transaction_input_object())
        .register(create_link_transaction_input_object())
        .register(delete_link_transaction_input_object())
        .register(canonical_operation_input_object())
        .register(mutation_preview_input_object())
        .register(approve_intent_input_object())
        .register(reject_intent_input_object())
        .register(commit_intent_input_object())
        .register(mutation_intent_filter_input_object())
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
        .field(change_event_object_field(
            "auditId",
            TypeRef::named_nn(TypeRef::ID),
            "auditId",
        ))
        .field(change_event_object_field(
            "collection",
            TypeRef::named_nn(TypeRef::STRING),
            "collection",
        ))
        .field(change_event_object_field(
            "entityId",
            TypeRef::named_nn(TypeRef::STRING),
            "entityId",
        ))
        .field(change_event_object_field(
            "operation",
            TypeRef::named_nn(TypeRef::STRING),
            "operation",
        ))
        .field(change_event_object_field(
            "mutation",
            TypeRef::named_nn(TypeRef::STRING),
            "operation",
        ))
        .field(change_event_object_field(
            "data",
            TypeRef::named("JSON"),
            "data",
        ))
        .field(change_event_object_field(
            "previousData",
            TypeRef::named("JSON"),
            "previousData",
        ))
        .field(change_event_object_field(
            "version",
            TypeRef::named_nn(TypeRef::INT),
            "version",
        ))
        .field(change_event_object_field(
            "previousVersion",
            TypeRef::named(TypeRef::INT),
            "previousVersion",
        ))
        .field(change_event_object_field(
            "timestampMs",
            TypeRef::named_nn(TypeRef::INT),
            "timestampMs",
        ))
        .field(change_event_object_field(
            "timestampNs",
            TypeRef::named_nn(TypeRef::STRING),
            "timestampNs",
        ))
        .field(change_event_object_field(
            "actor",
            TypeRef::named_nn(TypeRef::STRING),
            "actor",
        ))
}

fn change_event_object_field(name: &'static str, ty: TypeRef, key: &'static str) -> Field {
    Field::new(name, ty, move |ctx| {
        FieldFuture::new(async move {
            match ctx.parent_value.try_to_value() {
                Ok(GqlValue::Object(map)) => {
                    let key = async_graphql::Name::new(key);
                    Ok(map.get(&key).map(|v| FieldValue::from(v.clone())))
                }
                _ => Ok(Some(FieldValue::NULL)),
            }
        })
    })
}

/// Convert a `ChangeEvent` into a `FieldValue` suitable for subscription emission.
fn change_event_to_field_value(event: &crate::subscriptions::ChangeEvent) -> FieldValue<'static> {
    let mut map = serde_json::Map::new();
    map.insert("auditId".into(), Value::String(event.audit_id.clone()));
    map.insert("collection".into(), Value::String(event.collection.clone()));
    map.insert("entityId".into(), Value::String(event.entity_id.clone()));
    map.insert("operation".into(), Value::String(event.operation.clone()));
    if let Some(data) = &event.data {
        map.insert("data".into(), data.clone());
    }
    if let Some(previous_data) = &event.previous_data {
        map.insert("previousData".into(), previous_data.clone());
    }
    map.insert("version".into(), json!(event.version));
    if let Some(previous_version) = event.previous_version {
        map.insert("previousVersion".into(), json!(previous_version));
    }
    map.insert("timestampMs".into(), json!(event.timestamp_ms));
    map.insert(
        "timestampNs".into(),
        Value::String(event.timestamp_ms.saturating_mul(1_000_000).to_string()),
    );
    map.insert("actor".into(), Value::String(event.actor.clone()));

    FieldValue::from(GqlValue::from_json(Value::Object(map)).unwrap_or(GqlValue::Null))
}

fn subscription_filter_matches(
    filter: &FilterNode,
    event: &crate::subscriptions::ChangeEvent,
) -> bool {
    let data = event.data.as_ref().or(event.previous_data.as_ref());
    match filter {
        FilterNode::Field(field) => subscription_field_filter_matches(field, data),
        FilterNode::Gate(_) => false,
        FilterNode::And { filters } => filters
            .iter()
            .all(|filter| subscription_filter_matches(filter, event)),
        FilterNode::Or { filters } => filters
            .iter()
            .any(|filter| subscription_filter_matches(filter, event)),
    }
}

fn subscription_field_filter_matches(field: &FieldFilter, data: Option<&Value>) -> bool {
    let field_value = data.and_then(|data| subscription_field_value(data, &field.field));
    match field.op {
        FilterOp::Eq => field_value == Some(&field.value),
        FilterOp::Ne => field_value != Some(&field.value),
        FilterOp::Gt => subscription_compare_values(field_value, Some(&field.value)).is_gt(),
        FilterOp::Gte => {
            let ordering = subscription_compare_values(field_value, Some(&field.value));
            ordering.is_gt() || ordering.is_eq()
        }
        FilterOp::Lt => subscription_compare_values(field_value, Some(&field.value)).is_lt(),
        FilterOp::Lte => {
            let ordering = subscription_compare_values(field_value, Some(&field.value));
            ordering.is_lt() || ordering.is_eq()
        }
        FilterOp::In => match &field.value {
            Value::Array(values) => values.iter().any(|value| field_value == Some(value)),
            _ => false,
        },
        FilterOp::Contains => match (field_value, &field.value) {
            (Some(Value::String(value)), Value::String(needle)) => value.contains(needle),
            _ => false,
        },
    }
}

fn subscription_field_value<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = data;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn subscription_compare_values(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(Value::Number(a)), Some(Value::Number(b))) => a
            .as_f64()
            .and_then(|a| b.as_f64().and_then(|b| a.partial_cmp(&b)))
            .unwrap_or(Ordering::Equal),
        (Some(Value::String(a)), Some(Value::String(b))) => a.cmp(b),
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a.cmp(b),
        (Some(Value::Null), Some(Value::Null)) | (None, None) => Ordering::Equal,
        (Some(Value::Null) | None, Some(_)) => Ordering::Less,
        (Some(_), Some(Value::Null) | None) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

#[derive(Clone)]
struct SubscriptionScope {
    tenant: String,
    database: String,
}

fn subscription_event_matches(
    event: &crate::subscriptions::ChangeEvent,
    collection: Option<&str>,
    filter: Option<&FilterNode>,
    scope: Option<&SubscriptionScope>,
) -> bool {
    if let Some(scope) = scope {
        if event.tenant.as_deref() != Some(scope.tenant.as_str())
            || event.database.as_deref() != Some(scope.database.as_str())
        {
            return false;
        }
    }
    if let Some(collection) = collection {
        if event.collection != collection {
            return false;
        }
    }
    match filter {
        Some(filter) => subscription_filter_matches(filter, event),
        None => true,
    }
}

fn build_change_subscription_field(
    field_name: &str,
    broker: BroadcastBroker,
    fixed_collection: Option<String>,
    scope: Option<SubscriptionScope>,
) -> SubscriptionField {
    let has_fixed_collection = fixed_collection.is_some();
    let mut field =
        SubscriptionField::new(field_name, TypeRef::named_nn("ChangeEvent"), move |ctx| {
            let broker = broker.clone();
            let fixed_collection = fixed_collection.clone();
            let scope = scope.clone();

            let collection_filter = match fixed_collection {
                Some(collection) => Some(collection),
                None => ctx
                    .args
                    .try_get("collection")
                    .ok()
                    .and_then(|v| v.string().ok())
                    .map(str::to_owned),
            };
            let filter_result = ctx
                .args
                .try_get("filter")
                .ok()
                .map(|value| parse_graphql_filter_arg(value.as_value()))
                .transpose();

            SubscriptionFieldFuture::new(async move {
                let filter = filter_result?;
                let rx = broker.subscribe();
                let stream =
                    tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(move |result| {
                        let collection_filter = collection_filter.clone();
                        let filter = filter.clone();
                        let scope = scope.clone();
                        async move {
                            match result {
                                Ok(event)
                                    if subscription_event_matches(
                                        &event,
                                        collection_filter.as_deref(),
                                        filter.as_ref(),
                                        scope.as_ref(),
                                    ) =>
                                {
                                    Some(Ok(change_event_to_field_value(&event)))
                                }
                                Ok(_) | Err(_) => None,
                            }
                        }
                    });

                Ok(stream)
            })
        })
        .argument(InputValue::new("filter", TypeRef::named(FILTER_INPUT)));

    if !has_fixed_collection {
        field = field.argument(InputValue::new(
            "collection",
            TypeRef::named(TypeRef::STRING),
        ));
    }
    field
}

/// Build the `Subscription` type with generic and per-collection change fields.
fn build_entity_changed_subscription(
    broker: BroadcastBroker,
    collections: &[CollectionSchema],
    scope: Option<SubscriptionScope>,
) -> Subscription {
    let entity_changed =
        build_change_subscription_field("entityChanged", broker.clone(), None, scope.clone())
            .description(
                "Subscribe to entity change events. Optionally filter by collection name.",
            );
    let mut subscription = Subscription::new("Subscription").field(entity_changed);
    for schema in collections {
        let field_name = format!(
            "{}Changed",
            collection_field_name(schema.collection.as_str())
        );
        subscription = subscription.field(
            build_change_subscription_field(
                &field_name,
                broker.clone(),
                Some(schema.collection.to_string()),
                scope.clone(),
            )
            .description(format!(
                "Subscribe to {} entity change events.",
                schema.collection
            )),
        );
    }
    subscription
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
        "entity"
            | "entities"
            | "collections"
            | "collection"
            | "auditLog"
            | "mutationIntent"
            | "pendingMutationIntents"
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
            | "CanonicalOperation"
            | "MutationPreviewInput"
            | "CanonicalOperationInput"
            | "ApproveIntentInput"
            | "RejectIntentInput"
            | "CommitIntentInput"
            | "MutationIntentFilter"
            | "MutationPreviewResult"
            | "MutationIntent"
            | "MutationApprovalRoute"
            | "MutationIntentPreImage"
            | "MutationIntentStaleDimension"
            | "MutationIntentEdge"
            | "MutationIntentConnection"
            | "CommitIntentResult"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_api::test_fixtures::seed_procurement_fixture;
    use axon_core::id::CollectionId;
    use axon_schema::access_control::AccessControlPolicy;
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
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        }
    }

    fn policy_nullable_schema() -> CollectionSchema {
        CollectionSchema {
            collection: CollectionId::new("purchase_orders"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["status", "amount_cents", "restricted_notes"],
                "properties": {
                    "status": { "type": "string" },
                    "amount_cents": { "type": "integer" },
                    "restricted_notes": { "type": "string" },
                    "memo": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value::<AccessControlPolicy>(json!({
                    "fields": {
                        "restricted_notes": {
                            "read": {
                                "deny": [
                                    {
                                        "name": "redact-restricted-notes",
                                        "redact_as": null
                                    }
                                ]
                            }
                        }
                    }
                }))
                .expect("access_control should deserialize"),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        }
    }

    fn policy_override_schema() -> CollectionSchema {
        CollectionSchema {
            collection: CollectionId::new("policy_items"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "title": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value::<AccessControlPolicy>(json!({
                    "read": { "allow": [{ "name": "active-allows-read" }] }
                }))
                .expect("access_control should deserialize"),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        }
    }

    async fn introspected_type_fields(schema: &AxonSchema, type_name: &str) -> Value {
        let result = schema
            .schema
            .execute(format!(
                r#"{{
                    __type(name: "{type_name}") {{
                        fields {{
                            name
                            type {{
                                kind
                                name
                                ofType {{ kind name }}
                            }}
                        }}
                    }}
                }}"#
            ))
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        result.data.into_json().expect("introspection data is JSON")["__type"]["fields"].clone()
    }

    fn introspected_field_type<'a>(fields: &'a Value, field_name: &str) -> &'a Value {
        fields
            .as_array()
            .expect("fields should be a list")
            .iter()
            .find(|field| field["name"] == field_name)
            .unwrap_or_else(|| panic!("field {field_name} should be introspectable"))
            .get("type")
            .expect("field should include type")
    }

    fn assert_nullable_scalar(fields: &Value, field_name: &str, scalar: &str) {
        let ty = introspected_field_type(fields, field_name);
        assert_eq!(ty["kind"], "SCALAR");
        assert_eq!(ty["name"], scalar);
        assert!(ty["ofType"].is_null(), "{field_name} should not be wrapped");
    }

    fn assert_non_null_scalar(fields: &Value, field_name: &str, scalar: &str) {
        let ty = introspected_field_type(fields, field_name);
        assert_eq!(ty["kind"], "NON_NULL");
        assert!(ty["name"].is_null());
        assert_eq!(ty["ofType"]["kind"], "SCALAR");
        assert_eq!(ty["ofType"]["name"], scalar);
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
    async fn build_schema_exposes_mutation_intent_roots_and_types() {
        let schema = build_schema(&[]).expect("schema should build");
        let result = schema
            .schema
            .execute(
                r#"{
                    query: __type(name: "Query") { fields { name } }
                    mutation: __type(name: "Mutation") {
                        fields { name args { name type { kind name ofType { kind name } } } }
                    }
                    previewInput: __type(name: "MutationPreviewInput") {
                        inputFields { name }
                    }
                    canonicalInput: __type(name: "CanonicalOperationInput") {
                        inputFields { name }
                    }
                    previewResult: __type(name: "MutationPreviewResult") {
                        fields { name }
                    }
                    intent: __type(name: "MutationIntent") { fields { name } }
                    approvalRoute: __type(name: "MutationApprovalRoute") { fields { name } }
                    preImage: __type(name: "MutationIntentPreImage") { fields { name } }
                    stale: __type(name: "MutationIntentStaleDimension") { fields { name } }
                    commitResult: __type(name: "CommitIntentResult") { fields { name } }
                }"#,
            )
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let body = result.data.into_json().expect("introspection data is JSON");

        let query_fields: Vec<&str> = body["query"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|field| field["name"].as_str())
            .collect();
        for expected in ["mutationIntent", "pendingMutationIntents"] {
            assert!(
                query_fields.contains(&expected),
                "missing intent query field {expected}: {body}"
            );
        }

        let mutation_fields: Vec<&str> = body["mutation"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|field| field["name"].as_str())
            .collect();
        for expected in [
            "previewMutation",
            "approveMutationIntent",
            "rejectMutationIntent",
            "commitMutationIntent",
        ] {
            assert!(
                mutation_fields.contains(&expected),
                "missing intent mutation field {expected}: {body}"
            );
        }

        for (type_alias, expected_fields) in [
            (
                "previewInput",
                vec!["operation", "subject", "expiresInSeconds", "reason"],
            ),
            (
                "canonicalInput",
                vec!["operationKind", "operationHash", "operation"],
            ),
            (
                "previewResult",
                vec![
                    "decision",
                    "intent",
                    "intentToken",
                    "canonicalOperation",
                    "diff",
                    "affectedRecords",
                    "affectedFields",
                    "approvalRoute",
                    "policyExplanation",
                ],
            ),
            (
                "intent",
                vec![
                    "id",
                    "tenantId",
                    "databaseId",
                    "subject",
                    "schemaVersion",
                    "policyVersion",
                    "operation",
                    "operationHash",
                    "preImages",
                    "decision",
                    "approvalState",
                    "approvalRoute",
                    "expiresAtNs",
                    "reviewSummary",
                ],
            ),
            (
                "approvalRoute",
                vec![
                    "role",
                    "reasonRequired",
                    "deadlineSeconds",
                    "separationOfDuties",
                ],
            ),
            ("preImage", vec!["kind", "collection", "id", "version"]),
            ("stale", vec!["dimension", "expected", "actual", "path"]),
            (
                "commitResult",
                vec![
                    "committed",
                    "intent",
                    "transactionId",
                    "auditEntry",
                    "stale",
                    "errorCode",
                ],
            ),
        ] {
            let member_key = if type_alias.ends_with("Input") {
                "inputFields"
            } else {
                "fields"
            };
            let fields: Vec<&str> = body[type_alias][member_key]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|field| field["name"].as_str())
                .collect();
            for expected in expected_fields {
                assert!(
                    fields.contains(&expected),
                    "missing {type_alias}.{expected}: {body}"
                );
            }
        }
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
            sdl.contains("tasksChanged"),
            "SDL should contain generated per-collection subscription field"
        );
        assert!(sdl.contains("filter: AxonFilterInput"));
        assert!(
            sdl.contains("type ChangeEvent"),
            "SDL should contain ChangeEvent type"
        );
        for field in [
            "auditId",
            "previousData",
            "previousVersion",
            "timestampNs",
            "mutation",
        ] {
            assert!(sdl.contains(field), "SDL should contain {field}");
        }
    }

    async fn wait_for_receivers(broker: &crate::subscriptions::BroadcastBroker, count: usize) {
        for _ in 0..50 {
            if broker.receiver_count() >= count {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!(
            "expected at least {count} subscription receiver(s), got {}",
            broker.receiver_count()
        );
    }

    fn subscription_test_event(id: &str, status: &str) -> crate::subscriptions::ChangeEvent {
        crate::subscriptions::ChangeEvent {
            tenant: None,
            database: None,
            audit_id: format!("audit-{id}"),
            collection: "tasks".into(),
            entity_id: id.into(),
            operation: "update".into(),
            data: Some(json!({"title": id, "status": status, "priority": 3})),
            previous_data: Some(json!({"title": id, "status": "draft", "priority": 2})),
            version: 2,
            previous_version: Some(1),
            timestamp_ms: 1234,
            actor: "tester".into(),
        }
    }

    fn response_data(response: async_graphql::Response) -> Value {
        assert!(
            response.errors.is_empty(),
            "unexpected subscription errors: {:?}",
            response.errors
        );
        response.data.into_json().expect("response data is JSON")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn subscription_stream_filters_delivery_with_filter_node_semantics() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;
        let broker = crate::subscriptions::BroadcastBroker::default();
        let schema =
            build_schema_with_handler_and_broker(&[ts], Arc::clone(&handler), Some(broker.clone()))
                .expect("schema with broker should build");

        let mut stream = schema.schema.execute_stream(async_graphql::Request::new(
            r#"subscription {
                tasksChanged(filter: { field: "status", op: "eq", value: "open" }) {
                    auditId
                    entityId
                    operation
                    mutation
                    data
                    previousData
                    version
                    previousVersion
                    timestampMs
                    actor
                }
            }"#,
        ));

        let publish = async {
            wait_for_receivers(&broker, 1).await;
            let _ = broker.publish(subscription_test_event("closed-task", "closed"));
            let _ = broker.publish(subscription_test_event("open-task", "open"));
        };
        let receive = async {
            tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
                .await
                .expect("filtered subscription receives a matching event")
                .expect("subscription stream yields a response")
        };
        let (response, _) = tokio::join!(receive, publish);
        let data = response_data(response);
        let event = &data["tasksChanged"];

        assert_eq!(event["auditId"], "audit-open-task");
        assert_eq!(event["entityId"], "open-task");
        assert_eq!(event["operation"], "update");
        assert_eq!(event["mutation"], "update");
        assert_eq!(event["data"]["status"], "open");
        assert_eq!(event["previousData"]["status"], "draft");
        assert_eq!(event["version"], 2);
        assert_eq!(event["previousVersion"], 1);
        assert_eq!(event["timestampMs"], 1234);
        assert_eq!(event["actor"], "tester");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn subscription_stream_delivers_to_generic_and_collection_subscribers() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;
        let broker = crate::subscriptions::BroadcastBroker::default();
        let schema =
            build_schema_with_handler_and_broker(&[ts], Arc::clone(&handler), Some(broker.clone()))
                .expect("schema with broker should build");

        let mut generic_stream = schema.schema.execute_stream(async_graphql::Request::new(
            r#"subscription {
                entityChanged(collection: "tasks") { entityId collection auditId }
            }"#,
        ));
        let mut collection_stream = schema.schema.execute_stream(async_graphql::Request::new(
            r#"subscription {
                tasksChanged { entityId collection auditId }
            }"#,
        ));

        let publish = async {
            wait_for_receivers(&broker, 2).await;
            let _ = broker.publish(subscription_test_event("fanout-task", "open"));
        };
        let receive_generic = async {
            tokio::time::timeout(std::time::Duration::from_secs(1), generic_stream.next())
                .await
                .expect("generic subscriber receives event")
                .expect("generic stream yields a response")
        };
        let receive_collection = async {
            tokio::time::timeout(std::time::Duration::from_secs(1), collection_stream.next())
                .await
                .expect("collection subscriber receives event")
                .expect("collection stream yields a response")
        };

        let (generic_response, collection_response, _) =
            tokio::join!(receive_generic, receive_collection, publish);
        let generic = response_data(generic_response);
        let collection = response_data(collection_response);

        assert_eq!(generic["entityChanged"]["entityId"], "fanout-task");
        assert_eq!(generic["entityChanged"]["collection"], "tasks");
        assert_eq!(generic["entityChanged"]["auditId"], "audit-fanout-task");
        assert_eq!(collection["tasksChanged"]["entityId"], "fanout-task");
        assert_eq!(collection["tasksChanged"]["collection"], "tasks");
        assert_eq!(collection["tasksChanged"]["auditId"], "audit-fanout-task");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn subscription_schema_rebuild_tracks_collection_drop_and_schema_add() {
        let tasks = test_schema();
        let handler_with_tasks = make_handler(std::slice::from_ref(&tasks)).await;
        let broker = crate::subscriptions::BroadcastBroker::default();
        let schema_with_tasks = build_schema_with_handler_and_broker(
            &[tasks],
            Arc::clone(&handler_with_tasks),
            Some(broker.clone()),
        )
        .expect("schema with tasks should build");
        assert!(schema_with_tasks.schema.sdl().contains("tasksChanged"));

        let empty_handler = make_handler(&[]).await;
        let schema_after_drop =
            build_schema_with_handler_and_broker(&[], Arc::clone(&empty_handler), Some(broker))
                .expect("schema after drop should build");
        let sdl_after_drop = schema_after_drop.schema.sdl();
        assert!(sdl_after_drop.contains("entityChanged"));
        assert!(
            !sdl_after_drop.contains("tasksChanged"),
            "rebuilt subscription schema should remove per-collection fields for dropped collections"
        );

        let users = CollectionSchema {
            collection: CollectionId::new("users"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            lifecycles: Default::default(),
        };
        let handler_with_users = make_handler(std::slice::from_ref(&users)).await;
        let schema_after_add = build_schema_with_handler_and_broker(
            &[users],
            Arc::clone(&handler_with_users),
            Some(crate::subscriptions::BroadcastBroker::default()),
        )
        .expect("schema after add should build");
        let sdl_after_add = schema_after_add.schema.sdl();
        assert!(sdl_after_add.contains("usersChanged"));
        assert!(!sdl_after_add.contains("tasksChanged"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn subscription_stream_delivery_stays_under_latency_target_for_test_load() {
        let ts = test_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;
        let broker = crate::subscriptions::BroadcastBroker::default();
        let schema =
            build_schema_with_handler_and_broker(&[ts], Arc::clone(&handler), Some(broker.clone()))
                .expect("schema with broker should build");

        let mut stream = schema.schema.execute_stream(async_graphql::Request::new(
            r#"subscription {
                tasksChanged { entityId auditId }
            }"#,
        ));

        let publish = async {
            wait_for_receivers(&broker, 1).await;
            let started = tokio::time::Instant::now();
            let _ = broker.publish(subscription_test_event("latency-task", "open"));
            started
        };
        let receive = async {
            tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
                .await
                .expect("subscription delivers within FEAT-015 latency target")
                .expect("subscription stream yields a response")
        };

        let (response, started) = tokio::join!(receive, publish);
        let elapsed = started.elapsed();
        let data = response_data(response);
        assert_eq!(data["tasksChanged"]["entityId"], "latency-task");
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "write-to-subscriber latency exceeded target: {elapsed:?}"
        );
    }

    #[test]
    fn subscription_filter_matches_filter_node_semantics() {
        let event = crate::subscriptions::ChangeEvent {
            tenant: None,
            database: None,
            audit_id: "7".into(),
            collection: "tasks".into(),
            entity_id: "task-1".into(),
            operation: "update".into(),
            data: Some(json!({"status": "open", "priority": 3})),
            previous_data: Some(json!({"status": "draft", "priority": 2})),
            version: 2,
            previous_version: Some(1),
            timestamp_ms: 1000,
            actor: "alice".into(),
        };
        let filter = FilterNode::And {
            filters: vec![
                FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("open"),
                }),
                FilterNode::Field(FieldFilter {
                    field: "priority".into(),
                    op: FilterOp::Gte,
                    value: json!(3),
                }),
            ],
        };

        assert!(subscription_event_matches(
            &event,
            Some("tasks"),
            Some(&filter),
            None
        ));
        assert!(!subscription_event_matches(
            &event,
            Some("notes"),
            Some(&filter),
            None
        ));
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
            access_control: None,
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
    async fn policy_redactable_required_fields_are_nullable_in_introspection() {
        let schema = build_schema(&[policy_nullable_schema()]).expect("schema should build");
        let fields = introspected_type_fields(&schema, "PurchaseOrders").await;

        assert_nullable_scalar(&fields, "restricted_notes", "String");
        assert_non_null_scalar(&fields, "amount_cents", "Int");
        assert_non_null_scalar(&fields, "status", "String");
        assert_nullable_scalar(&fields, "memo", "String");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn collection_without_policy_preserves_required_output_fields() {
        let schema = build_schema(&[test_schema()]).expect("schema should build");
        let fields = introspected_type_fields(&schema, "Tasks").await;

        assert_non_null_scalar(&fields, "title", "String");
        assert_nullable_scalar(&fields, "status", "String");
        assert_nullable_scalar(&fields, "priority", "Int");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn policy_override_arguments_are_exposed_on_policy_queries() {
        let handler = make_handler(&[]).await;
        let schema = build_schema_with_handler(&[], handler).expect("schema should build");
        let result = schema
            .schema
            .execute(
                r#"{
                    __type(name: "Query") {
                        fields {
                            name
                            args { name type { kind name ofType { kind name } } }
                        }
                    }
                }"#,
            )
            .await;
        let data = response_data(result);
        let fields = data["__type"]["fields"].as_array().expect("fields");
        for field_name in ["effectivePolicy", "explainPolicy"] {
            let field = fields
                .iter()
                .find(|field| field["name"] == field_name)
                .unwrap_or_else(|| panic!("{field_name} query should exist"));
            let arg = field["args"]
                .as_array()
                .expect("args")
                .iter()
                .find(|arg| arg["name"] == "policyOverride")
                .unwrap_or_else(|| panic!("{field_name} should expose policyOverride"));
            assert_eq!(arg["type"]["kind"], "SCALAR");
            assert_eq!(arg["type"]["name"], "JSON");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn effective_policy_null_override_uses_active_policy_and_valid_override_applies() {
        let ts = policy_override_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;
        {
            let mut guard = handler.lock().await;
            guard
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new("policy_items"),
                    id: EntityId::new("p1"),
                    data: json!({"status": "archived", "title": "Archived"}),
                    actor: Some("setup".into()),
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("entity should be created");
        }
        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let active = schema
            .schema
            .execute(
                r#"{
                    effectivePolicy(
                        collection: "policy_items",
                        entityId: "p1",
                        policyOverride: null
                    ) { canRead policyVersion }
                }"#,
            )
            .await;
        let active = response_data(active);
        assert_eq!(active["effectivePolicy"]["canRead"], true);
        assert_eq!(active["effectivePolicy"]["policyVersion"], 1);

        let overridden = schema
            .schema
            .execute(
                r#"{
                    effectivePolicy(
                        collection: "policy_items",
                        entityId: "p1",
                        policyOverride: {
                            read: {
                                allow: [{
                                    name: "only-open",
                                    where: { field: "status", eq: "open" }
                                }]
                            }
                        }
                    ) { canRead policyVersion }
                }"#,
            )
            .await;
        let overridden = response_data(overridden);
        assert_eq!(overridden["effectivePolicy"]["canRead"], false);
        assert_eq!(overridden["effectivePolicy"]["policyVersion"], 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn explain_policy_override_flips_active_allow_to_deny() {
        let ts = policy_override_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;
        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(
                r#"{
                    explainPolicy(
                        input: {
                            operation: "read",
                            collection: "policy_items",
                            data: { status: "archived", title: "Archived" }
                        },
                        policyOverride: {
                            read: {
                                allow: [{
                                    name: "only-open",
                                    where: { field: "status", eq: "open" }
                                }]
                            }
                        }
                    ) { decision reason ruleIds policyVersion }
                }"#,
            )
            .await;
        let data = response_data(result);
        assert_eq!(data["explainPolicy"]["decision"], "deny");
        assert_eq!(data["explainPolicy"]["reason"], "row_read_denied");
        assert_eq!(data["explainPolicy"]["policyVersion"], 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn malformed_policy_override_returns_typed_diagnostic_error() {
        let ts = policy_override_schema();
        let handler = make_handler(std::slice::from_ref(&ts)).await;
        let schema =
            build_schema_with_handler(&[ts], Arc::clone(&handler)).expect("schema should build");

        let result = schema
            .schema
            .execute(
                r#"{
                    explainPolicy(
                        input: {
                            operation: "read",
                            collection: "policy_items",
                            data: { status: "open", title: "Open" }
                        },
                        policyOverride: {
                            read: {
                                allow: [{
                                    name: "broken",
                                    where: { field: "missing_field", eq: "open" }
                                }]
                            }
                        }
                    ) { decision }
                }"#,
            )
            .await;
        assert_eq!(result.errors.len(), 1, "errors: {:?}", result.errors);
        let ext = result.errors[0].extensions.as_ref().expect("extensions");
        assert!(matches!(
            ext.get("code"),
            Some(GqlValue::String(code)) if code == "invalid_policy_override"
        ));
        let diagnostics = ext
            .get("diagnostics")
            .and_then(|value| match value {
                GqlValue::List(items) => Some(items),
                _ => None,
            })
            .expect("diagnostics array");
        assert_eq!(diagnostics.len(), 1);
        let GqlValue::Object(diagnostic) = &diagnostics[0] else {
            panic!("diagnostic should be an object: {:?}", diagnostics[0]);
        };
        assert!(matches!(
            diagnostic.get("code"),
            Some(GqlValue::String(code)) if code == "invalid_policy_override"
        ));
        assert!(matches!(
            diagnostic.get("field"),
            Some(GqlValue::String(field)) if field == "missing_field"
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn procurement_fixture_can_drive_graphql_schema_and_relationships() {
        let mut seeded = AxonHandler::new(MemoryStorageAdapter::default());
        let fixture =
            seed_procurement_fixture(&mut seeded).expect("procurement fixture should seed");
        let handler = Arc::new(Mutex::new(seeded));

        let schema =
            build_schema_with_handler(&fixture.schemas, Arc::clone(&handler)).expect("schema");
        let fields = introspected_type_fields(&schema, "Invoices").await;
        assert_nullable_scalar(&fields, "amount_cents", "Int");
        assert_nullable_scalar(&fields, "commercial_terms", "String");
        assert_non_null_scalar(&fields, "number", "String");

        let query = async_graphql::Request::new(format!(
            r#"{{
                    invoices(id: "{}") {{
                        id
                        number
                        amount_cents
                        commercial_terms
                        vendor(limit: 1) {{
                            totalCount
                            edges {{ linkType node {{ id name }} }}
                        }}
                    }}
                }}"#,
            fixture.ids.under_threshold_invoice.as_str()
        ))
        .data(CallerIdentity::new(
            fixture.subjects.finance_agent,
            axon_core::auth::Role::Read,
        ));
        let result = schema.schema.execute(query).await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("result data should be JSON");
        let expected = fixture
            .entity(
                &fixture.collections.invoices,
                &fixture.ids.under_threshold_invoice,
            )
            .expect("under-threshold invoice should be fixture data");
        let invoice = &data["invoices"];
        assert_eq!(invoice["number"], expected.data["number"]);
        assert_eq!(invoice["amount_cents"], expected.data["amount_cents"]);
        assert_eq!(invoice["vendor"]["totalCount"], json!(1));
        assert_eq!(invoice["vendor"]["edges"][0]["linkType"], json!("vendor"));
        assert_eq!(
            invoice["vendor"]["edges"][0]["node"]["id"],
            json!(fixture.ids.primary_vendor.as_str())
        );
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
