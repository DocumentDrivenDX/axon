use axon_core::error::AxonError;
use serde_json::Value;

use crate::schema::CollectionSchema;

/// Validates entity data against its collection schema.
///
/// Returns `Ok(())` if the data conforms to the schema, or an
/// `AxonError::SchemaValidation` describing the violation.
pub fn validate(_schema: &CollectionSchema, _data: &Value) -> Result<(), AxonError> {
    // Stub: full validation will be implemented in a follow-on issue.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::CollectionId;
    use serde_json::json;

    #[test]
    fn stub_validation_always_passes() {
        let schema = CollectionSchema::new(CollectionId::new("tasks"));
        let result = validate(&schema, &json!({"title": "hello"}));
        assert!(result.is_ok());
    }
}
