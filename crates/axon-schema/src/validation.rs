use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::error::AxonError;

use crate::schema::CollectionSchema;

/// A single schema validation error with structured location and description.
///
/// Enhanced with severity, fix suggestions, and context for actionable
/// error messages (US-068 / FEAT-019).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaValidationError {
    /// JSON Pointer to the failing field (e.g., `"/status"`, `"/amount/currency"`).
    pub field_path: String,
    /// Human-readable description of the violation.
    pub message: String,
    /// Severity of the error: "error", "warning", or "info".
    #[serde(default = "default_severity")]
    pub severity: String,
    /// Actionable fix suggestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Context about the constraint that was violated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

fn default_severity() -> String {
    "error".into()
}

impl SchemaValidationError {
    /// Create a basic error (backward compatible).
    pub fn new(field_path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field_path: field_path.into(),
            message: message.into(),
            severity: "error".into(),
            fix: None,
            context: None,
        }
    }
}

impl fmt::Display for SchemaValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field_path, self.message)?;
        if let Some(fix) = &self.fix {
            write!(f, " Fix: {fix}")?;
        }
        Ok(())
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
            SchemaValidationErrors(vec![SchemaValidationError::new(
                "/",
                format!("invalid schema definition: {e}"),
            )])
        })?;

    let errors: Vec<SchemaValidationError> = validator
        .iter_errors(data)
        .map(|e| enhance_json_schema_error(&e, json_schema))
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

/// Validate link metadata against a JSON Schema document.
///
/// Returns `Ok(())` if the metadata conforms. Returns
/// `Err(AxonError::SchemaValidation)` listing every violation.
pub fn validate_link_metadata(metadata_schema: &Value, metadata: &Value) -> Result<(), AxonError> {
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(metadata_schema)
        .map_err(|e| AxonError::SchemaValidation(format!("invalid metadata_schema: {e}")))?;

    let errors: Vec<String> = validator
        .iter_errors(metadata)
        .map(|e| {
            if e.instance_path.as_str().is_empty() {
                e.to_string()
            } else {
                format!("{}: {}", e.instance_path, e)
            }
        })
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(AxonError::SchemaValidation(format!(
            "link metadata validation failed: {}",
            errors.join("; ")
        )))
    }
}

/// Compile a raw JSON Schema value to check it is well-formed.
///
/// Returns `AxonError::SchemaValidation` if the value cannot be compiled as a
/// valid JSON Schema 2020-12 document. Returns `Ok(())` if it compiles
/// successfully.
pub fn compile_entity_schema(json_schema: &Value) -> Result<(), AxonError> {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(json_schema)
        .map(|_| ())
        .map_err(|e| AxonError::SchemaValidation(format!("invalid schema: {e}")))
}

// ── JSON Schema error enhancement ────────────────────────────────────────────

/// Enhance a JSON Schema validation error with actionable information.
fn enhance_json_schema_error(
    error: &jsonschema::ValidationError,
    schema: &Value,
) -> SchemaValidationError {
    let field_path = error.instance_path.to_string();
    let field_name = field_path
        .rsplit('/')
        .next()
        .unwrap_or(&field_path)
        .to_string();
    let raw_msg = error.to_string();

    // Try to classify the error and produce better messages.
    let kind = classify_json_schema_error(&raw_msg, error);

    match kind {
        JsonSchemaErrorKind::RequiredMissing { field } => {
            let default_hint = find_default_in_schema(schema, &field);
            let fix = match default_hint {
                Some(default_val) => format!("Add a '{field}' field (default: {default_val})"),
                None => format!("Add a '{field}' field"),
            };
            SchemaValidationError {
                field_path: field_path.clone(),
                message: format!("Required field '{field}' is missing"),
                severity: "error".into(),
                fix: Some(fix),
                context: Some(serde_json::json!({
                    "constraint": "required",
                    "field": field,
                })),
            }
        }
        JsonSchemaErrorKind::EnumMismatch { actual, allowed } => {
            let suggestion = if allowed.len() <= 20 {
                nearest_match(&actual, &allowed)
            } else {
                None
            };
            let mut fix = format!("Use one of the allowed values: {}", allowed.join(", "));
            if let Some(near) = &suggestion {
                fix = format!("{fix}. Did you mean '{near}'?");
            }
            SchemaValidationError {
                field_path,
                message: format!(
                    "Field '{field_name}' must be one of [{}]. Got: '{actual}'",
                    allowed.join(", ")
                ),
                severity: "error".into(),
                fix: Some(fix),
                context: Some(serde_json::json!({
                    "constraint": "enum",
                    "actual": actual,
                    "allowed": allowed,
                })),
            }
        }
        JsonSchemaErrorKind::TypeMismatch { expected, actual } => SchemaValidationError {
            field_path,
            message: format!("Field '{field_name}' must be {expected}. Got: {actual}"),
            severity: "error".into(),
            fix: Some(format!("Provide a {expected} value")),
            context: Some(serde_json::json!({
                "constraint": "type",
                "expected": expected,
                "actual": actual,
            })),
        },
        JsonSchemaErrorKind::Other => SchemaValidationError {
            field_path,
            message: raw_msg,
            severity: "error".into(),
            fix: None,
            context: None,
        },
    }
}

