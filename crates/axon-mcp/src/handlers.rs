//! MCP tool handlers wired to the Axon API layer.
//!
//! Each handler function creates a [`ToolDef`] that dispatches to the
//! appropriate `AxonHandler` method via a shared `Arc<Mutex<AxonHandler>>`.
//!
//! Uses `std::sync::Mutex` since all `AxonHandler` methods are synchronous.

use std::sync::{Arc, Mutex};

use async_graphql::parser::{
    parse_query,
    types::{Field as GraphQlField, OperationType, Selection, SelectionSet},
};
use async_graphql::Value as GraphQlConstValue;
use axon_api::handler::AxonHandler;
use axon_api::request::{
    AggregateFunction, AggregateRequest, CountEntitiesRequest, CreateEntityRequest,
    DeleteEntityRequest, FilterNode, FindLinkCandidatesRequest, GetEntityRequest,
    ListCollectionsRequest, ListNeighborsRequest, QueryEntitiesRequest, TransitionLifecycleRequest,
    TraverseDirection, UpdateEntityRequest,
};
use axon_core::id::{CollectionId, EntityId};
use axon_storage::adapter::StorageAdapter;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
use tokio::sync::Mutex as TokioMutex;

use crate::tools::{ToolDef, ToolError};

/// Build CRUD tools for a collection, wired to a shared handler.
///
/// Returns tool definitions for `{collection}.create`, `.get`, `.patch`, `.delete`.
pub fn build_crud_tools<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> Vec<ToolDef> {
    let col = collection.to_string();
    vec![
        build_create_tool(&col, Arc::clone(&handler)),
        build_get_tool(&col, Arc::clone(&handler)),
        build_patch_tool(&col, Arc::clone(&handler)),
        build_delete_tool(&col, handler),
    ]
}

fn lock_handler<S: StorageAdapter>(
    handler: &Mutex<AxonHandler<S>>,
) -> Result<std::sync::MutexGuard<'_, AxonHandler<S>>, ToolError> {
    handler
        .lock()
        .map_err(|e| ToolError::Internal(format!("mutex poisoned: {e}")))
}

fn text_tool_response<T: Serialize>(payload: &T) -> Result<Value, ToolError> {
    let text = serde_json::to_string(payload).map_err(|e| ToolError::Internal(e.to_string()))?;
    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": text
        }]
    }))
}

fn parse_optional_filter(value: Option<Value>) -> Result<Option<FilterNode>, ToolError> {
    value
        .map(|raw| {
            serde_json::from_value(raw)
                .map_err(|e| ToolError::InvalidArgument(format!("invalid 'filter': {e}")))
        })
        .transpose()
}

fn get_required_string(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| ToolError::InvalidArgument(format!("missing '{key}'")))
}

fn get_optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn get_optional_usize(args: &Value, key: &str) -> Result<Option<usize>, ToolError> {
    args.get(key)
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                ToolError::InvalidArgument(format!("'{key}' must be an unsigned integer"))
            })
        })
        .transpose()
        .and_then(|value| {
            value
                .map(|raw| {
                    usize::try_from(raw)
                        .map_err(|_| ToolError::InvalidArgument(format!("'{key}' is too large")))
                })
                .transpose()
        })
}

fn get_direction(direction: Option<&str>) -> Result<Option<TraverseDirection>, ToolError> {
    match direction.map(|value| value.to_ascii_lowercase()) {
        None => Ok(None),
        Some(value) if value == "both" => Ok(None),
        Some(value) if value == "outbound" || value == "forward" => {
            Ok(Some(TraverseDirection::Forward))
        }
        Some(value) if value == "inbound" || value == "reverse" => {
            Ok(Some(TraverseDirection::Reverse))
        }
        Some(other) => Err(ToolError::InvalidArgument(format!(
            "unsupported direction: {other}"
        ))),
    }
}

fn get_aggregate_function(function: &str) -> Result<Option<AggregateFunction>, ToolError> {
    match function.to_ascii_lowercase().as_str() {
        "count" => Ok(None),
        "sum" => Ok(Some(AggregateFunction::Sum)),
        "avg" => Ok(Some(AggregateFunction::Avg)),
        "min" => Ok(Some(AggregateFunction::Min)),
        "max" => Ok(Some(AggregateFunction::Max)),
        other => Err(ToolError::InvalidArgument(format!(
            "unknown aggregation function: {other}"
        ))),
    }
}

fn graphql_variables(variables: &Value) -> Result<&Map<String, Value>, ToolError> {
    variables.as_object().ok_or_else(|| {
        ToolError::InvalidArgument("'variables' must be an object when provided".into())
    })
}

