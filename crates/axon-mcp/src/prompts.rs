//! MCP prompt discovery and prompt generation helpers for Axon.

use axon_api::handler::AxonHandler;
use axon_api::request::{GetEntityRequest, QueryAuditRequest, QueryEntitiesRequest};
use axon_core::id::{CollectionId, EntityId, Namespace};
use axon_storage::adapter::StorageAdapter;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::error_mapping::map_axon_error;
use crate::protocol::McpError;

const DEFAULT_PROMPT_LIMIT: usize = 20;
const DEFAULT_SAMPLE_LIMIT: usize = 5;

/// Prompt handler function.
pub type PromptHandler = Box<dyn Fn(&str, &Value) -> Result<Value, McpError> + Send + Sync>;

/// A prompt argument definition returned by `prompts/list`.
#[derive(Debug, Clone, Serialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// A prompt entry returned by `prompts/list`.
#[derive(Debug, Clone, Serialize)]
pub struct PromptInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<PromptArgument>,
}

/// Registry of MCP prompts.
pub struct PromptRegistry {
    prompts: Vec<PromptInfo>,
    handler: PromptHandler,
}

impl PromptRegistry {
    /// Create a new prompt registry.
    pub fn new(prompts: Vec<PromptInfo>, handler: PromptHandler) -> Self {
        Self { prompts, handler }
    }

    /// List available prompts.
    pub fn list_prompts(&self) -> Vec<PromptInfo> {
        self.prompts.clone()
    }

    /// Materialize a prompt by name and arguments.
    pub fn get_prompt(&self, name: &str, arguments: &Value) -> Result<Value, McpError> {
        (self.handler)(name, arguments)
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new(
            Vec::new(),
            Box::new(|name, _arguments| Err(McpError::NotFound(format!("prompt `{name}`")))),
        )
    }
}

fn qualify_collection_name(collection: &str, current_database: &str) -> CollectionId {
    CollectionId::new(Namespace::qualify_with_database(
        collection,
        current_database,
    ))
}

fn arguments_object(arguments: &Value) -> Result<&Map<String, Value>, McpError> {
    arguments.as_object().ok_or_else(|| {
        McpError::InvalidParams("prompt arguments must be an object when provided".into())
    })
}

fn required_string(arguments: &Map<String, Value>, key: &str) -> Result<String, McpError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| McpError::InvalidParams(format!("missing required prompt argument `{key}`")))
}

