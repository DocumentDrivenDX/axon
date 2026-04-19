use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::error::AxonError;
use axon_core::id::CollectionId;

use crate::rules::ValidationRule;

/// Cardinality constraint for a link type (ADR-002).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

/// Definition of a single link type within a collection schema (ADR-002, Layer 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkTypeDef {
    /// The collection that target entities must belong to.
    pub target_collection: String,
    /// Cardinality constraint for this link type.
    pub cardinality: Cardinality,
    /// Whether at least one link of this type must exist for every entity.
    #[serde(default)]
    pub required: bool,
    /// Optional JSON Schema 2020-12 for validating link metadata.
    pub metadata_schema: Option<Value>,
}

/// Lifecycle definition for a state-machine field (ESF Layer 6).
///
/// Describes valid states and allowed transitions for a single field
/// (e.g. `status`). Used by `transition_lifecycle` to validate state changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleDef {
    /// The entity field this lifecycle governs (e.g. `"status"`).
    pub field: String,
    /// The initial state for new entities.
    pub initial: String,
    /// Map from state name to the list of states reachable from it.
    pub transitions: HashMap<String, Vec<String>>,
}

/// Gate definition declared in the schema (ESF Layer 5).
///
/// Gates group validation rules by purpose. The `save` gate blocks persistence;
/// custom gates (e.g. `complete`, `review`) allow saves but track readiness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateDef {
    /// Human-readable description of what this gate means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Other gates whose rules are included in this gate.
    /// e.g., `review` includes `complete` means all complete rules
    /// must also pass for review to pass.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
}

/// Value type for an index field (ESF Layer 4).
///
/// Determines which EAV index table the value is stored in and how
/// comparisons (equality, range) are performed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexType {
    String,
    Integer,
    Float,
    Datetime,
    Boolean,
}

impl std::fmt::Display for IndexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexType::String => write!(f, "string"),
            IndexType::Integer => write!(f, "integer"),
            IndexType::Float => write!(f, "float"),
            IndexType::Datetime => write!(f, "datetime"),
            IndexType::Boolean => write!(f, "boolean"),
        }
    }
}

/// A single-field secondary index declaration (ESF Layer 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    /// JSON field path to index (e.g. `"status"`, `"address.city"`).
    pub field: String,
    /// Value type for this index.
    #[serde(rename = "type")]
    pub index_type: IndexType,
    /// When `true`, the index enforces uniqueness: no two entities in
    /// the same collection may share the same indexed value.
    #[serde(default)]
    pub unique: bool,
}

/// A single field within a compound index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexField {
    /// JSON field path.
    pub field: String,
    /// Value type.
    #[serde(rename = "type")]
    pub index_type: IndexType,
}

/// A compound (multi-field) secondary index declaration (ESF Layer 4, US-033).
///
/// Compound indexes accelerate queries that filter on multiple fields.
/// The field order matters: leftmost prefix matching is supported (a
/// compound index on `[status, priority]` also accelerates queries
/// filtering on `status` alone).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexDef {
    /// Ordered list of fields in the compound key.
    pub fields: Vec<CompoundIndexField>,
    /// When `true`, the combination of all indexed field values must be unique.
    #[serde(default)]
    pub unique: bool,
}

/// Defines the structure and constraints for entities in a collection.
///
/// The `entity_schema` field holds a JSON Schema 2020-12 document (Layer 1 of ESF)
/// that validates entity bodies on every create/update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionSchema {
    /// The collection this schema governs.
    pub collection: CollectionId,
    /// Human-readable description.
    pub description: Option<String>,
    /// Schema version; incremented on each migration.
    pub version: u32,
    /// The JSON Schema 2020-12 document for entity body validation (Layer 1 of ESF).
    /// When `None`, no structural validation is enforced (all entities are accepted).
    pub entity_schema: Option<Value>,
    /// Link-type definitions (Layer 2 of ESF). Keys are link-type names.
    #[serde(default)]
    pub link_types: HashMap<String, LinkTypeDef>,
    /// Gate definitions (ESF Layer 5). Keys are gate names.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub gates: HashMap<String, GateDef>,
    /// Validation rules (ESF Layer 5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_rules: Vec<ValidationRule>,
    /// Secondary index declarations (ESF Layer 4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDef>,
    /// Compound index declarations (ESF Layer 4, US-033).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
    /// Lifecycle definitions (ESF Layer 6). Keys are lifecycle names.
    #[serde(default)]
    pub lifecycles: HashMap<String, LifecycleDef>,
}