fn graphql_argument_json(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<Option<Value>, ToolError> {
    let Some(argument) = field.get_argument(name) else {
        return Ok(None);
    };

    let resolved = argument.node.clone().into_const_with(|variable_name| {
        let key = variable_name.to_string();
        let value = variables.get(&key).cloned().ok_or_else(|| {
            ToolError::InvalidArgument(format!("missing GraphQL variable '${key}'"))
        })?;
        GraphQlConstValue::from_json(value).map_err(|e| {
            ToolError::InvalidArgument(format!("invalid GraphQL variable '${key}': {e}"))
        })
    })?;

    resolved
        .into_json()
        .map(Some)
        .map_err(|e| ToolError::InvalidArgument(format!("invalid GraphQL argument '{name}': {e}")))
}

fn graphql_required<T: DeserializeOwned>(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<T, ToolError> {
    let value = graphql_argument_json(field, name, variables)?
        .ok_or_else(|| ToolError::InvalidArgument(format!("missing GraphQL argument '{name}'")))?;
    serde_json::from_value(value)
        .map_err(|e| ToolError::InvalidArgument(format!("invalid GraphQL argument '{name}': {e}")))
}

fn graphql_optional<T: DeserializeOwned>(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<Option<T>, ToolError> {
    graphql_argument_json(field, name, variables)?
        .map(|value| {
            serde_json::from_value(value).map_err(|e| {
                ToolError::InvalidArgument(format!("invalid GraphQL argument '{name}': {e}"))
            })
        })
        .transpose()
}

fn graphql_optional_filter(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<Option<FilterNode>, ToolError> {
    parse_optional_filter(graphql_argument_json(field, name, variables)?)
}

fn graphql_root_field_response<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    field: &GraphQlField,
    variables: &Map<String, Value>,
) -> Result<Value, ToolError> {
    match field.name.node.as_str() {
        "collections" => {
            let response = handler
                .list_collections(ListCollectionsRequest::default())
                .map_err(to_tool_error)?;
            Ok(Value::Array(
                response
                    .collections
                    .into_iter()
                    .map(|collection| {
                        serde_json::json!({
                            "name": collection.name,
                            "entityCount": collection.entity_count,
                            "schemaVersion": collection.schema_version,
                            "createdAtNs": collection.created_at_ns,
                            "updatedAtNs": collection.updated_at_ns
                        })
                    })
                    .collect(),
            ))
        }
        "entity" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let id: String = graphql_required(field, "id", variables)?;
            let response = handler
                .get_entity(GetEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                })
                .map_err(to_tool_error)?;
            serde_json::to_value(response.entity).map_err(|e| ToolError::Internal(e.to_string()))
        }
        "entities" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let limit = graphql_optional(field, "limit", variables)?;
            let after = graphql_optional::<String>(field, "after", variables)?
                .map(|cursor| EntityId::new(&cursor));
            let response = handler
                .query_entities(QueryEntitiesRequest {
                    collection: CollectionId::new(&collection),
                    filter,
                    sort: Vec::new(),
                    limit,
                    after_id: after,
                    count_only: false,
                })
                .map_err(to_tool_error)?;

            let edges: Vec<Value> = response
                .entities
                .into_iter()
                .map(|entity| {
                    serde_json::json!({
                        "node": entity,
                        "cursor": entity.id
                    })
                })
                .collect();

            Ok(serde_json::json!({
                "edges": edges,
                "pageInfo": {
                    "hasNextPage": response.next_cursor.is_some(),
                    "endCursor": response.next_cursor
                },
                "totalCount": response.total_count
            }))
        }
        "countEntities" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let group_by = graphql_optional(field, "groupBy", variables)?;
            let response = handler
                .count_entities(CountEntitiesRequest {
                    collection: CollectionId::new(&collection),
                    filter,
                    group_by,
                })
                .map_err(to_tool_error)?;

            Ok(serde_json::json!({
                "totalCount": response.total_count,
                "groups": response.groups.into_iter().map(|group| {
                    serde_json::json!({
                        "key": group.key,
                        "count": group.count
                    })
                }).collect::<Vec<_>>()
            }))
        }
        "aggregate" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let function: String = graphql_required(field, "function", variables)?;
            let aggregate_function = get_aggregate_function(&function)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let group_by = graphql_optional(field, "groupBy", variables)?;

            if let Some(function) = aggregate_function {
                let field_name: String = graphql_required(field, "field", variables)?;
                let response = handler
                    .aggregate(AggregateRequest {
                        collection: CollectionId::new(&collection),
                        function,
                        field: field_name,
                        filter,
                        group_by,
                    })
                    .map_err(to_tool_error)?;

                Ok(serde_json::json!({
                    "results": response.results.into_iter().map(|result| {
                        serde_json::json!({
                            "key": result.key,
                            "value": result.value,
                            "count": result.count
                        })
                    }).collect::<Vec<_>>()
                }))
            } else {
                let count = handler
                    .count_entities(CountEntitiesRequest {
                        collection: CollectionId::new(&collection),
                        filter,
                        group_by,
                    })
                    .map_err(to_tool_error)?;
                let mut results: Vec<Value> = count
                    .groups
                    .into_iter()
                    .map(|group| {
                        serde_json::json!({
                            "key": group.key,
                            "value": group.count,
                            "count": group.count
                        })
                    })
                    .collect();
                if results.is_empty() {
                    results.push(serde_json::json!({
                        "key": Value::Null,
                        "value": count.total_count,
                        "count": count.total_count
                    }));
                }
                Ok(serde_json::json!({ "results": results }))
            }
        }
        "linkCandidates" => {
            let source_collection: String = graphql_required(field, "sourceCollection", variables)?;
            let source_id: String = graphql_required(field, "sourceId", variables)?;
            let link_type: String = graphql_required(field, "linkType", variables)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let limit = graphql_optional(field, "limit", variables)?;
            let response = handler
                .find_link_candidates(FindLinkCandidatesRequest {
                    source_collection: CollectionId::new(&source_collection),
                    source_id: EntityId::new(&source_id),
                    link_type,
                    filter,
                    limit,
                })
                .map_err(to_tool_error)?;

            Ok(serde_json::json!({
                "targetCollection": response.target_collection,
                "linkType": response.link_type,
                "cardinality": response.cardinality,
                "existingLinkCount": response.existing_link_count,
                "candidates": response.candidates.into_iter().map(|candidate| {
                    serde_json::json!({
                        "entity": candidate.entity,
                        "alreadyLinked": candidate.already_linked
                    })
                }).collect::<Vec<_>>()
            }))
        }
        "neighbors" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let id: String = graphql_required(field, "id", variables)?;
            let link_type = graphql_optional(field, "linkType", variables)?;
            let direction = get_direction(
                graphql_optional::<String>(field, "direction", variables)?.as_deref(),
            )?;
            let response = handler
                .list_neighbors(ListNeighborsRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                    link_type,
                    direction,
                })
                .map_err(to_tool_error)?;

            Ok(serde_json::json!({
                "groups": response.groups.into_iter().map(|group| {
                    serde_json::json!({
                        "linkType": group.link_type,
                        "direction": group.direction,
                        "entities": group.entities
                    })
                }).collect::<Vec<_>>(),
                "totalCount": response.total_count
            }))
        }
        other => Err(ToolError::InvalidArgument(format!(
            "unsupported GraphQL root field: {other}"
        ))),
    }
}

