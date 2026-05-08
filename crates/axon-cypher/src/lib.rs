//! openCypher subset for Axon graph queries.
//!
//! AST, parser, validator, and schema types live in the leaf crate
//! `axon-cypher-ast`. This crate adds the planner and executor and
//! re-exports the full public surface so existing callers need no changes.
//!
//! Surface in V1:
//! - [`parse`] — parse a Cypher query string into an [`ast::Query`].
//! - [`planner::plan`] — compile a validated query into an execution plan.
//!
//! The supported clauses, exclusions, and policy semantics are all
//! specified in ADR-021. Any clause outside the supported subset is
//! rejected at parse time with a typed error from [`error::CypherError`].

// ── Re-export leaf modules from axon-cypher-ast ─────────────────────────────
pub use axon_cypher_ast::ast;
pub use axon_cypher_ast::error;
pub use axon_cypher_ast::lexer;
pub use axon_cypher_ast::parser;
pub use axon_cypher_ast::validator;

// ── Local modules: schema extension + executor-side ─────────────────────────
// `schema` overrides axon_cypher_ast::schema; it re-exports everything from
// there and adds the SchemaSnapshotExt extension trait.
pub mod schema;
pub mod executor;
pub mod memory_store;
pub mod planner;

// ── Top-level re-exports for API compatibility ───────────────────────────────
pub use axon_cypher_ast::parse;
pub use ast::Query;
pub use error::CypherError;
pub use executor::{
    execute, execute_with_options, ExecutionClock, ExecutionOptions, Row, RowStream,
};
pub use memory_store::{
    EntityScan, LinkTraversal, MemoryQueryStore, PropertyFilter, PropertyFilterOp, QueryEntity,
    QueryLink, QueryStore,
};
pub use planner::{plan, ExecutionPlan, PlanOperator};
pub use schema::{SchemaSnapshot, SchemaSnapshotExt};
pub use validator::validate;
