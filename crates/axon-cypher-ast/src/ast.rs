//! Abstract syntax tree for the supported openCypher subset.
//!
//! The AST represents only the V1 subset specified in ADR-021. Excluded
//! clauses (write clauses, `shortestPath`, `CALL`, etc.) have no AST
//! representation — they are rejected at the lexer/parser boundary.

use serde::{Deserialize, Serialize};

/// A complete Cypher query: one or more `MATCH`/`OPTIONAL MATCH` clauses
/// followed by an optional `WHERE`, then `RETURN` with optional ordering
/// and pagination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub matches: Vec<MatchClause>,
    pub where_clause: Option<Expression>,
    pub return_clause: ReturnClause,
    pub order_by: Vec<SortItem>,
    pub skip: Option<u64>,
    pub limit: Option<u64>,
}

/// `MATCH` or `OPTIONAL MATCH`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchClause {
    pub optional: bool,
    pub patterns: Vec<PathPattern>,
}

/// A path pattern: a node, then alternating relationships and nodes.
///
/// Examples:
/// - `(b:Bead)` — single node, no relationships
/// - `(a)-[:R]->(b)` — node, outgoing rel, node
/// - `(a)-[:R*1..3]->(b)<-[:S]-(c)` — multi-hop, mixed direction
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathPattern {
    pub start: NodePattern,
    pub steps: Vec<PathStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathStep {
    pub relationship: RelationshipPattern,
    pub node: NodePattern,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodePattern {
    pub variable: Option<String>,
    pub label: Option<String>,
    pub properties: Vec<PropertyEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipPattern {
    pub variable: Option<String>,
    pub direction: Direction,
    /// `:TYPE` or `:TYPE_A|TYPE_B` — empty means any type.
    pub types: Vec<String>,
    pub properties: Vec<PropertyEntry>,
    pub range: Option<HopRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// `-[:R]->`
    Outgoing,
    /// `<-[:R]-`
    Incoming,
    /// `-[:R]-`
    Either,
}

/// `*N..M` — explicit lower and upper bounds. ADR-021 forbids unbounded
/// quantifiers in V1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopRange {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyEntry {
    pub key: String,
    pub value: Expression,
}

/// `RETURN [DISTINCT] item [, item]*`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnClause {
    pub distinct: bool,
    pub items: Vec<ReturnItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnItem {
    pub expression: Expression,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortItem {
    pub expression: Expression,
    pub descending: bool,
}

/// Expression tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expression {
    /// Literal value (number, string, bool, null, list, map).
    Literal(Literal),
    /// Bare variable reference: `n`.
    Variable(String),
    /// Property access: `n.field` or `n.field.nested`.
    Property { variable: String, path: Vec<String> },
    /// Parameter reference: `$paramName`.
    Parameter(String),
    /// `EXISTS { ... }` — subquery existence check.
    Exists(Box<Subquery>),
    /// `NOT EXISTS { ... }`.
    NotExists(Box<Subquery>),
    /// Logical `AND`/`OR`.
    BinaryLogical {
        op: LogicalOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    /// Logical `NOT`.
    Not(Box<Expression>),
    /// Comparison.
    Comparison {
        op: ComparisonOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    /// `expr IS NULL` / `expr IS NOT NULL`.
    IsNull {
        expression: Box<Expression>,
        negated: bool,
    },
    /// Function call: `count(*)`, `count(n)`, `sum(n.x)`, etc.
    FunctionCall {
        name: String,
        arguments: Vec<FunctionArg>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FunctionArg {
    /// `count(*)`
    Star,
    /// Any other expression argument.
    Expression(Expression),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(Vec<Expression>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    In,
    Contains,
    StartsWith,
    EndsWith,
}

/// A subquery used inside `EXISTS { ... }` — a `MATCH` clause with an
/// optional `WHERE`. No `RETURN`, no ordering, no pagination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subquery {
    pub matches: Vec<MatchClause>,
    pub where_clause: Option<Expression>,
}