fn select_value(value: Value, selection_set: &SelectionSet) -> Result<Value, ToolError> {
    if selection_set.items.is_empty() {
        return Ok(value);
    }

    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| select_value(item, selection_set))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut selected = Map::new();
            for selection in &selection_set.items {
                let Selection::Field(field) = &selection.node else {
                    return Err(ToolError::InvalidArgument(
                        "GraphQL fragments are not supported by axon.query".into(),
                    ));
                };

                let response_key = field.node.response_key().node.to_string();
                let field_name = field.node.name.node.to_string();
                let value = map.get(&field_name).cloned().unwrap_or(Value::Null);
                let projected = select_value(value, &field.node.selection_set.node)?;
                selected.insert(response_key, projected);
            }
            Ok(Value::Object(selected))
        }
        primitive => Ok(primitive),
    }
}

fn execute_graphql_query<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    query: &str,
    variables: &Value,
) -> Result<Value, ToolError> {
    if query.trim().is_empty() {
        return Err(ToolError::InvalidArgument("empty query string".into()));
    }

    let document = parse_query(query)
        .map_err(|e| ToolError::InvalidArgument(format!("GraphQL syntax error: {e}")))?;
    let mut operations = document.operations.iter();
    let (_, operation) = operations.next().ok_or_else(|| {
        ToolError::InvalidArgument("GraphQL document must contain an operation".into())
    })?;
    if operations.next().is_some() {
        return Err(ToolError::InvalidArgument(
            "multiple GraphQL operations are not supported".into(),
        ));
    }
    if operation.node.ty != OperationType::Query {
        return Err(ToolError::InvalidArgument(format!(
            "unsupported GraphQL operation: {}",
            operation.node.ty
        )));
    }
    if !document.fragments.is_empty() {
        return Err(ToolError::InvalidArgument(
            "GraphQL fragments are not supported by axon.query".into(),
        ));
    }

    let variables = graphql_variables(variables)?;
    let mut data = Map::new();
    for selection in &operation.node.selection_set.node.items {
        let Selection::Field(field) = &selection.node else {
            return Err(ToolError::InvalidArgument(
                "GraphQL fragments are not supported by axon.query".into(),
            ));
        };
        let response_key = field.node.response_key().node.to_string();
        let value = graphql_root_field_response(handler, &field.node, variables)?;
        let projected = select_value(value, &field.node.selection_set.node)?;
        data.insert(response_key, projected);
    }

    Ok(Value::Object(data))
}

fn execute_create<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let id = args
        .get("id")
        .and_then(|value| value.as_str())
        .map(EntityId::new)
        .unwrap_or_else(EntityId::generate);
    let data = args
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let response = handler
        .create_entity(CreateEntityRequest {
            collection: CollectionId::new(collection),
            id,
            data,
            actor: Some("mcp".into()),
            audit_metadata: None,
            attribution: None,
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response.entity)
}

fn execute_get<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let id = get_required_string(args, "id")?;
    let response = handler
        .get_entity(GetEntityRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(&id),
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response.entity)
}

fn execute_patch<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let id = get_required_string(args, "id")?;
    let data = args
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let expected_version = args
        .get("expected_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'expected_version'".into()))?;

    let response = handler
        .update_entity(UpdateEntityRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(&id),
            data,
            expected_version,
            actor: Some("mcp".into()),
            audit_metadata: None,
            attribution: None,
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response.entity)
}

fn execute_delete<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let id = get_required_string(args, "id")?;
    let response = handler
        .delete_entity(DeleteEntityRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(&id),
            actor: Some("mcp".into()),
            audit_metadata: None,
            force: false,
            attribution: None,
        })
        .map_err(to_tool_error)?;
    text_tool_response(&serde_json::json!({
        "collection": response.collection,
        "id": response.id,
        "status": "deleted",
    }))
}

