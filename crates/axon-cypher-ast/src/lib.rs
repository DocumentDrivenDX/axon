//! openCypher subset AST, parser, validator, and planner for Axon graph queries.
//!
//! This is a leaf crate (depends on `axon-core` only among workspace crates).
//! It provides the parser, AST, validator, schema snapshot types, and the
//! rule-based planner. The executor lives in `axon-cypher`, which depends on
//! this crate.

pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod planner;
pub mod schema;
pub mod validator;

pub use error::CypherError;
pub use planner::plan;
pub use schema::SchemaSnapshot;
pub use validator::validate;

/// Parse a Cypher query string into an [`ast::Query`].
///
/// Returns [`CypherError::Parse`] if the query is malformed or contains
/// constructs outside the supported V1 subset.
pub fn parse(input: &str) -> Result<ast::Query, CypherError> {
    let tokens = lexer::tokenize(input)?;
    parser::Parser::new(tokens).parse_query()
}

/// Distinct node-pattern labels referenced by a query's `MATCH` clauses.
///
/// Each label names a collection the query reads. Serializable auto-capture
/// (FEAT-008 TXN-05) records a scan read for each so a concurrent membership
/// change aborts the transaction. Sorted and deduplicated; relationship *types*
/// are not labels (links live in one collection — see
/// [`references_relationships`]).
pub fn referenced_labels(query: &ast::Query) -> Vec<String> {
    let mut labels = std::collections::BTreeSet::new();
    for clause in &query.matches {
        for pattern in &clause.patterns {
            if let Some(label) = &pattern.start.label {
                labels.insert(label.clone());
            }
            for step in &pattern.steps {
                if let Some(label) = &step.node.label {
                    labels.insert(label.clone());
                }
            }
        }
    }
    labels.into_iter().collect()
}

/// Whether any `MATCH` clause traverses a relationship.
///
/// When `true`, the query's result depends on link membership, so a serializable
/// guard should also record the links collection (a new/removed link is a
/// traversal phantom).
pub fn references_relationships(query: &ast::Query) -> bool {
    query
        .matches
        .iter()
        .any(|clause| clause.patterns.iter().any(|p| !p.steps.is_empty()))
}

#[cfg(test)]
mod read_footprint_tests {
    use super::*;

    #[test]
    fn referenced_labels_are_distinct_and_sorted() {
        let q = parse("MATCH (t:tasks), (u:users) RETURN t, u").expect("parse");
        assert_eq!(
            referenced_labels(&q),
            vec!["tasks".to_string(), "users".to_string()]
        );
        assert!(!references_relationships(&q));
    }

    #[test]
    fn referenced_labels_include_relationship_targets() {
        let q = parse("MATCH (a:tasks)-[:depends_on]->(b:tasks) RETURN b").expect("parse");
        // Both ends share the label `tasks` → deduplicated to one.
        assert_eq!(referenced_labels(&q), vec!["tasks".to_string()]);
        assert!(references_relationships(&q));
    }

    #[test]
    fn unlabeled_nodes_are_ignored() {
        let q = parse("MATCH (n) RETURN n").expect("parse");
        assert!(referenced_labels(&q).is_empty());
    }
}