/// Presentation metadata for a collection, versioned independently from the
/// validation schema.
///
/// Markdown templates are a rendering concern. Keeping them in a sibling type
/// avoids inflating schema versions when only presentation changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionView {
    /// The collection this view describes.
    pub collection: CollectionId,
    /// Optional human-readable description for the view itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Markdown template used to render entities in the collection.
    pub markdown_template: String,
    /// View version; incremented on each template update.
    pub version: u32,
    /// Nanoseconds since Unix epoch when the view was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ns: Option<u64>,
    /// Actor who last updated the view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

impl CollectionView {
    pub fn new(collection: CollectionId, markdown_template: impl Into<String>) -> Self {
        Self {
            collection,
            description: None,
            markdown_template: markdown_template.into(),
            version: 1,
            updated_at_ns: None,
            updated_by: None,
        }
    }
}

impl CollectionSchema {
    pub fn new(collection: CollectionId) -> Self {
        Self {
            collection,
            description: None,
            version: 1,
            entity_schema: None,
            link_types: HashMap::new(),
            gates: HashMap::new(),
            validation_rules: Vec::new(),
            indexes: Vec::new(),
            compound_indexes: Vec::new(),
            lifecycles: HashMap::new(),
        }
    }
}

/// A parsed Entity Schema Format (ESF) document.
///
/// ESF uses a three-layer model (see ADR-002):
/// - Layer 1: JSON Schema 2020-12 for entity body validation
/// - Layer 2: Axon link-type definitions
/// - Layer 3: Axon validation rules with severity levels
///
/// Parsing is YAML-first; JSON is also accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsfDocument {
    /// ESF format version (e.g., `"1.0"`).
    pub esf_version: String,
    /// The collection this schema governs.
    pub collection: String,
    /// Layer 1: JSON Schema 2020-12 document governing entity bodies.
    pub entity_schema: Option<Value>,
    /// Layer 2: Link-type definitions (Axon vocabulary). Stored as raw JSON for now.
    pub link_types: Option<Value>,
    /// Layer 3: Custom validation rules with severity. Stored as raw JSON for now.
    pub validation_rules: Option<Value>,
    /// Layer 6: Lifecycle definitions. Stored as raw JSON for now.
    pub lifecycles: Option<Value>,
}

impl EsfDocument {
    /// Parse an ESF document from a YAML (or JSON) string.
    ///
    /// Returns `AxonError::SchemaValidation` if the input cannot be parsed or
    /// is missing required top-level fields.
    pub fn parse(input: &str) -> Result<Self, AxonError> {
        let value: Value = serde_yaml::from_str(input)
            .map_err(|e| AxonError::SchemaValidation(format!("ESF parse error: {e}")))?;
        serde_json::from_value(value)
            .map_err(|e| AxonError::SchemaValidation(format!("ESF structure error: {e}")))
    }

