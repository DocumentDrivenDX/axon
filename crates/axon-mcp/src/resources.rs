//! MCP resource discovery and read helpers for Axon.

use axon_api::handler::AxonHandler;
use axon_api::request::{
    DescribeCollectionRequest, GetEntityRequest, ListNamespaceCollectionsRequest,
    ListNamespacesRequest, ListNeighborsRequest, QueryAuditRequest, QueryEntitiesRequest,
    TraverseDirection,
};
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_DATABASE, DEFAULT_SCHEMA};
use axon_storage::adapter::StorageAdapter;
use serde::Serialize;
use serde_json::{json, Value};

use crate::error_mapping::map_axon_error;
use crate::protocol::McpError;

const DEFAULT_COLLECTION_LIMIT: usize = 50;
const DEFAULT_AUDIT_LIMIT: usize = 20;
const JSON_MIME_TYPE: &str = "application/json";

/// Resource handler function.
pub type ResourceHandler = Box<dyn Fn(&str) -> Result<Value, McpError> + Send + Sync>;

/// A concrete MCP resource entry returned by `resources/list`.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// An MCP resource template entry returned by `resources/templates/list`.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceTemplateInfo {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// Registry of MCP resources and templates.
pub struct ResourceRegistry {
    resources: Vec<ResourceInfo>,
    templates: Vec<ResourceTemplateInfo>,
    reader: ResourceHandler,
}

impl ResourceRegistry {
    /// Create a new resource registry.
    pub fn new(
        resources: Vec<ResourceInfo>,
        templates: Vec<ResourceTemplateInfo>,
        reader: ResourceHandler,
    ) -> Self {
        Self {
            resources,
            templates,
            reader,
        }
    }

    /// List concrete resources.
    pub fn list_resources(&self) -> Vec<ResourceInfo> {
        self.resources.clone()
    }

    /// List supported resource templates.
    pub fn list_resource_templates(&self) -> Vec<ResourceTemplateInfo> {
        self.templates.clone()
    }

    /// Read a resource by URI.
    pub fn read_resource(&self, uri: &str) -> Result<Value, McpError> {
        (self.reader)(uri)
    }
}