fn execute_query<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    args: &Value,
) -> Result<Value, ToolError> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'query' string".into()))?;
    let variables = args
        .get("variables")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let data = execute_graphql_query(handler, query, &variables)?;
    text_tool_response(&serde_json::json!({ "data": data }))
}

fn execute_aggregate<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let function_name = get_required_string(args, "function")?;
    let aggregate_function = get_aggregate_function(&function_name)?;
    let filter = parse_optional_filter(args.get("filter").cloned())?;
    let group_by = get_optional_string(args, "group_by");

    let result = if let Some(function) = aggregate_function {
        let field = get_required_string(args, "field")?;
        let response = handler
            .aggregate(AggregateRequest {
                collection: CollectionId::new(collection),
                function,
                field,
                filter,
                group_by,
            })
            .map_err(to_tool_error)?;
        serde_json::json!({ "results": response.results })
    } else {
        let response = handler
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new(collection),
                filter,
                group_by,
            })
            .map_err(to_tool_error)?;
        return text_tool_response(&response);
    };

    text_tool_response(&result)
}

fn execute_link_candidates<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let source_id = args
        .get("source_id")
        .and_then(Value::as_str)
        .or_else(|| args.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'source_id'".into()))?;
    let link_type = get_required_string(args, "link_type")?;
    let filter = parse_optional_filter(args.get("filter").cloned())?;
    let limit = get_optional_usize(args, "limit")?;

    let response = handler
        .find_link_candidates(FindLinkCandidatesRequest {
            source_collection: CollectionId::new(collection),
            source_id: EntityId::new(&source_id),
            link_type,
            filter,
            limit,
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response)
}

fn execute_neighbors<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let id = get_required_string(args, "id")?;
    let link_type = get_optional_string(args, "link_type");
    let direction = get_direction(args.get("direction").and_then(Value::as_str))?;

    let response = handler
        .list_neighbors(ListNeighborsRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(&id),
            link_type,
            direction,
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response)
}

fn build_create_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.create"),
        description: format!("Create a new entity in the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID (optional, auto-generated UUIDv7 if omitted)" },
                "data": { "type": "object", "description": "Entity data" }
            },
            "required": ["data"]
        }),
        handler: Box::new(move |args| {
            let mut guard = lock_handler(&handler)?;
            execute_create(&mut guard, &col, args)
        }),
    }
}

fn build_get_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.get"),
        description: format!("Get an entity from the {col} collection by ID"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_get(&guard, &col, args)
        }),
    }
}

fn build_patch_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.patch"),
        description: format!("Merge-patch an entity in the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "data": { "type": "object", "description": "Merge-patch data" },
                "expected_version": { "type": "integer", "description": "Expected version for OCC" }
            },
            "required": ["id", "data", "expected_version"]
        }),
        handler: Box::new(move |args| {
            let mut guard = lock_handler(&handler)?;
            execute_patch(&mut guard, &col, args)
        }),
    }
}

fn build_delete_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.delete"),
        description: format!("Delete an entity from the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let mut guard = lock_handler(&handler)?;
            execute_delete(&mut guard, &col, args)
        }),
    }
}

/// Build the `axon.query` tool for GraphQL queries via MCP.
///
/// Accepts a `query` string and optional `variables` object and executes a
/// limited GraphQL read surface against the live handler.
pub fn build_query_tool<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    ToolDef {
        name: "axon.query".into(),
        description: "Execute a live GraphQL read query against Axon. Accepts a query string and optional variables.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "GraphQL query string"
                },
                "variables": {
                    "type": "object",
                    "description": "Optional variables for the query"
                }
            },
            "required": ["query"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_query(&guard, args)
        }),
    }
}

/// Build the global `axon.transition_lifecycle` tool (FEAT-015).
///
/// Transitions an entity through a named lifecycle state machine declared in
/// its collection schema. `expected_version` is optional: when omitted, the
/// tool reads the current entity version and uses it, which is the usual
/// ergonomic mode for agent-driven callers. Supply it explicitly for strict
/// OCC-guarded transitions.
///
/// Error mapping flows through [`to_tool_error`] so that `LifecycleNotFound`
/// becomes `ToolError::NotFound` and `InvalidTransition` becomes
/// `ToolError::InvalidArgument` with the list of valid transitions embedded
/// in the message.
pub fn build_transition_lifecycle_tool<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    ToolDef {
        name: "axon.transition_lifecycle".into(),
        description: "Transition an entity through a named lifecycle state machine. \
            Returns the updated entity on success, or a structured error listing the \
            valid transitions if the requested target state is not reachable."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "collection_id": {
                    "type": "string",
                    "description": "Collection name containing the entity"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity ID to transition"
                },
                "lifecycle_name": {
                    "type": "string",
                    "description": "Name of the lifecycle declared in the collection schema"
                },
                "target_state": {
                    "type": "string",
                    "description": "The state to transition to"
                },
                "expected_version": {
                    "type": "integer",
                    "description": "Optional OCC guard — if omitted, the current entity version is used"
                }
            },
            "required": ["collection_id", "entity_id", "lifecycle_name", "target_state"]
        }),
        handler: Box::new(move |args| {
            let collection_id = get_required_string(args, "collection_id")?;
            let entity_id = get_required_string(args, "entity_id")?;
            let lifecycle_name = get_required_string(args, "lifecycle_name")?;
            let target_state = get_required_string(args, "target_state")?;

            let cid = CollectionId::new(&collection_id);
            let eid = EntityId::new(&entity_id);

            let expected_version = match args.get("expected_version") {
                Some(Value::Null) | None => {
                    // Read current version so callers can omit the OCC guard.
                    let guard = lock_handler(&handler)?;
                    let resp = guard
                        .get_entity(GetEntityRequest {
                            collection: cid.clone(),
                            id: eid.clone(),
                        })
                        .map_err(to_tool_error)?;
                    resp.entity.version
                }
                Some(v) => v.as_u64().ok_or_else(|| {
                    ToolError::InvalidArgument("'expected_version' must be a u64".into())
                })?,
            };

            let mut guard = lock_handler(&handler)?;
            let resp = guard
                .transition_lifecycle(TransitionLifecycleRequest {
                    collection_id: cid,
                    entity_id: eid,
                    lifecycle_name,
                    target_state,
                    expected_version,
                    actor: Some("mcp".into()),
                    audit_metadata: None,
                    attribution: None,
                })
                .map_err(to_tool_error)?;

            text_tool_response(&resp.entity)
        }),
    }
}

