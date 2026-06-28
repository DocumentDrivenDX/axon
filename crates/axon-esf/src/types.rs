use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, DeserializeOwned};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// Value type for an ESF Layer 4 index field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexType {
    String,
    Integer,
    Float,
    Datetime,
    Boolean,
}

impl fmt::Display for IndexType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Integer => write!(f, "integer"),
            Self::Float => write!(f, "float"),
            Self::Datetime => write!(f, "datetime"),
            Self::Boolean => write!(f, "boolean"),
        }
    }
}

/// A single-field secondary index declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    /// JSON field path to index.
    pub field: String,
    /// Value type for this index.
    #[serde(rename = "type")]
    pub index_type: IndexType,
    /// Whether the indexed value must be unique within a collection.
    #[serde(default)]
    pub unique: bool,
}

/// A single field within a compound index declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexField {
    /// JSON field path to index.
    pub field: String,
    /// Value type for this index field.
    #[serde(rename = "type")]
    pub index_type: IndexType,
}

/// A compound secondary index declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexDef {
    /// Ordered list of indexed fields.
    pub fields: Vec<CompoundIndexField>,
    /// Whether the compound key must be unique within a collection.
    #[serde(default)]
    pub unique: bool,
}

/// A unified ESF `indexes:` declaration, accepting single and compound forms.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum IndexDeclaration {
    Single(IndexDef),
    Compound(CompoundIndexDef),
}

impl<'de> Deserialize<'de> for IndexDeclaration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Object(map) = value else {
            return Err(de::Error::custom("index declaration must be an object"));
        };

        match (map.contains_key("field"), map.contains_key("fields")) {
            (true, false) => {
                reject_unknown_keys::<D::Error>(
                    &map,
                    &["field", "type", "unique"],
                    "index declaration",
                )?;
                deserialize_from_map(map).map(Self::Single)
            }
            (false, true) => {
                reject_unknown_keys::<D::Error>(&map, &["fields", "unique"], "index declaration")?;
                deserialize_from_map(map).map(Self::Compound)
            }
            (true, true) => Err(de::Error::custom(
                "index declaration cannot contain both 'field' and 'fields'",
            )),
            (false, false) => Err(de::Error::custom(
                "index declaration must contain either 'field' or 'fields'",
            )),
        }
    }
}

fn reject_unknown_keys<E>(
    map: &Map<String, Value>,
    allowed: &'static [&'static str],
    type_name: &str,
) -> Result<(), E>
where
    E: de::Error,
{
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(de::Error::unknown_field(key, allowed));
    }
    if map.is_empty() {
        return Err(de::Error::custom(format!("{type_name} cannot be empty")));
    }
    Ok(())
}

fn deserialize_from_map<T, E>(map: Map<String, Value>) -> Result<T, E>
where
    T: DeserializeOwned,
    E: de::Error,
{
    serde_json::from_value(Value::Object(map)).map_err(de::Error::custom)
}

/// Minimal entity-schema carrier for ESF consumers that do not need Axon internals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySchemaDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

/// Minimal ESF document carrier for external consumers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EsfCoreDocument {
    pub esf_version: String,
    pub collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn index_type_serde_and_display_are_lowercase_value_kinds() {
        let cases = [
            (IndexType::String, "string"),
            (IndexType::Integer, "integer"),
            (IndexType::Float, "float"),
            (IndexType::Datetime, "datetime"),
            (IndexType::Boolean, "boolean"),
        ];

        for (index_type, expected) in cases {
            assert_eq!(index_type.to_string(), expected);
            assert_eq!(serde_json::to_value(&index_type).unwrap(), json!(expected));
            assert_eq!(
                serde_json::from_value::<IndexType>(json!(expected)).unwrap(),
                index_type
            );
        }
    }

    #[test]
    fn index_def_uses_type_rename_and_unique_default() {
        let index: IndexDef =
            serde_json::from_value(json!({"field": "status", "type": "string"})).unwrap();

        assert_eq!(index.field, "status");
        assert_eq!(index.index_type, IndexType::String);
        assert!(!index.unique);
        assert_eq!(
            serde_json::to_value(&index).unwrap(),
            json!({"field": "status", "type": "string", "unique": false})
        );
    }

    #[test]
    fn compound_index_uses_type_rename_and_unique_default() {
        let index: CompoundIndexDef = serde_json::from_value(json!({
            "fields": [
                {"field": "status", "type": "string"},
                {"field": "priority", "type": "integer"}
            ]
        }))
        .unwrap();

        assert_eq!(index.fields.len(), 2);
        assert_eq!(index.fields[1].index_type, IndexType::Integer);
        assert!(!index.unique);
    }

    #[test]
    fn index_declaration_deserializes_single_and_compound_forms() {
        let single: IndexDeclaration =
            serde_json::from_value(json!({"field": "status", "type": "string"})).unwrap();
        assert!(matches!(single, IndexDeclaration::Single(_)));

        let compound: IndexDeclaration = serde_json::from_value(json!({
            "fields": [
                {"field": "status", "type": "string"},
                {"field": "priority", "type": "integer"}
            ],
            "unique": true
        }))
        .unwrap();
        assert!(matches!(compound, IndexDeclaration::Compound(_)));
    }

    #[test]
    fn index_declaration_rejects_ambiguous_or_unknown_shapes() {
        for value in [
            json!({}),
            json!({"type": "string"}),
            json!({"field": "status", "fields": [], "type": "string"}),
            json!({"fields": [{"field": "status", "type": "string"}], "type": "string"}),
            json!({"field": "status", "type": "string", "algorithm": "btree"}),
            json!("status"),
        ] {
            assert!(
                serde_json::from_value::<IndexDeclaration>(value).is_err(),
                "shape should be rejected"
            );
        }
    }

    #[test]
    fn document_carriers_preserve_extra_and_legacy_compound_indexes() {
        let doc: EsfCoreDocument = serde_json::from_value(json!({
            "esf_version": "1.0",
            "collection": "tasks",
            "description": "Task queue",
            "entity_schema": {"type": "object"},
            "indexes": [{"field": "status", "type": "string"}],
            "compound_indexes": [{
                "fields": [{"field": "status", "type": "string"}]
            }],
            "link_types": {"blocks": {"target_collection": "tasks", "cardinality": "many-to-many"}}
        }))
        .unwrap();

        assert_eq!(doc.collection, "tasks");
        assert_eq!(doc.indexes.len(), 1);
        assert_eq!(doc.compound_indexes.len(), 1);
        assert!(doc.extra.contains_key("link_types"));

        let entity_doc: EntitySchemaDocument = serde_json::from_value(json!({
            "entity_schema": {"type": "object"},
            "indexes": [{"field": "status", "type": "string"}],
            "validation_rules": []
        }))
        .unwrap();

        assert_eq!(entity_doc.indexes.len(), 1);
        assert!(entity_doc.extra.contains_key("validation_rules"));
    }

    #[test]
    fn document_carriers_reject_malformed_compound_indexes() {
        let malformed = json!({
            "indexes": [{
                "fields": [{"field": "status", "type": "string"}],
                "type": "string"
            }]
        });
        assert!(serde_json::from_value::<EntitySchemaDocument>(malformed).is_err());

        let malformed = json!({
            "esf_version": "1.0",
            "collection": "tasks",
            "indexes": [{
                "fields": [{"field": "status", "type": "string"}],
                "type": "string"
            }]
        });
        assert!(serde_json::from_value::<EsfCoreDocument>(malformed).is_err());
    }
}
