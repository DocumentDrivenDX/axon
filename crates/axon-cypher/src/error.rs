use thiserror::Error;

/// Errors raised by the Cypher parser, planner, and executor.
///
/// These map to the stable error codes documented in ADR-021 and FEAT-009
/// (US-076 ad-hoc query). GraphQL/MCP surfaces translate these into the
/// caller-visible structured-error shape.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CypherError {
    /// Lexer or parser failure. The message describes the position and
    /// the expected/actual tokens.
    #[error("parse error at line {line}, column {column}: {message}")]
    Parse {
        line: usize,
        column: usize,
        message: String,
    },

    /// A keyword, clause, or function appears in the input that is not part
    /// of the V1 subset (per ADR-021's exclusion list).
    #[error("unsupported clause: {0}")]
    UnsupportedClause(String),

    /// A reference to a label, property, or relationship type that does
    /// not exist in the active schema. Caught at parse time when schema
    /// context is provided; otherwise caught at plan time.
    #[error("unknown identifier in schema: {kind}: {name}")]
    UnknownIdentifier { kind: &'static str, name: String },

    /// Planner could not produce an execution plan within configured cost
    /// or index-coverage limits.
    #[error("unsupported query plan: {0}")]
    UnsupportedQueryPlan(String),

    /// The named query (or ad-hoc query) requires policy bypass to be
    /// useful — it would only return rows hidden by the active policy.
    #[error("policy required bypass: {0}")]
    PolicyRequiredBypass(String),

    /// The query's planned worst-case cardinality exceeds the configured
    /// budget.
    #[error("query too large: {0}")]
    QueryTooLarge(String),

    /// The query exceeded its wall-clock budget during execution.
    #[error("query timeout: {0}")]
    QueryTimeout(String),
}

impl CypherError {
    /// Stable error code for use in structured GraphQL/MCP error payloads.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse { .. } => "parse_error",
            Self::UnsupportedClause(_) => "unsupported_clause",
            Self::UnknownIdentifier { .. } => "unknown_identifier",
            Self::UnsupportedQueryPlan(_) => "unsupported_query_plan",
            Self::PolicyRequiredBypass(_) => "policy_required_bypass",
            Self::QueryTooLarge(_) => "query_too_large",
            Self::QueryTimeout(_) => "query_timeout",
        }
    }
}