/// Build a `{collection}.aggregate` tool for MCP.
///
/// Accepts structured aggregation requests: function, field, optional filter and group_by.
pub fn build_aggregate_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.aggregate"),
        description: format!("Run an aggregation query on the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "function": {
                    "type": "string",
                    "enum": ["count", "sum", "avg", "min", "max"],
                    "description": "Aggregation function"
                },
                "field": {
                    "type": "string",
                    "description": "Field to aggregate"
                },
                "filter": {
                    "type": "object",
                    "description": "Optional filter to restrict entities"
                },
                "group_by": {
                    "type": "string",
                    "description": "Optional field to group results by"
                }
            },
            "required": ["function"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_aggregate(&guard, &col, args)
        }),
    }
}

/// Build `{collection}.link_candidates` tool.
///
/// Returns candidate entities that can be linked from the given source entity.
pub fn build_link_candidates_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.link_candidates"),
        description: format!("Find candidate target entities for a link from the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Source entity ID" },
                "link_type": { "type": "string", "description": "Link type to discover candidates for" },
                "filter": { "type": "object", "description": "Optional filter applied to candidate entities" },
                "limit": { "type": "integer", "description": "Maximum number of candidates to return" }
            },
            "required": ["id", "link_type"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_link_candidates(&guard, &col, args)
        }),
    }
}

/// Build `{collection}.neighbors` tool.
///
/// Returns linked entities for a given entity.
pub fn build_neighbors_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.neighbors"),
        description: format!("Find entities linked to a {col} entity"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "link_type": { "type": "string", "description": "Optional link type filter" },
                "direction": {
                    "type": "string",
                    "enum": ["outbound", "inbound", "both"],
                    "description": "Link direction (default: both)"
                }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_neighbors(&guard, &col, args)
        }),
    }
}

/// Build CRUD tools backed by a Tokio mutex.
pub fn build_crud_tools_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> Vec<ToolDef> {
    let col = collection.to_string();
    vec![
        ToolDef {
            name: format!("{col}.create"),
            description: format!("Create a new entity in the {col} collection"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID (optional, auto-generated UUIDv7 if omitted)" },
                    "data": { "type": "object", "description": "Entity data" }
                },
                "required": ["data"]
            }),
            handler: {
                let handler = Arc::clone(&handler);
                let col = col.clone();
                Box::new(move |args| {
                    let mut guard = handler.blocking_lock();
                    execute_create(&mut guard, &col, args)
                })
            },
        },
        ToolDef {
            name: format!("{col}.get"),
            description: format!("Get an entity from the {col} collection by ID"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" }
                },
                "required": ["id"]
            }),
            handler: {
                let handler = Arc::clone(&handler);
                let col = col.clone();
                Box::new(move |args| {
                    let guard = handler.blocking_lock();
                    execute_get(&guard, &col, args)
                })
            },
        },
        ToolDef {
            name: format!("{col}.patch"),
            description: format!("Merge-patch an entity in the {col} collection"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" },
                    "data": { "type": "object", "description": "Merge-patch data" },
                    "expected_version": { "type": "integer", "description": "Expected version for OCC" }
                },
                "required": ["id", "data", "expected_version"]
            }),
            handler: {
                let handler = Arc::clone(&handler);
                let col = col.clone();
                Box::new(move |args| {
                    let mut guard = handler.blocking_lock();
                    execute_patch(&mut guard, &col, args)
                })
            },
        },
        ToolDef {
            name: format!("{col}.delete"),
            description: format!("Delete an entity from the {col} collection"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" }
                },
                "required": ["id"]
            }),
            handler: {
                let handler = Arc::clone(&handler);
                Box::new(move |args| {
                    let mut guard = handler.blocking_lock();
                    execute_delete(&mut guard, &col, args)
                })
            },
        },
    ]
}

/// Build the `axon.query` tool backed by a Tokio mutex.
pub fn build_query_tool_tokio<S: StorageAdapter + 'static>(
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> ToolDef {
    ToolDef {
        name: "axon.query".into(),
        description: "Execute a live GraphQL read query against Axon. Accepts a query string and optional variables.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "GraphQL query string"
                },
                "variables": {
                    "type": "object",
                    "description": "Optional variables for the query"
                }
            },
            "required": ["query"]
        }),
        handler: Box::new(move |args| {
            let guard = handler.blocking_lock();
            execute_query(&guard, args)
        }),
    }
}

