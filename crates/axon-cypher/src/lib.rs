//! openCypher subset for Axon graph queries.
//!
//! This crate parses, plans, and executes a read-only subset of openCypher
//! per [ADR-021](../../docs/helix/02-design/adr/ADR-021-graph-query-language.md).
//! Writes are not supported; that flow lives in mutation intents (FEAT-030).
//!
//! Surface in V1:
//! - [`parse`] — parse a Cypher query string into an [`ast::Query`].
//! - [`planner::plan`] — compile a validated query into an execution plan.
//!
//! The supported clauses, exclusions, and policy semantics are all
//! specified in ADR-021. Any clause outside the supported subset is
//! rejected at parse time with a typed error from [`error::CypherError`].

pub mod ast;
pub mod error;
pub mod executor;
pub mod lexer;
pub mod memory_store;
pub mod parser;
pub mod planner;
pub mod schema;
pub mod validator;

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
pub use schema::SchemaSnapshot;
pub use validator::validate;

/// Parse a Cypher query string into an [`ast::Query`].
///
/// Returns [`CypherError::Parse`] if the query is malformed or contains
/// constructs outside the supported V1 subset.
pub fn parse(input: &str) -> Result<Query, CypherError> {
    let tokens = lexer::tokenize(input)?;
    parser::Parser::new(tokens).parse_query()
}