enum JsonSchemaErrorKind {
    RequiredMissing {
        field: String,
    },
    EnumMismatch {
        actual: String,
        allowed: Vec<String>,
    },
    TypeMismatch {
        expected: String,
        actual: String,
    },
    Other,
}

fn classify_json_schema_error(
    msg: &str,
    error: &jsonschema::ValidationError,
) -> JsonSchemaErrorKind {
    // Required property missing.
    if msg.contains("is a required property") {
        // The message format is: '"field_name" is a required property'
        if let Some(field) = msg.split('"').nth(1) {
            return JsonSchemaErrorKind::RequiredMissing {
                field: field.to_string(),
            };
        }
    }

    // Enum mismatch: look at the error instance value and try to extract allowed values.
    if msg.contains("is not one of") {
        let actual = format_value_brief(error.instance.as_ref());
        // Try to extract allowed values from the error message.
        let allowed = extract_enum_values(msg);
        if !allowed.is_empty() {
            return JsonSchemaErrorKind::EnumMismatch { actual, allowed };
        }
    }

    // Type mismatch.
    if msg.contains("is not of type") || msg.contains("is not of types") {
        let actual = json_type_name(error.instance.as_ref());
        let expected = extract_type_from_msg(msg);
        if !expected.is_empty() {
            return JsonSchemaErrorKind::TypeMismatch { expected, actual };
        }
    }

    JsonSchemaErrorKind::Other
}