impl Default for ResourceRegistry {
    fn default() -> Self {
        Self::new(
            Vec::new(),
            Vec::new(),
            Box::new(|uri| Err(McpError::NotFound(format!("resource URI `{uri}`")))),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkView {
    Outbound,
    Inbound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedResource {
    Collections,
    Schemas,
    Schema {
        collection: String,
    },
    Collection {
        collection: String,
    },
    Entity {
        collection: String,
        id: String,
    },
    Links {
        collection: String,
        id: String,
        view: LinkView,
    },
    Audit {
        collection: String,
        id: String,
    },
}

fn visible_collection_name(qualified_name: &str, current_database: &str) -> String {
    let (namespace, collection) = Namespace::parse_with_database(qualified_name, current_database);
    if namespace.database != current_database {
        return namespace.qualify(&collection);
    }
    if namespace.schema == DEFAULT_SCHEMA {
        return collection;
    }
    format!("{}.{}", namespace.schema, collection)
}

/// Discover collections visible within the current database scope.
pub fn discover_collections<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    current_database: &str,
) -> Result<Vec<String>, McpError> {
    let namespaces = handler
        .list_namespaces(ListNamespacesRequest {
            database: current_database.to_string(),
        })
        .map_err(map_axon_error)?;

    let mut collections = Vec::new();
    for schema in namespaces.schemas {
        let namespace_collections = handler
            .list_namespace_collections(ListNamespaceCollectionsRequest {
                database: current_database.to_string(),
                schema: schema.clone(),
            })
            .map_err(map_axon_error)?;

        for collection in namespace_collections.collections {
            collections.push(
                match (
                    current_database == DEFAULT_DATABASE,
                    schema == DEFAULT_SCHEMA,
                ) {
                    (true, true) => collection,
                    (_, true) => collection,
                    _ => format!("{schema}.{collection}"),
                },
            );
        }
    }

    collections.sort();
    collections.dedup();
    Ok(collections)
}

/// Concrete resources advertised for the supplied collections.
pub fn resource_infos(collections: &[String]) -> Vec<ResourceInfo> {
    let mut resources = vec![
        ResourceInfo {
            uri: "axon://_collections".into(),
            name: "Collections".into(),
            description: "List visible collection metadata".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
        ResourceInfo {
            uri: "axon://_schemas".into(),
            name: "Schemas".into(),
            description: "List visible collection schemas".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
    ];

    for collection in collections {
        resources.push(ResourceInfo {
            uri: format!("axon://{collection}"),
            name: format!("{collection} collection"),
            description: format!("List entities in the `{collection}` collection"),
            mime_type: JSON_MIME_TYPE.into(),
        });
        resources.push(ResourceInfo {
            uri: format!("axon://_schemas/{collection}"),
            name: format!("{collection} schema"),
            description: format!("Read the schema for the `{collection}` collection"),
            mime_type: JSON_MIME_TYPE.into(),
        });
    }

    resources
}

/// Resource templates supported by Axon's MCP server.
pub fn resource_template_infos() -> Vec<ResourceTemplateInfo> {
    vec![
        ResourceTemplateInfo {
            uri_template: "axon://{collection}".into(),
            name: "Collection listing".into(),
            description: "Read a paginated listing for a collection".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
        ResourceTemplateInfo {
            uri_template: "axon://{collection}/{id}".into(),
            name: "Entity by ID".into(),
            description: "Read a single entity by collection and ID".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
        ResourceTemplateInfo {
            uri_template: "axon://{collection}/{id}/links".into(),
            name: "Entity outbound links".into(),
            description: "Read the outbound neighbors for an entity".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
        ResourceTemplateInfo {
            uri_template: "axon://{collection}/{id}/links/inbound".into(),
            name: "Entity inbound links".into(),
            description: "Read the inbound neighbors for an entity".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
        ResourceTemplateInfo {
            uri_template: "axon://{collection}/{id}/audit".into(),
            name: "Entity audit history".into(),
            description: "Read the audit history for an entity".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
        ResourceTemplateInfo {
            uri_template: "axon://_schemas/{collection}".into(),
            name: "Collection schema".into(),
            description: "Read the schema for a collection".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
        ResourceTemplateInfo {
            uri_template: "axon://{database}/{schema}/{collection}/{id}".into(),
            name: "Qualified entity by ID".into(),
            description: "Read a single entity with an explicit database and schema".into(),
            mime_type: JSON_MIME_TYPE.into(),
        },
    ]
}

fn qualify_collection_name(collection: &str, current_database: &str) -> CollectionId {
    CollectionId::new(Namespace::qualify_with_database(
        collection,
        current_database,
    ))
}

fn parse_resource_uri(uri: &str) -> Result<ParsedResource, McpError> {
    let Some(raw) = uri.strip_prefix("axon://") else {
        return Err(McpError::InvalidParams(format!(
            "resource URI `{uri}` must start with `axon://`"
        )));
    };
    let parts: Vec<&str> = raw
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(McpError::InvalidParams("empty resource URI".into()));
    }

    if parts[0] == "_collections" {
        return match parts.len() {
            1 => Ok(ParsedResource::Collections),
            _ => Err(McpError::InvalidParams(format!(
                "unsupported collections resource URI `{uri}`"
            ))),
        };
    }

    if parts[0] == "_schemas" {
        return match parts.len() {
            1 => Ok(ParsedResource::Schemas),
            2 => Ok(ParsedResource::Schema {
                collection: parts[1].to_string(),
            }),
            _ => Err(McpError::InvalidParams(format!(
                "unsupported schema resource URI `{uri}`"
            ))),
        };
    }

    match parts.as_slice() {
        [collection] => Ok(ParsedResource::Collection {
            collection: (*collection).to_string(),
        }),
        [collection, id] => Ok(ParsedResource::Entity {
            collection: (*collection).to_string(),
            id: (*id).to_string(),
        }),
        [collection, id, "links"] => Ok(ParsedResource::Links {
            collection: (*collection).to_string(),
            id: (*id).to_string(),
            view: LinkView::Outbound,
        }),
        [collection, id, "links", "inbound"] => Ok(ParsedResource::Links {
            collection: (*collection).to_string(),
            id: (*id).to_string(),
            view: LinkView::Inbound,
        }),
        [collection, id, "audit"] => Ok(ParsedResource::Audit {
            collection: (*collection).to_string(),
            id: (*id).to_string(),
        }),
        [database, schema, collection] => Ok(ParsedResource::Collection {
            collection: Namespace::new(*database, *schema).qualify(collection),
        }),
        [database, schema, collection, id] => Ok(ParsedResource::Entity {
            collection: Namespace::new(*database, *schema).qualify(collection),
            id: (*id).to_string(),
        }),
        [database, schema, collection, id, "links"] => Ok(ParsedResource::Links {
            collection: Namespace::new(*database, *schema).qualify(collection),
            id: (*id).to_string(),
            view: LinkView::Outbound,
        }),
        [database, schema, collection, id, "links", "inbound"] => Ok(ParsedResource::Links {
            collection: Namespace::new(*database, *schema).qualify(collection),
            id: (*id).to_string(),
            view: LinkView::Inbound,
        }),
        [database, schema, collection, id, "audit"] => Ok(ParsedResource::Audit {
            collection: Namespace::new(*database, *schema).qualify(collection),
            id: (*id).to_string(),
        }),
        _ => Err(McpError::InvalidParams(format!(
            "unsupported resource URI `{uri}`"
        ))),
    }
}

fn resource_result(uri: &str, payload: &Value) -> Result<Value, McpError> {
    let text =
        serde_json::to_string(payload).map_err(|error| McpError::Internal(error.to_string()))?;
    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": JSON_MIME_TYPE,
            "text": text,
        }],
    }))
}

/// Read an MCP resource using the live Axon handler.
pub fn read_resource_from_handler<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    current_database: &str,
    uri: &str,
) -> Result<Value, McpError> {
    match parse_resource_uri(uri)? {
        ParsedResource::Collections => {
            let collections = discover_collections(handler, current_database)?;
            let payload = json!({
                "database": current_database,
                "collections": collections,
            });
            resource_result(uri, &payload)
        }
        ParsedResource::Schemas => {
            let collections = discover_collections(handler, current_database)?;
            let mut schemas = Vec::new();
            for collection in collections {
                let collection_id = qualify_collection_name(&collection, current_database);
                if let Some(schema) = handler.get_schema(&collection_id).map_err(map_axon_error)? {
                    schemas.push(json!({
                        "collection": collection,
                        "schema": schema,
                    }));
                }
            }
            resource_result(uri, &json!({ "schemas": schemas }))
        }
        ParsedResource::Schema { collection } => {
            let collection_id = qualify_collection_name(&collection, current_database);
            let schema = handler
                .get_schema(&collection_id)
                .map_err(map_axon_error)?
                .ok_or_else(|| {
                    McpError::NotFound(format!("schema for collection `{collection}`"))
                })?;
            resource_result(uri, &json!({ "schema": schema }))
        }
        ParsedResource::Collection { collection } => {
            let collection_id = qualify_collection_name(&collection, current_database);
            let describe = handler
                .describe_collection(DescribeCollectionRequest {
                    name: collection_id.clone(),
                })
                .map_err(map_axon_error)?;
            let entities = handler
                .query_entities(QueryEntitiesRequest {
                    collection: collection_id,
                    filter: None,
                    sort: Vec::new(),
                    limit: Some(DEFAULT_COLLECTION_LIMIT),
                    after_id: None,
                    count_only: false,
                })
                .map_err(map_axon_error)?;
            let payload = json!({
                "collection": visible_collection_name(&describe.name, current_database),
                "entity_count": describe.entity_count,
                "schema_version": describe.schema.as_ref().map(|schema| schema.version),
                "entities": entities.entities,
                "total_count": entities.total_count,
                "next_cursor": entities.next_cursor,
            });
            resource_result(uri, &payload)
        }
        ParsedResource::Entity { collection, id } => {
            let response = handler
                .get_entity(GetEntityRequest {
                    collection: qualify_collection_name(&collection, current_database),
                    id: EntityId::new(&id),
                })
                .map_err(map_axon_error)?;
            resource_result(uri, &json!({ "entity": response.entity }))
        }
        ParsedResource::Links {
            collection,
            id,
            view,
        } => {
            let response = handler
                .list_neighbors(ListNeighborsRequest {
                    collection: qualify_collection_name(&collection, current_database),
                    id: EntityId::new(&id),
                    link_type: None,
                    direction: Some(match view {
                        LinkView::Outbound => TraverseDirection::Forward,
                        LinkView::Inbound => TraverseDirection::Reverse,
                    }),
                })
                .map_err(map_axon_error)?;
            resource_result(
                uri,
                &json!({
                    "direction": match view {
                        LinkView::Outbound => "outbound",
                        LinkView::Inbound => "inbound",
                    },
                    "groups": response.groups,
                    "total_count": response.total_count,
                }),
            )
        }
        ParsedResource::Audit { collection, id } => {
            let response = handler
                .query_audit(QueryAuditRequest {
                    database: Some(current_database.to_string()),
                    collection: Some(qualify_collection_name(&collection, current_database)),
                    collection_ids: Vec::new(),
                    entity_id: Some(EntityId::new(&id)),
                    actor: None,
                    operation: None,
                    since_ns: None,
                    until_ns: None,
                    after_id: None,
                    limit: Some(DEFAULT_AUDIT_LIMIT),
                })
                .map_err(map_axon_error)?;
            resource_result(
                uri,
                &json!({
                    "entries": response.entries,
                    "next_cursor": response.next_cursor,
                }),
            )
        }
    }
}