fn optional_string(arguments: &Map<String, Value>, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn optional_limit(
    arguments: &Map<String, Value>,
    key: &str,
    default: usize,
) -> Result<usize, McpError> {
    match arguments.get(key) {
        None => Ok(default),
        Some(value) => {
            let raw = value.as_u64().ok_or_else(|| {
                McpError::InvalidParams(format!(
                    "prompt argument `{key}` must be an unsigned integer"
                ))
            })?;
            usize::try_from(raw).map_err(|_| {
                McpError::InvalidParams(format!("prompt argument `{key}` is too large"))
            })
        }
    }
}

fn prompt_text_result(description: &str, text: String) -> Value {
    serde_json::json!({
        "description": description,
        "messages": [{
            "role": "user",
            "content": {
                "type": "text",
                "text": text,
            }
        }],
    })
}

/// Prompt definitions supported by Axon's MCP server.
pub fn prompt_infos() -> Vec<PromptInfo> {
    vec![
        PromptInfo {
            name: "axon.explore_collection".into(),
            title: "Explore Collection".into(),
            description: "Inspect a collection schema and sample entities to understand its purpose and shape".into(),
            arguments: vec![PromptArgument {
                name: "collection".into(),
                description: "Collection name to inspect".into(),
                required: true,
            }],
        },
        PromptInfo {
            name: "axon.dependency_analysis".into(),
            title: "Dependency Analysis".into(),
            description: "Analyze a specific entity and its neighbors to understand blocking relationships".into(),
            arguments: vec![
                PromptArgument {
                    name: "collection".into(),
                    description: "Collection containing the entity".into(),
                    required: true,
                },
                PromptArgument {
                    name: "id".into(),
                    description: "Entity ID to analyze".into(),
                    required: true,
                },
                PromptArgument {
                    name: "link_type".into(),
                    description: "Optional link type to emphasize during analysis".into(),
                    required: false,
                },
            ],
        },
        PromptInfo {
            name: "axon.audit_review".into(),
            title: "Audit Review".into(),
            description: "Summarize recent audit activity for a collection or entity".into(),
            arguments: vec![
                PromptArgument {
                    name: "collection".into(),
                    description: "Collection to review".into(),
                    required: true,
                },
                PromptArgument {
                    name: "id".into(),
                    description: "Optional entity ID for entity-scoped review".into(),
                    required: false,
                },
                PromptArgument {
                    name: "limit".into(),
                    description: "Maximum audit entries to include".into(),
                    required: false,
                },
            ],
        },
        PromptInfo {
            name: "axon.schema_review".into(),
            title: "Schema Review".into(),
            description: "Review a collection schema for field quality, constraints, and missing coverage".into(),
            arguments: vec![PromptArgument {
                name: "collection".into(),
                description: "Collection whose schema should be reviewed".into(),
                required: true,
            }],
        },
    ]
}

/// Materialize a prompt from the live Axon handler.
pub fn get_prompt_from_handler<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    current_database: &str,
    name: &str,
    arguments: &Value,
) -> Result<Value, McpError> {
    let arguments = arguments_object(arguments)?;

    match name {
        "axon.explore_collection" => {
            let collection = required_string(arguments, "collection")?;
            let collection_id = qualify_collection_name(&collection, current_database);
            let schema = handler
                .get_schema(&collection_id)
                .map_err(map_axon_error)?
                .ok_or_else(|| {
                    McpError::NotFound(format!("schema for collection `{collection}`"))
                })?;
            let samples = handler
                .query_entities(QueryEntitiesRequest {
                    collection: collection_id,
                    filter: None,
                    sort: Vec::new(),
                    limit: Some(DEFAULT_SAMPLE_LIMIT),
                    after_id: None,
                    count_only: false,
                })
                .map_err(map_axon_error)?;

            Ok(prompt_text_result(
                "Explore an Axon collection using its schema and sample entities",
                format!(
                    "Review the `{collection}` collection. Summarize its purpose, key fields, likely workflows, and any data quality risks.\n\nSchema:\n{}\n\nSample entities:\n{}",
                    serde_json::to_string_pretty(&schema)
                        .map_err(|error| McpError::Internal(error.to_string()))?,
                    serde_json::to_string_pretty(&samples.entities)
                        .map_err(|error| McpError::Internal(error.to_string()))?,
                ),
            ))
        }
        "axon.dependency_analysis" => {
            let collection = required_string(arguments, "collection")?;
            let id = required_string(arguments, "id")?;
            let collection_id = qualify_collection_name(&collection, current_database);
            let entity = handler
                .get_entity(GetEntityRequest {
                    collection: collection_id.clone(),
                    id: EntityId::new(&id),
                })
                .map_err(map_axon_error)?;
            let neighbors = handler
                .list_neighbors(axon_api::request::ListNeighborsRequest {
                    collection: collection_id,
                    id: EntityId::new(&id),
                    link_type: optional_string(arguments, "link_type"),
                    direction: None,
                })
                .map_err(map_axon_error)?;

            Ok(prompt_text_result(
                "Analyze dependencies around a specific Axon entity",
                format!(
                    "Analyze the dependency graph for `{collection}/{id}`. Identify direct dependencies, reverse dependencies, likely blockers, and recommended next actions.\n\nEntity:\n{}\n\nNeighbors:\n{}",
                    serde_json::to_string_pretty(&entity.entity)
                        .map_err(|error| McpError::Internal(error.to_string()))?,
                    serde_json::to_string_pretty(&neighbors)
                        .map_err(|error| McpError::Internal(error.to_string()))?,
                ),
            ))
        }
        "axon.audit_review" => {
            let collection = required_string(arguments, "collection")?;
            let limit = optional_limit(arguments, "limit", DEFAULT_PROMPT_LIMIT)?;
            let response = handler
                .query_audit(QueryAuditRequest {
                    database: Some(current_database.to_string()),
                    collection: Some(qualify_collection_name(&collection, current_database)),
                    collection_ids: Vec::new(),
                    entity_id: optional_string(arguments, "id").map(|id| EntityId::new(&id)),
                    actor: None,
                    operation: None,
                    intent_id: None,
                    approval_id: None,
                    since_ns: None,
                    until_ns: None,
                    after_id: None,
                    limit: Some(limit),
                })
                .map_err(map_axon_error)?;

            Ok(prompt_text_result(
                "Review recent Axon audit activity",
                format!(
                    "Summarize the recent audit activity for `{collection}`{}.\nFlag important mutations, actor patterns, and anything unusual.\n\nAudit entries:\n{}",
                    optional_string(arguments, "id")
                        .map(|id| format!("/{id}"))
                        .unwrap_or_default(),
                    serde_json::to_string_pretty(&response.entries)
                        .map_err(|error| McpError::Internal(error.to_string()))?,
                ),
            ))
        }
        "axon.schema_review" => {
            let collection = required_string(arguments, "collection")?;
            let collection_id = qualify_collection_name(&collection, current_database);
            let schema = handler
                .get_schema(&collection_id)
                .map_err(map_axon_error)?
                .ok_or_else(|| {
                    McpError::NotFound(format!("schema for collection `{collection}`"))
                })?;

            Ok(prompt_text_result(
                "Review an Axon collection schema",
                format!(
                    "Review the `{collection}` schema. Comment on field quality, validation coverage, lifecycle completeness, and index strategy. Suggest concrete improvements where helpful.\n\nSchema:\n{}",
                    serde_json::to_string_pretty(&schema)
                        .map_err(|error| McpError::Internal(error.to_string()))?,
                ),
            ))
        }
        other => Err(McpError::NotFound(format!("prompt `{other}`"))),
    }
}
