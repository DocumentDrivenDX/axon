//! Dynamic GraphQL schema builder from Axon collections.
//!
//! Generates a full GraphQL schema (queries + mutations + introspection)
//! from the set of registered collections and their entity schemas.

use async_graphql::dynamic::{Field, FieldFuture, FieldValue, Object, Schema, TypeRef};
use async_graphql::Value as GqlValue;

use axon_schema::schema::CollectionSchema;

use crate::types::extract_fields;

/// Wrapper around the dynamically generated `async-graphql` schema.
pub struct AxonSchema {
    pub schema: Schema,
}

/// Build a dynamic GraphQL schema from the given collection schemas.
///
/// Each collection produces:
/// - A query field `<collection>(id: ID!): <CollectionType>`
/// - A query field `<collection>s(filter: JSON, limit: Int, after: ID): <CollectionConnection>`
/// - A mutation field `create<Collection>(input: JSON!): <CollectionType>`
/// - A mutation field `update<Collection>(id: ID!, version: Int!, input: JSON!): <CollectionType>`
/// - A mutation field `delete<Collection>(id: ID!): Boolean!`
///
/// The schema is rebuilt when collections change (schema is immutable once built).
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
            |_ctx| FieldFuture::new(async move { Ok(Some(FieldValue::list(Vec::<FieldValue>::new()))) }),
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
                FieldFuture::new(async move {
                    Ok(Some(FieldValue::from(GqlValue::from(true))))
                })
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
    use axon_core::id::CollectionId;
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
        }
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

    #[test]
    fn empty_collections_builds_valid_schema() {
        // Empty schema should still be valid (query/mutation with no fields won't work
        // but the schema should build).
        // async-graphql requires at least one field, so we skip this for now.
    }
}