/// Extract enum allowed values from a JSON Schema error message.
fn extract_enum_values(msg: &str) -> Vec<String> {
    // Typical message: '"value" is not one of ["a","b","c"]'
    if let Some(bracket_start) = msg.find('[') {
        if let Some(bracket_end) = msg[bracket_start..].find(']') {
            let inner = &msg[bracket_start + 1..bracket_start + bracket_end];
            return inner
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// Extract expected type from a JSON Schema type mismatch message.
fn extract_type_from_msg(msg: &str) -> String {
    // Message like: '"value" is not of type "string"'
    if let Some(pos) = msg.rfind('"') {
        let before = &msg[..pos];
        if let Some(start) = before.rfind('"') {
            return msg[start + 1..pos].to_string();
        }
    }
    String::new()
}

/// Brief display of a JSON value for error messages.
fn format_value_brief(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        _ => value.to_string(),
    }
}

/// Name of the JSON value type.
fn json_type_name(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Number(_) => "number".into(),
        Value::String(s) => format!("string '{s}'"),
        Value::Array(_) => "array".into(),
        Value::Object(_) => "object".into(),
    }
}

/// Find a default value for a field in the schema (if declared).
fn find_default_in_schema(schema: &Value, field: &str) -> Option<String> {
    schema
        .get("properties")?
        .get(field)?
        .get("default")
        .map(format_value_brief)
}

// ── Levenshtein distance ────────────────────────────────────────────────────

/// Compute the Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Find the nearest match to `input` among `candidates` using Levenshtein distance.
///
/// Returns `Some(match)` if the best match has a distance <= max(2, input.len()/3).
fn nearest_match(input: &str, candidates: &[String]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    let threshold = (input.len() / 3).max(2);
    let mut best: Option<(usize, &str)> = None;

    for candidate in candidates {
        let dist = levenshtein(input, candidate);
        if dist == 0 {
            continue; // Exact match, not useful as suggestion.
        }
        let is_better_match = match best {
            Some((best_dist, _)) => dist < best_dist,
            None => true,
        };
        if dist <= threshold && is_better_match {
            best = Some((dist, candidate));
        }
    }

    best.map(|(_, s)| s.to_string())
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
            .expect("invoice ESF fixture should parse")
            .into_collection_schema()
            .expect("invoice ESF fixture should convert to collection schema")
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
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail when required fields are missing");
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
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail for invalid enum values");
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
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail for missing nested required fields");
        assert!(errs.len() >= 2, "expected at least 2 errors, got: {errs}");
    }

    // ── US-068: Actionable error messages ────────────────────────────────

    #[test]
    fn required_field_error_names_field_and_suggests_fix() {
        let schema = invoice_schema();
        let entity = json!({});
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail for fields below minimum");

        // Should have errors for all three required fields.
        let required_errors: Vec<_> = errs
            .0
            .iter()
            .filter(|e| e.message.contains("Required field"))
            .collect();
        assert!(
            required_errors.len() >= 3,
            "expected 3 required-field errors, got: {errs}"
        );

        // Each required-field error should have a fix suggestion.
        for e in &required_errors {
            assert!(e.fix.is_some(), "required error should have fix: {e:?}");
            assert_eq!(e.severity, "error");
            assert!(e.context.is_some(), "should have context: {e:?}");
        }
    }

    #[test]
    fn enum_mismatch_includes_allowed_values_and_actual() {
        let schema = invoice_schema();
        let entity = json!({
            "vendor_id": "vnd-001",
            "amount": { "value": 50.0, "currency": "JPY" },
            "status": "draft"
        });
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail when additional properties are present");

        let enum_err = errs
            .0
            .iter()
            .find(|e| e.field_path.contains("currency"))
            .expect("should have currency error");

        assert!(
            enum_err.message.contains("must be one of"),
            "message should list allowed values: {}",
            enum_err.message
        );
        assert!(
            enum_err.message.contains("JPY"),
            "message should show actual value: {}",
            enum_err.message
        );
        assert!(enum_err.fix.is_some());
        assert!(enum_err.context.is_some());
    }

    #[test]
    fn enum_mismatch_did_you_mean_levenshtein() {
        let schema = invoice_schema();
        // "pendng" is close to "pending" but invoice uses different statuses.
        // Use "drafy" close to "draft" which is in the invoice enum.
        let entity = json!({
            "vendor_id": "vnd-001",
            "amount": { "value": 50.0, "currency": "USD" },
            "status": "drafy"
        });
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail for malformed nested objects");

        let status_err = errs
            .0
            .iter()
            .find(|e| e.field_path.contains("status"))
            .expect("should have status error");

        assert!(
            status_err
                .fix
                .as_deref()
                .unwrap_or("")
                .contains("Did you mean"),
            "fix should include did-you-mean suggestion: {:?}",
            status_err.fix
        );
        assert!(
            status_err.fix.as_deref().unwrap_or("").contains("draft"),
            "should suggest 'draft': {:?}",
            status_err.fix
        );
    }

    #[test]
    fn type_mismatch_shows_expected_and_actual() {
        let schema = CollectionSchema {
            collection: CollectionId::new("test"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "count": { "type": "integer" }
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        let entity = json!({"count": "not-a-number"});
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail when nested arrays contain invalid items");

        let type_err = &errs.0[0];
        assert!(
            type_err.message.contains("must be"),
            "should mention expected type: {}",
            type_err.message
        );
        assert!(type_err.fix.is_some(), "should have a fix suggestion");
    }

    #[test]
    fn all_errors_have_severity() {
        let schema = invoice_schema();
        let entity = json!({
            "status": "INVALID",
            "amount": { "value": "bad", "currency": "ZZZ" }
        });
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail when list items miss required fields");

        for e in &errs.0 {
            assert_eq!(e.severity, "error", "all errors should have severity=error");
        }
    }

    #[test]
    fn required_field_with_default_suggests_default() {
        let schema = CollectionSchema {
            collection: CollectionId::new("test"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["status"],
                "properties": {
                    "status": { "type": "string", "default": "draft" }
                }
            })),
            link_types: Default::default(),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
        };
        let entity = json!({});
        let errs = validate_entity(&schema, &entity)
            .expect_err("validation should fail for duplicate or invalid enum values");

        let status_err = errs
            .0
            .iter()
            .find(|e| e.message.contains("status"))
            .expect("should have status error");

        assert!(
            status_err.fix.as_deref().unwrap_or("").contains("default"),
            "fix should mention default value: {:?}",
            status_err.fix
        );
    }

    #[test]
    fn display_includes_fix_suggestion() {
        let err = SchemaValidationError {
            field_path: "/status".into(),
            message: "Field 'status' must be one of [draft, pending]".into(),
            severity: "error".into(),
            fix: Some("Use 'draft' or 'pending'".into()),
            context: None,
        };
        let display = err.to_string();
        assert!(
            display.contains("Fix:"),
            "display should include fix: {display}"
        );
    }

    // ── Levenshtein unit tests ───────────────────────────────────────────

    #[test]
    fn levenshtein_identical_strings() {
        assert_eq!(levenshtein("hello", "hello"), 0);
    }

    #[test]
    fn levenshtein_one_edit() {
        assert_eq!(levenshtein("draft", "drafy"), 1);
        assert_eq!(levenshtein("pending", "pendng"), 1);
    }

    #[test]
    fn levenshtein_empty_strings() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn nearest_match_finds_close_candidate() {
        let candidates = vec!["draft".into(), "submitted".into(), "approved".into()];
        assert_eq!(nearest_match("drafy", &candidates), Some("draft".into()));
    }

    #[test]
    fn nearest_match_returns_none_for_distant_input() {
        let candidates = vec!["draft".into(), "submitted".into()];
        assert_eq!(nearest_match("zzzzzzzzz", &candidates), None);
    }
}