/// Build a collection aggregate tool backed by a Tokio mutex.
pub fn build_aggregate_tool_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.aggregate"),
        description: format!("Run an aggregation query on the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "function": {
                    "type": "string",
                    "enum": ["count", "sum", "avg", "min", "max"],
                    "description": "Aggregation function"
                },
                "field": {
                    "type": "string",
                    "description": "Field to aggregate"
                },
                "filter": {
                    "type": "object",
                    "description": "Optional filter to restrict entities"
                },
                "group_by": {
                    "type": "string",
                    "description": "Optional field to group results by"
                }
            },
            "required": ["function"]
        }),
        handler: Box::new(move |args| {
            let guard = handler.blocking_lock();
            execute_aggregate(&guard, &col, args)
        }),
    }
}

/// Build `{collection}.link_candidates` backed by a Tokio mutex.
pub fn build_link_candidates_tool_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.link_candidates"),
        description: format!("Find candidate target entities for a link from the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Source entity ID" },
                "link_type": { "type": "string", "description": "Link type to discover candidates for" },
                "filter": { "type": "object", "description": "Optional filter applied to candidate entities" },
                "limit": { "type": "integer", "description": "Maximum number of candidates to return" }
            },
            "required": ["id", "link_type"]
        }),
        handler: Box::new(move |args| {
            let translated = serde_json::json!({
                "source_id": args.get("id").cloned().unwrap_or(Value::Null),
                "link_type": args.get("link_type").cloned().unwrap_or(Value::Null),
                "filter": args.get("filter").cloned().unwrap_or(Value::Null),
                "limit": args.get("limit").cloned().unwrap_or(Value::Null),
            });
            let guard = handler.blocking_lock();
            execute_link_candidates(&guard, &col, &translated)
        }),
    }
}

/// Build `{collection}.neighbors` backed by a Tokio mutex.
pub fn build_neighbors_tool_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.neighbors"),
        description: format!("Find entities linked to a {col} entity"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "link_type": { "type": "string", "description": "Optional link type filter" },
                "direction": {
                    "type": "string",
                    "enum": ["outbound", "inbound", "both"],
                    "description": "Link direction (default: both)"
                }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let guard = handler.blocking_lock();
            execute_neighbors(&guard, &col, args)
        }),
    }
}

