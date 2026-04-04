use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::error::AxonError;
use axon_core::id::CollectionId;

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
}

impl CollectionSchema {
    pub fn new(collection: CollectionId) -> Self {
        Self {
            collection,
            description: None,
            version: 1,
            entity_schema: None,
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
    /// name from the document and the Layer 1 JSON Schema.
    pub fn into_collection_schema(self) -> CollectionSchema {
        CollectionSchema {
            collection: CollectionId::new(self.collection),
            description: None,
            version: 1,
            entity_schema: self.entity_schema,
        }
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

    /// Sample ESF document derived from ADR-002 (invoice collection).
    pub(crate) const INVOICE_ESF: &str = r#"
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
        let doc = EsfDocument::parse(INVOICE_ESF).unwrap();
        assert_eq!(doc.esf_version, "1.0");
        assert_eq!(doc.collection, "invoices");
        assert!(
            doc.entity_schema.is_some(),
            "entity_schema should be present"
        );
        let schema = doc.entity_schema.as_ref().unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "vendor_id"));
        assert!(required.iter().any(|v| v == "amount"));
        assert!(required.iter().any(|v| v == "status"));
    }

    #[test]
    fn esf_into_collection_schema() {
        let doc = EsfDocument::parse(INVOICE_ESF).unwrap();
        let schema = doc.into_collection_schema();
        assert_eq!(schema.collection.as_str(), "invoices");
        assert_eq!(schema.version, 1);
        assert!(schema.entity_schema.is_some());
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let result = EsfDocument::parse("{ not: valid yaml: [");
        assert!(result.is_err());
    }
}
