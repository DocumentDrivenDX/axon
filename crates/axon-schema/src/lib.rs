//! Schema definitions, validation, and migration for Axon collections.
//!
//! `axon-schema` provides the schema engine that enforces structure on entities
//! stored in Axon collections. Every collection has an associated schema that
//! validates entity fields, types, and constraints.

pub mod schema;
pub mod validation;

pub use schema::CollectionSchema;
