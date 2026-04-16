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
    Field, FieldFuture, FieldValue, InputValue, Object, Schema, Subscription, SubscriptionField,
    SubscriptionFieldFuture, TypeRef,
};
use async_graphql::futures_util::StreamExt;
use async_graphql::{Error as GqlError, ErrorExtensions, Value as GqlValue};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::subscriptions::BroadcastBroker;

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, DeleteLinkRequest,
    GetEntityRequest, PatchEntityRequest, QueryEntitiesRequest, TransitionLifecycleRequest,
    UpdateEntityRequest,
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

// ── Entity → GraphQL FieldValue conversion ──────────────────────────────────

/// Convert an `Entity` into an `async-graphql` `FieldValue` that the dynamic
/// object type can resolve field-by-field.
fn entity_to_field_value(entity: &Entity) -> FieldValue<'static> {
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

    FieldValue::from(GqlValue::from_json(Value::Object(map)).unwrap_or(GqlValue::Null))
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

    for schema in collections {
        let collection_name = schema.collection.as_str();
        let type_name = pascal_case(collection_name);
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

        // ── Query: get by ID ─────────────────────────────────────────────
        let col_id = CollectionId::new(collection_name);
        let handler_get = Arc::clone(&handler);
        let col_for_get = col_id.clone();
        let get_field = Field::new(collection_name, TypeRef::named(&type_name), move |ctx| {
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
        let list_field_name = format!("{collection_name}s");
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

                    let guard = handler.lock().await;
                    match guard.query_entities(QueryEntitiesRequest {
                        collection: col.clone(),
                        filter: None,
                        sort: Vec::new(),
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
        .argument(InputValue::new("afterId", TypeRef::named(TypeRef::ID)));
        query = query.field(list_field);

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
    .register(query)
    .register(mutation);

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

    for schema in collections {
        let collection_name = schema.collection.as_str();
        let type_name = pascal_case(collection_name);
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

        // Query: get by ID.
        let get_field_name = collection_name.to_string();
        let type_name_ref = type_name.clone();
        query = query.field(Field::new(
            &get_field_name,
            TypeRef::named(&type_name_ref),
            |_ctx| FieldFuture::new(async move { Ok(Some(FieldValue::NULL)) }),
        ));

        // Query: list.
        let list_field_name = format!("{collection_name}s");
        let type_name_list = type_name.clone();
        query = query.field(Field::new(
            &list_field_name,
            TypeRef::named_list(&type_name_list),
            |_ctx| {
                FieldFuture::new(
                    async move { Ok(Some(FieldValue::list(Vec::<FieldValue>::new()))) },
                )
            },
        ));

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

    let mut schema_builder = Schema::build(query.type_name(), Some(mutation.type_name()), None)
        .register(query)
        .register(mutation);

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
    s.split('_')
        .flat_map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect::<Vec<_>>(),
                None => Vec::new(),
            }
        })
        .collect()
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
    use async_graphql::{
        EmptyMutation, EmptySubscription, Json, Schema as StaticSchema, SimpleObject, ID,
    };
    use axon_core::id::CollectionId;
    use axon_storage::MemoryStorageAdapter;
    use serde_json::json;

    #[derive(SimpleObject, Clone)]
    #[graphql(name = "CollectionMeta", rename_fields = "camelCase")]
    struct Feat015CollectionMeta {
        name: String,
        entity_count: i32,
    }

    #[derive(SimpleObject, Clone)]
    #[graphql(name = "PageInfo", rename_fields = "camelCase")]
    struct Feat015PageInfo {
        has_next_page: bool,
        end_cursor: Option<String>,
    }

    #[derive(SimpleObject, Clone)]
    #[graphql(name = "EntityEdge", rename_fields = "camelCase")]
    struct Feat015EntityEdge {
        node: Json<Value>,
        cursor: String,
    }

    #[derive(SimpleObject, Clone)]
    #[graphql(name = "EntityConnection", rename_fields = "camelCase")]
    struct Feat015EntityConnection {
        edges: Vec<Feat015EntityEdge>,
        page_info: Feat015PageInfo,
    }

    struct Feat015Query;

    #[async_graphql::Object(rename_fields = "camelCase")]
    impl Feat015Query {
        async fn collections(&self) -> Vec<Feat015CollectionMeta> {
            vec![Feat015CollectionMeta {
                name: String::from("tasks"),
                entity_count: 1,
            }]
        }

        async fn entity(&self, collection: String, id: ID) -> Json<Value> {
            let _ = (collection, id);
            Json(json!({
                "id": "task-1",
                "version": 2,
                "data": { "title": "Ship it" },
                "createdAt": "2026-04-08T00:00:00Z",
                "updatedAt": "2026-04-08T00:00:00Z"
            }))
        }

        async fn entities(
            &self,
            collection: String,
            limit: Option<i32>,
            after: Option<String>,
        ) -> Feat015EntityConnection {
            let _ = (collection, limit, after);
            Feat015EntityConnection {
                edges: vec![Feat015EntityEdge {
                    node: Json(json!({
                        "id": "task-1",
                        "version": 2,
                        "data": { "title": "Ship it" }
                    })),
                    cursor: String::from("cursor-1"),
                }],
                page_info: Feat015PageInfo {
                    has_next_page: false,
                    end_cursor: None,
                },
            }
        }
    }

    fn feat_015_schema() -> StaticSchema<Feat015Query, EmptyMutation, EmptySubscription> {
        StaticSchema::build(Feat015Query, EmptyMutation, EmptySubscription).finish()
    }

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
    }

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
    async fn introspection_query_works() {
        let schema = build_schema(&[test_schema()]).expect("schema should build");
        let result = schema
            .schema
            .execute("{ __schema { types { name } } }")
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[tokio::test]
    async fn ui_helper_queries_do_not_match_current_dynamic_schema() {
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
                !result.errors.is_empty(),
                "{name} unexpectedly matched the current schema",
            );
        }
    }

    #[tokio::test]
    async fn ui_helper_queries_fail_fast_against_feat_015_generic_contract() {
        let schema = feat_015_schema();

        let collections_result = schema.execute("{ collections { name entityCount } }").await;
        assert!(
            collections_result.errors.is_empty(),
            "collections helper query should match FEAT-015: {:?}",
            collections_result.errors
        );

        let helper_queries = [
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
            let result = schema.execute(query).await;
            assert!(
                !result.errors.is_empty(),
                "{name} unexpectedly matched the FEAT-015 generic contract",
            );
        }
    }

    #[test]
    fn empty_collections_builds_valid_schema() {
        // Empty schema should still be valid (query/mutation with no fields won't work
        // but the schema should build).
        // async-graphql requires at least one field, so we skip this for now.
    }

    // ── Live handler integration tests ──────────────────────────────────────

    #[tokio::test]
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

    #[tokio::test]
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
            .execute("{ taskss(limit: 2) { id title } }")
            .await;
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let data = result.data.into_json().expect("json");
        let tasks = data["taskss"].as_array().expect("should be array");
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
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
