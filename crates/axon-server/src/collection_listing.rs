use axon_api::handler::AxonHandler;
use axon_api::request::{
    DescribeCollectionRequest, ListNamespaceCollectionsRequest, ListNamespacesRequest,
};
use axon_api::response::CollectionMetadata;
use axon_audit::AuditEntry;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, Namespace, DEFAULT_DATABASE, DEFAULT_SCHEMA};
use axon_storage::adapter::StorageAdapter;

pub fn list_collections_for_database<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    database: &str,
) -> Result<Vec<CollectionMetadata>, AxonError> {
    let namespaces = handler.list_namespaces(ListNamespacesRequest {
        database: database.to_string(),
    })?;

    let mut collections = Vec::new();
    for schema in namespaces.schemas {
        let namespace_collections =
            handler.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: database.to_string(),
                schema: schema.clone(),
            })?;

        for collection in namespace_collections.collections {
            let description = handler.describe_collection(DescribeCollectionRequest {
                name: scoped_collection_id(database, &schema, &collection),
            })?;
            collections.push(CollectionMetadata {
                name: description.name,
                entity_count: description.entity_count,
                schema_version: description.schema.as_ref().map(|schema| schema.version),
                created_at_ns: description.created_at_ns,
                updated_at_ns: description.updated_at_ns,
            });
        }
    }

    collections.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(collections)
}

pub fn collection_belongs_to_database(name: &str, database: &str) -> bool {
    let (namespace, _) = Namespace::parse(name);
    namespace.database == database
}

pub fn filter_audit_entries_to_database(
    entries: Vec<AuditEntry>,
    database: Option<&str>,
) -> Vec<AuditEntry> {
    match database {
        Some(database) => entries
            .into_iter()
            .filter(|entry| collection_belongs_to_database(entry.collection.as_str(), database))
            .collect(),
        None => entries,
    }
}

fn scoped_collection_id(database: &str, schema: &str, collection: &str) -> CollectionId {
    match (database == DEFAULT_DATABASE, schema == DEFAULT_SCHEMA) {
        (true, true) => CollectionId::new(collection),
        (true, false) => CollectionId::new(format!("{schema}.{collection}")),
        (false, _) => CollectionId::new(Namespace::new(database, schema).qualify(collection)),
    }
}
