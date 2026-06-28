#![forbid(unsafe_code)]
//! Thin ESF types and compile-once validation for external consumers.

pub mod index_key;
pub mod types;
pub mod validation;

pub use index_key::{extract_path, IndexKeyError};
pub use types::{
    CompoundIndexDef, CompoundIndexField, EntitySchemaDocument, EsfCoreDocument, IndexDeclaration,
    IndexDef, IndexType,
};
pub use validation::{CompiledSchema, RawValidationError, RawValidationErrors, SchemaCompileError};
