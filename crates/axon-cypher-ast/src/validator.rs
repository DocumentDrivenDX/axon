//! Schema validation for parsed queries.
//!
//! Walks the AST and checks that every label, property, and relationship
//! type referenced exists in the active schema snapshot. Returns the first
//! violation as [`CypherError::UnknownIdentifier`]; future iterations may
//! collect multiple errors.

use crate::ast::{
    Expression, FunctionArg, MatchClause, NodePattern, PathPattern, Query, RelationshipPattern,
    Subquery,
};
use crate::error::CypherError;
use crate::schema::SchemaSnapshot;
use std::collections::HashMap;

/// Validate a parsed query against the schema snapshot.
///
/// Returns `Ok(())` if every label/property/relationship reference resolves;
/// otherwise returns the first unknown-identifier error encountered.
pub fn validate(query: &Query, schema: &SchemaSnapshot) -> Result<(), CypherError> {
    let mut env = Env::new();

    // Pass 1: collect all variable bindings (variable → label) introduced
    // by MATCH clauses. We need these before WHERE/RETURN so property
    // accesses like `b.status` can resolve back to the bound label.
    for m in &query.matches {
        bind_match_clause(m, schema, &mut env)?;
    }

    // Pass 2: validate WHERE and RETURN expressions against the bindings.
    if let Some(expr) = &query.where_clause {
        validate_expression(expr, schema, &env)?;
    }
    for item in &query.return_clause.items {
        validate_expression(&item.expression, schema, &env)?;
    }
    for sort in &query.order_by {
        validate_expression(&sort.expression, schema, &env)?;
    }

    Ok(())
}

#[derive(Debug, Default)]
struct Env {
    /// Variable name → bound label (if known). Variables without a label
    /// are present in the map with `None` so that property accesses against
    /// them are caught as ambiguous.
    bindings: HashMap<String, Option<String>>,
}

impl Env {
    fn new() -> Self {
        Self::default()
    }

    fn bind(&mut self, var: &str, label: Option<String>) {
        // Last write wins; later patterns refining the label are accepted.
        self.bindings.insert(var.to_string(), label);
    }

    fn label_of(&self, var: &str) -> Option<&Option<String>> {
        self.bindings.get(var)
    }
}

fn bind_match_clause(
    clause: &MatchClause,
    schema: &SchemaSnapshot,
    env: &mut Env,
) -> Result<(), CypherError> {
    for pattern in &clause.patterns {
        bind_path_pattern(pattern, schema, env)?;
    }
    Ok(())
}

fn bind_path_pattern(
    pattern: &PathPattern,
    schema: &SchemaSnapshot,
    env: &mut Env,
) -> Result<(), CypherError> {
    bind_node_pattern(&pattern.start, schema, env)?;
    for step in &pattern.steps {
        validate_relationship_pattern(&step.relationship, schema)?;
        if let Some(var) = &step.relationship.variable {
            env.bind(var, None); // relationship variables don't carry labels
        }
        bind_node_pattern(&step.node, schema, env)?;
    }
    Ok(())
}

fn bind_node_pattern(
    node: &NodePattern,
    schema: &SchemaSnapshot,
    env: &mut Env,
) -> Result<(), CypherError> {
    if let Some(label) = &node.label {
        if !schema.has_label(label) {
            return Err(CypherError::UnknownIdentifier {
                kind: "label",
                name: label.clone(),
            });
        }
        // Validate inline property predicates.
        let label_def = schema.label(label).expect("label presence checked above");
        for prop in &node.properties {
            if !label_def.properties.contains_key(&prop.key) {
                return Err(CypherError::UnknownIdentifier {
                    kind: "property",
                    name: format!("{}.{}", label, prop.key),
                });
            }
        }
    }
    if let Some(var) = &node.variable {
        env.bind(var, node.label.clone());
    }
    Ok(())
}

fn validate_relationship_pattern(
    rel: &RelationshipPattern,
    schema: &SchemaSnapshot,
) -> Result<(), CypherError> {
    for ty in &rel.types {
        if !schema.has_relationship(ty) {
            return Err(CypherError::UnknownIdentifier {
                kind: "relationship_type",
                name: ty.clone(),
            });
        }
    }
    Ok(())
}

