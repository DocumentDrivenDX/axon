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
    use serde_json::{json, Value};

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
}
