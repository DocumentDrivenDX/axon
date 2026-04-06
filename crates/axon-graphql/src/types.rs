//! Type mapping from ESF (Entity Schema Format) to GraphQL types.

use axon_schema::schema::CollectionSchema;

/// Map an ESF JSON Schema type to a GraphQL type name.
///
/// Returns the GraphQL scalar type name for a JSON Schema `type` value.
pub fn json_schema_type_to_graphql(json_type: &str) -> &'static str {
    match json_type {
        "string" => "String",
        "integer" => "Int",
        "number" => "Float",
        "boolean" => "Boolean",
        "array" => "JSON",  // fallback for complex arrays
        "object" => "JSON", // fallback for nested objects
        _ => "JSON",
    }
}

/// Extract field names and their GraphQL types from a collection schema.
///
/// Returns `(field_name, graphql_type, is_required)` tuples.
pub fn extract_fields(schema: &CollectionSchema) -> Vec<(String, String, bool)> {
    let mut fields = vec![
        ("id".into(), "ID!".into(), true),
        ("version".into(), "Int!".into(), true),
        ("createdAt".into(), "String".into(), false),
        ("updatedAt".into(), "String".into(), false),
    ];

    // Extract fields from entity_schema if present.
    if let Some(ref entity_schema) = schema.entity_schema {
        if let Some(properties) = entity_schema.get("properties") {
            if let Some(props) = properties.as_object() {
                let required: Vec<String> = entity_schema
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                for (name, prop_schema) in props {
                    let gql_type = prop_schema
                        .get("type")
                        .and_then(|t| t.as_str())
                        .map(json_schema_type_to_graphql)
                        .unwrap_or("JSON");
                    let is_required = required.contains(name);
                    fields.push((name.clone(), gql_type.to_string(), is_required));
                }
            }
        }
    }

    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::CollectionId;
    use serde_json::json;

    #[test]
    fn extract_fields_from_schema() {
        let schema = CollectionSchema {
            collection: CollectionId::new("tasks"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title", "status"],
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
        };

        let fields = extract_fields(&schema);
        // System fields + 3 user fields.
        assert!(fields.len() >= 7);

        let title = fields.iter().find(|(n, _, _)| n == "title").unwrap();
        assert_eq!(title.1, "String");
        assert!(title.2); // required

        let priority = fields.iter().find(|(n, _, _)| n == "priority").unwrap();
        assert_eq!(priority.1, "Int");
        assert!(!priority.2); // not required
    }

    #[test]
    fn json_type_mapping() {
        assert_eq!(json_schema_type_to_graphql("string"), "String");
        assert_eq!(json_schema_type_to_graphql("integer"), "Int");
        assert_eq!(json_schema_type_to_graphql("number"), "Float");
        assert_eq!(json_schema_type_to_graphql("boolean"), "Boolean");
        assert_eq!(json_schema_type_to_graphql("object"), "JSON");
    }
}
