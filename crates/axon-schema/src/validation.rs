use std::fmt;

use serde_json::Value;

use axon_core::error::AxonError;

use crate::schema::CollectionSchema;

/// A single schema validation error with structured location and description.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaValidationError {
    /// JSON Pointer to the failing field (e.g., `"/status"`, `"/amount/currency"`).
    pub field_path: String,
    /// Human-readable description of the violation.
    pub message: String,
}

impl fmt::Display for SchemaValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field_path, self.message)
    }
}

/// A collection of [`SchemaValidationError`]s returned when an entity fails
/// schema validation. All violations are reported, not just the first.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaValidationErrors(pub Vec<SchemaValidationError>);

impl SchemaValidationErrors {
    /// Returns the number of validation errors.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for SchemaValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl From<SchemaValidationErrors> for AxonError {
    fn from(errs: SchemaValidationErrors) -> Self {
        AxonError::SchemaValidation(errs.to_string())
    }
}

/// Validates `data` against the collection `schema`.
///
/// - If `schema.entity_schema` is `None`, validation passes unconditionally.
/// - Otherwise, `data` is validated against the JSON Schema 2020-12 document
///   in `entity_schema`. All violations are collected and returned.
///
/// Returns `Ok(())` if the entity conforms, or `Err(SchemaValidationErrors)`
/// listing every violation with its field path.
pub fn validate_entity(
    schema: &CollectionSchema,
    data: &Value,
) -> Result<(), SchemaValidationErrors> {
    let Some(json_schema) = &schema.entity_schema else {
        return Ok(());
    };

    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(json_schema)
        .map_err(|e| {
            SchemaValidationErrors(vec![SchemaValidationError {
                field_path: "/".into(),
                message: format!("invalid schema definition: {e}"),
            }])
        })?;

    let errors: Vec<SchemaValidationError> = validator
        .iter_errors(data)
        .map(|e| SchemaValidationError {
            field_path: e.instance_path.to_string(),
            message: e.to_string(),
        })
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(SchemaValidationErrors(errors))
    }
}

/// Convenience wrapper: validates and converts to [`AxonError`] on failure.
pub fn validate(schema: &CollectionSchema, data: &Value) -> Result<(), AxonError> {
    validate_entity(schema, data).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::EsfDocument;
    use axon_core::id::CollectionId;
    use serde_json::json;

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
"#;

    fn invoice_schema() -> CollectionSchema {
        EsfDocument::parse(INVOICE_ESF)
            .unwrap()
            .into_collection_schema()
    }

    #[test]
    fn stub_validation_always_passes() {
        // Schema with no entity_schema — all data is accepted.
        let schema = CollectionSchema::new(CollectionId::new("tasks"));
        let result = validate(&schema, &json!({"title": "hello"}));
        assert!(result.is_ok());
    }

    #[test]
    fn valid_entity_returns_ok() {
        let schema = invoice_schema();
        let entity = json!({
            "vendor_id": "vnd-001",
            "amount": { "value": 100.0, "currency": "USD" },
            "status": "draft"
        });
        assert!(
            validate_entity(&schema, &entity).is_ok(),
            "valid invoice entity should pass"
        );
    }

    #[test]
    fn invalid_entity_returns_structured_errors_with_field_path() {
        let schema = invoice_schema();
        // Missing all required fields.
        let entity = json!({});
        let errs = validate_entity(&schema, &entity).unwrap_err();
        assert!(!errs.is_empty(), "should have validation errors");

        // Each error has a field_path and message.
        for e in &errs.0 {
            assert!(
                !e.message.is_empty(),
                "message should not be empty, got: {e:?}"
            );
        }

        // Display output is non-empty.
        assert!(!errs.to_string().is_empty());
    }

    #[test]
    fn wrong_enum_value_reports_field_path() {
        let schema = invoice_schema();
        let entity = json!({
            "vendor_id": "vnd-001",
            "amount": { "value": 50.0, "currency": "JPY" },  // JPY not in enum
            "status": "draft"
        });
        let errs = validate_entity(&schema, &entity).unwrap_err();
        assert!(!errs.is_empty());
        // At least one error should reference the currency field.
        let has_currency_error = errs.0.iter().any(|e| e.field_path.contains("currency"));
        assert!(
            has_currency_error,
            "expected error for currency field, got: {errs}"
        );
    }

    #[test]
    fn multiple_violations_all_reported() {
        let schema = invoice_schema();
        // vendor_id missing and status is invalid.
        let entity = json!({
            "amount": { "value": 10.0, "currency": "USD" },
            "status": "unknown_status"
        });
        let errs = validate_entity(&schema, &entity).unwrap_err();
        assert!(errs.len() >= 2, "expected at least 2 errors, got: {errs}");
    }
}
