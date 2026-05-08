//! openCypher subset AST, parser, and validator for Axon graph queries.
//!
//! This is a leaf crate (depends on `axon-core` only among workspace crates).
//! It provides the parser, AST, validator, and schema snapshot types but does
//! NOT include the executor or planner — those live in `axon-cypher`, which
//! depends on this crate.

pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod schema;
pub mod validator;

pub use error::CypherError;
pub use schema::SchemaSnapshot;

/// Parse a Cypher query string into an [`ast::Query`].
///
/// Returns [`CypherError::Parse`] if the query is malformed or contains
/// constructs outside the supported V1 subset.
pub fn parse(input: &str) -> Result<ast::Query, CypherError> {
    let tokens = lexer::tokenize(input)?;
    parser::Parser::new(tokens).parse_query()
}
