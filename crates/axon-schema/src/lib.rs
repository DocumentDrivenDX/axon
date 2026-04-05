//! Schema definitions, validation, and migration for Axon collections.
//!
//! `axon-schema` provides the schema engine that enforces structure on entities
//! stored in Axon collections. Every collection has an associated schema that
//! validates entity fields, types, and constraints.

pub mod schema;
pub mod validation;

#[cfg(test)]
mod proptest_schema;

pub use schema::{Cardinality, CollectionSchema, EsfDocument, LinkTypeDef};
pub use validation::{
    compile_entity_schema, validate, validate_entity, validate_link_metadata,
    SchemaValidationError, SchemaValidationErrors,
};
