#![forbid(unsafe_code)]
//! Thin ESF types and compile-once validation for external consumers.

pub mod types;
pub mod validation;

pub use types::{
    CompoundIndexDef, CompoundIndexField, EntitySchemaDocument, EsfCoreDocument, IndexDeclaration,
    IndexDef, IndexType,
};
pub use validation::{CompiledSchema, RawValidationError, RawValidationErrors, SchemaCompileError};
