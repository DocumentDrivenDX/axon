#![forbid(unsafe_code)]
//! Schema definitions, validation, and migration for Axon collections.
//!
//! `axon-schema` provides the schema engine that enforces structure on entities
//! stored in Axon collections. Every collection has an associated schema that
//! validates entity fields, types, and constraints.

pub mod evolution;
pub mod gates;
pub mod rules;
pub mod schema;
pub mod validation;

#[cfg(test)]
mod proptest_schema;

pub use evolution::{
    classify, diff_schemas, Compatibility, FieldChange, FieldChangeKind, SchemaDiff,
};
pub use gates::{evaluate_gates, GateEvaluation, GateResult};
pub use rules::validate_rule_definitions;
pub use schema::{
    Cardinality, CollectionSchema, CollectionView, CompoundIndexDef, CompoundIndexField,
    EsfDocument, GateDef, IndexDef, IndexType, LinkTypeDef,
};
pub use validation::{
    compile_entity_schema, validate, validate_entity, validate_link_metadata,
    SchemaValidationError, SchemaValidationErrors,
};