fn validate_expression(
    expr: &Expression,
    schema: &SchemaSnapshot,
    env: &Env,
) -> Result<(), CypherError> {
    match expr {
        Expression::Literal(_) | Expression::Parameter(_) => Ok(()),
        Expression::Variable(name) => {
            if env.label_of(name).is_none() {
                return Err(CypherError::UnknownIdentifier {
                    kind: "variable",
                    name: name.clone(),
                });
            }
            Ok(())
        }
        Expression::Property { variable, path } => {
            let label = env
                .label_of(variable)
                .ok_or_else(|| CypherError::UnknownIdentifier {
                    kind: "variable",
                    name: variable.clone(),
                })?;
            // Property access requires a known label so we can resolve
            // the property. Untyped nodes (no `:Label` in the pattern)
            // are conservatively rejected for now; a future iteration
            // could allow them and validate at runtime.
            let label_name = label
                .as_ref()
                .ok_or_else(|| CypherError::UnknownIdentifier {
                    kind: "untyped variable property access",
                    name: variable.clone(),
                })?;
            let label_def =
                schema
                    .label(label_name)
                    .ok_or_else(|| CypherError::UnknownIdentifier {
                        kind: "label",
                        name: label_name.clone(),
                    })?;
            // Only validate the first segment; nested JSON path navigation
            // beyond declared properties is allowed (entity bodies are
            // opaque per ADR-010 §Entity Data Opacity).
            let first = path.first().ok_or_else(|| CypherError::Parse {
                line: 0,
                column: 0,
                message: "property access without a property name".to_string(),
            })?;
            if !label_def.properties.contains_key(first) {
                return Err(CypherError::UnknownIdentifier {
                    kind: "property",
                    name: format!("{}.{}", label_name, first),
                });
            }
            Ok(())
        }
        Expression::BinaryLogical { left, right, .. } => {
            validate_expression(left, schema, env)?;
            validate_expression(right, schema, env)
        }
        Expression::Not(inner) => validate_expression(inner, schema, env),
        Expression::Comparison { left, right, .. } => {
            validate_expression(left, schema, env)?;
            validate_expression(right, schema, env)
        }
        Expression::IsNull { expression, .. } => validate_expression(expression, schema, env),
        Expression::Exists(sq) | Expression::NotExists(sq) => validate_subquery(sq, schema, env),
        Expression::FunctionCall { name, arguments } => {
            validate_function_call(name, arguments, schema, env)
        }
    }
}

fn validate_function_call(
    name: &str,
    arguments: &[FunctionArg],
    schema: &SchemaSnapshot,
    env: &Env,
) -> Result<(), CypherError> {
    // V1 supported functions: count, sum, avg, min, max, collect, type, id.
    let supported = matches!(
        name.to_ascii_lowercase().as_str(),
        "count" | "sum" | "avg" | "min" | "max" | "collect" | "type" | "id"
    );
    if !supported {
        return Err(CypherError::UnsupportedClause(format!(
            "function {}()",
            name
        )));
    }
    for arg in arguments {
        if let FunctionArg::Expression(expr) = arg {
            validate_expression(expr, schema, env)?;
        }
    }
    Ok(())
}

fn validate_subquery(
    sq: &Subquery,
    schema: &SchemaSnapshot,
    outer: &Env,
) -> Result<(), CypherError> {
    // Subqueries inherit outer variable bindings (so `EXISTS { (b)-[...]->(d) }`
    // can reference the outer `b`) but introduce their own as well.
    let mut env = Env {
        bindings: outer.bindings.clone(),
    };
    for m in &sq.matches {
        bind_match_clause(m, schema, &mut env)?;
    }
    if let Some(expr) = &sq.where_clause {
        validate_expression(expr, schema, &env)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::Parser;
    use crate::schema::test_fixtures;

    fn parse_and_validate(input: &str) -> Result<(), CypherError> {
        let tokens = tokenize(input)?;
        let query = Parser::new(tokens).parse_query()?;
        validate(&query, &test_fixtures::ddx_beads())
    }

    #[test]
    fn ddx_ready_queue_validates() {
        parse_and_validate(
            r"
            MATCH (b:DdxBead {status: 'open'})
            WHERE NOT EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
                WHERE d.status <> 'closed'
            }
            RETURN b
            ORDER BY b.priority DESC, b.updated_at DESC
            LIMIT 100
        ",
        )
        .unwrap();
    }

    #[test]
    fn unknown_label_rejected() {
        let err = parse_and_validate("MATCH (b:UnknownLabel) RETURN b").unwrap_err();
        assert!(matches!(err, CypherError::UnknownIdentifier { kind, name }
            if kind == "label" && name == "UnknownLabel"));
    }

    #[test]
    fn unknown_property_in_inline_predicate_rejected() {
        let err = parse_and_validate("MATCH (b:DdxBead {bogus: 'x'}) RETURN b").unwrap_err();
        assert!(matches!(err, CypherError::UnknownIdentifier { kind, .. } if kind == "property"));
    }

    #[test]
    fn unknown_relationship_rejected() {
        let err = parse_and_validate("MATCH (a:DdxBead)-[:UNKNOWN_REL]->(b:DdxBead) RETURN a")
            .unwrap_err();
        assert!(matches!(err, CypherError::UnknownIdentifier { kind, .. }
            if kind == "relationship_type"));
    }

    #[test]
    fn unknown_property_in_where_rejected() {
        let err = parse_and_validate("MATCH (b:DdxBead) WHERE b.bogus = 'x' RETURN b").unwrap_err();
        assert!(matches!(err, CypherError::UnknownIdentifier { kind, .. } if kind == "property"));
    }

    #[test]
    fn untyped_variable_property_access_rejected() {
        // No label on `n`, so `n.field` is conservatively rejected.
        let err = parse_and_validate("MATCH (n) WHERE n.x = 1 RETURN n").unwrap_err();
        assert!(matches!(err, CypherError::UnknownIdentifier { .. }));
    }

    #[test]
    fn unsupported_function_rejected() {
        let err = parse_and_validate("MATCH (b:DdxBead) RETURN bogus(b)").unwrap_err();
        assert!(matches!(err, CypherError::UnsupportedClause(_)));
    }

    #[test]
    fn outer_binding_visible_in_subquery() {
        // `b` is bound in the outer MATCH and referenced (without re-labeling)
        // in the EXISTS subquery's relationship pattern.
        parse_and_validate(
            r"
            MATCH (b:DdxBead)
            WHERE EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
            }
            RETURN b
        ",
        )
        .unwrap();
    }
}