    /// Convert this ESF document into a [`CollectionSchema`] using the collection
    /// name from the document, the Layer 1 JSON Schema, and Layer 2 link-type
    /// definitions.
    pub fn into_collection_schema(self) -> Result<CollectionSchema, AxonError> {
        let link_types: HashMap<String, LinkTypeDef> = match self.link_types {
            Some(val) => serde_json::from_value(val).map_err(|e| {
                AxonError::SchemaValidation(format!("invalid link_types definition: {e}"))
            })?,
            None => HashMap::new(),
        };
        let lifecycles: HashMap<String, LifecycleDef> = match self.lifecycles {
            Some(val) => serde_json::from_value(val).map_err(|e| {
                AxonError::SchemaValidation(format!("invalid lifecycles definition: {e}"))
            })?,
            None => HashMap::new(),
        };
        Ok(CollectionSchema {
            collection: CollectionId::new(self.collection),
            description: None,
            version: 1,
            entity_schema: self.entity_schema,
            link_types,
            gates: HashMap::new(),
            validation_rules: Vec::new(),
            indexes: Vec::new(),
            compound_indexes: Vec::new(),
            lifecycles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_new_starts_at_version_one() {
        let schema = CollectionSchema::new(CollectionId::new("tasks"));
        assert_eq!(schema.version, 1);
        assert!(schema.description.is_none());
        assert!(schema.entity_schema.is_none());
    }

    #[test]
    fn collection_view_new_starts_at_version_one() {
        let view = CollectionView::new(CollectionId::new("tasks"), "# {{title}}");
        assert_eq!(view.version, 1);
        assert_eq!(view.markdown_template, "# {{title}}");
        assert!(view.updated_at_ns.is_none());
        assert!(view.updated_by.is_none());
    }

    /// Sample ESF document derived from ADR-002 (invoice collection).
    const INVOICE_ESF: &str = r#"
esf_version: "1.0"
collection: invoices
entity_schema:
  type: object
  required:
    - vendor_id
    - amount
    - status
  properties:
    vendor_id:
      type: string
    amount:
      type: object
      properties:
        value:
          type: number
          minimum: 0
        currency:
          type: string
          enum: [USD, EUR, GBP]
    status:
      type: string
      enum: [draft, submitted, approved, paid, reconciled]
    line_items:
      type: array
      items:
        type: object
        properties:
          description:
            type: string
          quantity:
            type: integer
            minimum: 1
          unit_price:
            type: number
            minimum: 0
"#;

    #[test]
    fn parse_esf_from_adr_002() {
        let doc = EsfDocument::parse(INVOICE_ESF).expect("invoice ESF fixture should parse");
        assert_eq!(doc.esf_version, "1.0");
        assert_eq!(doc.collection, "invoices");
        assert!(
            doc.entity_schema.is_some(),
            "entity_schema should be present"
        );
        let schema = doc
            .entity_schema
            .as_ref()
            .expect("invoice ESF fixture should include an entity schema");
        let required = schema["required"]
            .as_array()
            .expect("invoice ESF fixture should mark required fields");
        assert!(required.iter().any(|v| v == "vendor_id"));
        assert!(required.iter().any(|v| v == "amount"));
        assert!(required.iter().any(|v| v == "status"));
    }

    #[test]
    fn esf_into_collection_schema() {
        let doc = EsfDocument::parse(INVOICE_ESF).expect("invoice ESF fixture should parse");
        let schema = doc
            .into_collection_schema()
            .expect("invoice ESF fixture should convert to collection schema");
        assert_eq!(schema.collection.as_str(), "invoices");
        assert_eq!(schema.version, 1);
        assert!(schema.entity_schema.is_some());
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let result = EsfDocument::parse("{ not: valid yaml: [");
        assert!(result.is_err());
    }

    #[test]
    fn esf_parses_lifecycles_from_beads_fixture() {
        let esf = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/beads.esf.yaml"),
        )
        .expect("beads.esf.yaml fixture missing");
        let doc = EsfDocument::parse(&esf).unwrap();
        let schema = doc.into_collection_schema().unwrap();

        let lc = schema
            .lifecycles
            .get("status")
            .expect("status lifecycle missing");
        assert_eq!(lc.field, "status");
        assert_eq!(lc.initial, "draft");

        let from_draft = lc
            .transitions
            .get("draft")
            .expect("draft transitions missing");
        assert!(from_draft.contains(&"pending".to_string()));
        assert!(from_draft.contains(&"cancelled".to_string()));
    }

    #[test]
    fn collection_schema_without_lifecycles_defaults_to_empty() {
        let doc = EsfDocument::parse(INVOICE_ESF).unwrap();
        let schema = doc.into_collection_schema().unwrap();
        assert!(schema.lifecycles.is_empty());
    }

    #[test]
    fn lifecycle_def_round_trips_through_json() {
        let mut transitions = HashMap::new();
        transitions.insert("draft".to_string(), vec!["active".to_string()]);
        transitions.insert("active".to_string(), vec!["closed".to_string()]);
        let lc = LifecycleDef {
            field: "status".to_string(),
            initial: "draft".to_string(),
            transitions,
        };
        let mut lifecycles = HashMap::new();
        lifecycles.insert("status".to_string(), lc.clone());
        let schema = CollectionSchema {
            collection: CollectionId::new("items"),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: HashMap::new(),
            gates: HashMap::new(),
            validation_rules: Vec::new(),
            indexes: Vec::new(),
            compound_indexes: Vec::new(),
            lifecycles,
        };
        let json = serde_json::to_string(&schema).unwrap();
        let restored: CollectionSchema = serde_json::from_str(&json).unwrap();
        let restored_lc = restored.lifecycles.get("status").unwrap();
        assert_eq!(restored_lc.field, lc.field);
        assert_eq!(restored_lc.initial, lc.initial);
        assert_eq!(restored_lc.transitions, lc.transitions);
    }

    #[test]
    fn collection_schema_new_has_empty_lifecycles() {
        let schema = CollectionSchema::new(CollectionId::new("tasks"));
        assert!(schema.lifecycles.is_empty());
    }
}
