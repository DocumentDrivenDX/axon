use std::error::Error;
use std::fmt;

use serde_json::Value;

/// Error returned when a JSON Schema document cannot be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCompileError {
    pub message: String,
}

impl fmt::Display for SchemaCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for SchemaCompileError {}

/// Raw JSON Schema validation error owned by `axon-esf`.
#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationError {
    pub instance_path: String,
    pub message: String,
    pub instance: Value,
}

impl fmt::Display for RawValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.instance_path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.instance_path, self.message)
        }
    }
}

/// Collection of raw JSON Schema validation errors.
#[derive(Debug, Clone, PartialEq)]
pub struct RawValidationErrors(pub Vec<RawValidationError>);

impl RawValidationErrors {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for RawValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, error) in self.0.iter().enumerate() {
            if idx > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl Error for RawValidationErrors {}

/// Compile-once JSON Schema validator for ESF entity schemas.
pub struct CompiledSchema {
    validator: jsonschema::Validator,
}

impl CompiledSchema {
    pub fn compile(schema: &Value) -> Result<Self, SchemaCompileError> {
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .should_validate_formats(true)
            .build(schema)
            .map_err(|error| SchemaCompileError {
                message: error.to_string(),
            })?;

        Ok(Self { validator })
    }

    pub fn validate(&self, data: &Value) -> Result<(), RawValidationErrors> {
        let errors: Vec<RawValidationError> = self
            .validator
            .iter_errors(data)
            .map(|error| RawValidationError {
                instance_path: error.instance_path.to_string(),
                message: error.to_string(),
                instance: error.instance.into_owned(),
            })
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(RawValidationErrors(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compiled_schema_validates_repeatedly() {
        let schema = json!({
            "type": "object",
            "required": ["status"],
            "properties": {
                "status": {"type": "string"}
            }
        });
        let compiled = CompiledSchema::compile(&schema).unwrap();
        let data = json!({"status": "ready"});

        for _ in 0..100 {
            compiled.validate(&data).unwrap();
        }
    }

    #[test]
    fn format_assertions_reject_malformed_values() {
        let schema = json!({
            "type": "object",
            "properties": {
                "email": {"type": "string", "format": "email"},
                "id": {"type": "string", "format": "uuid"},
                "created_at": {"type": "string", "format": "date-time"}
            }
        });
        let compiled = CompiledSchema::compile(&schema).unwrap();
        let errors = compiled
            .validate(&json!({
                "email": "not an email",
                "id": "not-a-uuid",
                "created_at": "not-a-date"
            }))
            .unwrap_err();

        assert!(
            errors.len() >= 3,
            "expected all format errors, got {errors}"
        );
    }

    #[test]
    fn internal_defs_refs_still_work() {
        let schema = json!({
            "$defs": {
                "status": {"type": "string", "enum": ["ready", "done"]}
            },
            "type": "object",
            "properties": {
                "status": {"$ref": "#/$defs/status"}
            }
        });
        let compiled = CompiledSchema::compile(&schema).unwrap();

        compiled.validate(&json!({"status": "ready"})).unwrap();
        assert!(compiled.validate(&json!({"status": "bad"})).is_err());
    }

    #[test]
    fn validate_collects_all_errors() {
        let schema = json!({
            "type": "object",
            "required": ["a", "b"],
            "properties": {
                "c": {"type": "integer"}
            }
        });
        let compiled = CompiledSchema::compile(&schema).unwrap();
        let errors = compiled.validate(&json!({"c": "wrong"})).unwrap_err();

        assert!(
            errors.len() >= 3,
            "expected required and type errors, got {errors}"
        );
    }
}