fn to_tool_error(err: axon_core::error::AxonError) -> ToolError {
    use axon_core::error::AxonError;
    match err {
        AxonError::NotFound(msg) => ToolError::NotFound(msg),
        AxonError::ConflictingVersion {
            expected,
            actual,
            current_entity,
        } => {
            let entity_json = current_entity
                .as_ref()
                .and_then(|e| serde_json::to_string(e).ok())
                .unwrap_or_else(|| "null".to_string());
            ToolError::Conflict(format!(
                "version conflict: expected {expected}, actual {actual}, current_entity: {entity_json}"
            ))
        }
        AxonError::InvalidArgument(msg) | AxonError::InvalidOperation(msg) => {
            ToolError::InvalidArgument(msg)
        }
        AxonError::SchemaValidation(msg) => ToolError::InvalidArgument(msg),
        AxonError::AlreadyExists(msg) => ToolError::Conflict(msg),
        AxonError::UniqueViolation { field, value } => {
            ToolError::Conflict(format!("unique violation on {field}: {value}"))
        }
        AxonError::Storage(msg) => ToolError::Internal(msg),
        AxonError::Serialization(e) => ToolError::Internal(e.to_string()),
        AxonError::LifecycleNotFound { lifecycle_name } => {
            ToolError::NotFound(format!("lifecycle not found: {lifecycle_name}"))
        }
        AxonError::InvalidTransition {
            lifecycle_name,
            current_state,
            target_state,
            valid_transitions,
        } => ToolError::InvalidArgument(format!(
            "invalid transition in lifecycle `{lifecycle_name}`: \
             cannot go from `{current_state}` to `{target_state}`; \
             valid transitions: [{}]",
            valid_transitions.join(", ")
        )),
        AxonError::LifecycleFieldMissing { field } => ToolError::InvalidArgument(format!(
            "lifecycle field `{field}` is missing from entity data"
        )),
        AxonError::LifecycleStateInvalid { field, actual } => ToolError::InvalidArgument(format!(
            "lifecycle field `{field}` has invalid value {actual}"
        )),
        AxonError::RateLimitExceeded { actor, retry_after_ms } => ToolError::InvalidArgument(format!(
            "rate limit exceeded for actor '{actor}'; retry after {retry_after_ms}ms"
        )),
        AxonError::Forbidden(msg) => ToolError::InvalidArgument(format!("forbidden: {msg}")),
        AxonError::PolicyDenied(denial) => ToolError::InvalidArgument(denial.to_string()),
        AxonError::ScopeViolation {
            actor,
            entity_id,
            filter_field,
            filter_value,
        } => ToolError::InvalidArgument(format!(
            "scope violation: actor '{actor}' denied access to entity '{entity_id}' (filter {filter_field}={filter_value})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolDef;
    use axon_api::handler::AxonHandler;
    use axon_api::request::{CreateCollectionRequest, CreateLinkRequest};
    use axon_api::test_fixtures::seed_procurement_fixture;
    use axon_schema::schema::{Cardinality, CollectionSchema, LinkTypeDef};
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::{json, Value};

    fn make_handler() -> Arc<Mutex<AxonHandler<MemoryStorageAdapter>>> {
        Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ))
    }

    fn make_graph_handler() -> Arc<Mutex<AxonHandler<MemoryStorageAdapter>>> {
        let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

        let mut tasks_schema = CollectionSchema::new(CollectionId::new("tasks"));
        tasks_schema.link_types.insert(
            "depends-on".into(),
            LinkTypeDef {
                target_collection: "tasks".into(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );

        handler
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("tasks"),
                schema: tasks_schema,
                actor: Some("test".into()),
            })
            .expect("tasks collection fixture should be created");
        handler
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("users"),
                schema: CollectionSchema::new(CollectionId::new("users")),
                actor: Some("test".into()),
            })
            .expect("users collection fixture should be created");

        for (collection, id, title, status, points) in [
            ("tasks", "t-001", "First task", "ready", 10),
            ("tasks", "t-002", "Second task", "in_progress", 20),
            ("tasks", "t-003", "Third task", "ready", 5),
        ] {
            handler
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new(collection),
                    id: EntityId::new(id),
                    data: json!({
                        "title": title,
                        "status": status,
                        "points": points
                    }),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("graph fixture entities should be created");
        }

        handler
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-001"),
                data: json!({ "title": "Owner" }),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .expect("users entity fixture should be created");

        for (source_collection, source_id, target_collection, target_id, link_type) in [
            ("tasks", "t-001", "tasks", "t-002", "depends-on"),
            ("tasks", "t-001", "tasks", "t-003", "depends-on"),
            ("users", "u-001", "tasks", "t-001", "assigned-to"),
        ] {
            handler
                .create_link(CreateLinkRequest {
                    source_collection: CollectionId::new(source_collection),
                    source_id: EntityId::new(source_id),
                    target_collection: CollectionId::new(target_collection),
                    target_id: EntityId::new(target_id),
                    link_type: link_type.into(),
                    metadata: Value::Null,
                    actor: None,
                    attribution: None,
                })
                .expect("graph fixture links should be created");
        }

        Arc::new(Mutex::new(handler))
    }

    fn parse_tool_payload(result: &Value) -> Value {
        serde_json::from_str(
            result["content"][0]["text"]
                .as_str()
                .expect("tool response should contain text content"),
        )
        .expect("tool response payload should be valid JSON")
    }

    fn invoke_tool(tool: &ToolDef, args: Value) -> Value {
        (tool.handler)(&args).expect("tool invocation should succeed")
    }

    fn invoke_tool_err(tool: &ToolDef, args: Value) -> ToolError {
        (tool.handler)(&args).expect_err("tool invocation should fail")
    }

    #[test]
    fn create_and_get_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create
        let create_tool = &tools[0];
        assert_eq!(create_tool.name, "tasks.create");
        let result = invoke_tool(
            create_tool,
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Test task"}
            }),
        );
        let entity = parse_tool_payload(&result);
        assert_eq!(entity["id"], "t-001");
        assert_eq!(entity["data"]["title"], "Test task");

        // Get
        let get_tool = &tools[1];
        assert_eq!(get_tool.name, "tasks.get");
        let result = invoke_tool(get_tool, serde_json::json!({"id": "t-001"}));
        let entity = parse_tool_payload(&result);
        assert_eq!(entity["data"]["title"], "Test task");
    }

    #[test]
    fn procurement_fixture_can_seed_mcp_handler() {
        let handler = make_handler();
        let fixture = {
            let mut guard = handler
                .lock()
                .expect("procurement fixture handler should lock");
            seed_procurement_fixture(&mut guard).expect("procurement fixture should seed")
        };

        let tools = build_crud_tools(fixture.collections.users.as_str(), Arc::clone(&handler));
        let get_tool = &tools[1];
        assert_eq!(get_tool.name, "users.get");

        let result = invoke_tool(
            get_tool,
            json!({ "id": fixture.ids.finance_agent.as_str() }),
        );
        let user = parse_tool_payload(&result);
        let expected = fixture
            .entity(&fixture.collections.users, &fixture.ids.finance_agent)
            .expect("finance agent should be fixture data");

        assert_eq!(user["data"]["user_id"], expected.data["user_id"]);
        assert_eq!(user["data"]["procurement_role"], json!("finance_agent"));
    }

    #[test]
    fn patch_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create first
        invoke_tool(
            &tools[0],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Original"}
            }),
        );

        // Patch
        let patch_tool = &tools[2];
        let result = invoke_tool(
            patch_tool,
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Updated"},
                "expected_version": 1
            }),
        );
        let entity = parse_tool_payload(&result);
        assert_eq!(entity["data"]["title"], "Updated");
        assert_eq!(entity["version"], 2);
    }

    #[test]
    fn delete_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create
        invoke_tool(
            &tools[0],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Delete me"}
            }),
        );

        // Delete
        let delete_tool = &tools[3];
        let result = invoke_tool(delete_tool, serde_json::json!({"id": "t-001"}));
        let text = result["content"][0]["text"]
            .as_str()
            .expect("delete tool should return text content");
        assert!(text.contains("t-001"));

        // Verify deleted
        let get_result = (tools[1].handler)(&serde_json::json!({"id": "t-001"}));
        assert!(get_result.is_err());
    }

    #[test]
    fn version_conflict_returns_current_entity() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        // Create
        invoke_tool(
            &tools[0],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "V1"}
            }),
        );

        // Patch with wrong version
        let err = invoke_tool_err(
            &tools[2],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "V2"},
                "expected_version": 99
            }),
        );

        match err {
            ToolError::Conflict(msg) => {
                assert!(msg.contains("version conflict"));
                assert!(msg.contains("current_entity"));
            }
            other => panic!("expected Conflict, got: {other:?}"),
        }
    }

    #[test]
    fn missing_id_returns_invalid_argument() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler));

        let err = invoke_tool_err(&tools[1], serde_json::json!({}));
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── axon.query tool tests ──────────────────────────────────────────

    #[test]
    fn query_tool_executes_live_handler_queries() {
        let handler = make_graph_handler();
        let tool = build_query_tool(Arc::clone(&handler));
        assert_eq!(tool.name, "axon.query");
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "query": r"query($collection: String!, $id: String!) {
                collections { name entityCount }
                entity(collection: $collection, id: $id) { id data }
                entities(collection: $collection, limit: 2) {
                    totalCount
                    pageInfo { hasNextPage endCursor }
                    edges { cursor node { id data } }
                }
            }",
                "variables": {
                    "collection": "tasks",
                    "id": "t-001"
                }
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["data"]["collections"][0]["name"], "tasks");
        assert_eq!(parsed["data"]["collections"][0]["entityCount"], 3);
        assert_eq!(parsed["data"]["entity"]["id"], "t-001");
        assert_eq!(parsed["data"]["entity"]["data"]["title"], "First task");
        assert_eq!(parsed["data"]["entities"]["totalCount"], 3);
        assert_eq!(
            parsed["data"]["entities"]["edges"][0]["node"]["id"],
            "t-001"
        );
    }

    #[test]
    fn query_tool_rejects_invalid_graphql_syntax() {
        let tool = build_query_tool(make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "query": "{ collections { name }"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_unsupported_root_fields() {
        let tool = build_query_tool(make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "query": "{ tasks { id } }"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_mutations_until_graphql_writes_exist() {
        let tool = build_query_tool(make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "query": "mutation { collections { name } }"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_missing_query() {
        let tool = build_query_tool(make_graph_handler());
        let err = invoke_tool_err(&tool, serde_json::json!({}));
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── aggregate tool tests ───────────────────────────────────────────

    #[test]
    fn aggregate_tool_returns_live_count_results() {
        let handler = make_graph_handler();
        let tool = build_aggregate_tool("tasks", Arc::clone(&handler));
        assert_eq!(tool.name, "tasks.aggregate");
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "function": "count",
                "group_by": "status"
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["total_count"], 3);
        assert_eq!(
            parsed["groups"]
                .as_array()
                .expect("aggregate groups should be an array")
                .len(),
            2
        );
    }

    #[test]
    fn aggregate_tool_returns_live_numeric_aggregates() {
        let handler = make_graph_handler();
        let tool = build_aggregate_tool("tasks", Arc::clone(&handler));
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "function": "sum",
                "field": "points"
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["results"][0]["value"], 35.0);
        assert_eq!(parsed["results"][0]["count"], 3);
    }

    #[test]
    fn aggregate_tool_rejects_unknown_function() {
        let tool = build_aggregate_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "function": "median",
                "field": "x"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn aggregate_tool_requires_function() {
        let tool = build_aggregate_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(&tool, serde_json::json!({"field": "x"}));
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── link tools tests ───────────────────────────────────────────────

    #[test]
    fn link_candidates_tool_returns_live_candidates() {
        let handler = make_graph_handler();
        let tool = build_link_candidates_tool("tasks", Arc::clone(&handler));
        assert_eq!(tool.name, "tasks.link_candidates");
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "id": "t-001",
                "link_type": "depends-on"
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["target_collection"], "tasks");
        assert_eq!(parsed["existing_link_count"], 2);
        let already_linked = parsed["candidates"]
            .as_array()
            .expect("link candidate payload should include a candidates array")
            .iter()
            .find(|candidate| candidate["entity"]["id"] == "t-002")
            .expect("existing linked entity should appear in the candidates list");
        assert!(already_linked["already_linked"]
            .as_bool()
            .expect("candidate payload should include already_linked"));
    }

    #[test]
    fn link_candidates_tool_maps_not_found_errors() {
        let tool = build_link_candidates_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "id": "ghost",
                "link_type": "depends-on"
            }),
        );
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn neighbors_tool_returns_live_neighbors() {
        let handler = make_graph_handler();
        let tool = build_neighbors_tool("tasks", Arc::clone(&handler));
        assert_eq!(tool.name, "tasks.neighbors");
        let result = invoke_tool(&tool, serde_json::json!({"id": "t-001"}));
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["total_count"], 3);
        assert_eq!(
            parsed["groups"]
                .as_array()
                .expect("neighbors payload should include grouped results")
                .len(),
            2
        );
    }

    #[test]
    fn neighbors_tool_rejects_invalid_direction() {
        let tool = build_neighbors_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "id": "t-001",
                "direction": "sideways"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
