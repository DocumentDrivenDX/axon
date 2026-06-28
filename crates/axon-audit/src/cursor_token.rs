//! Opaque, scope-bound CDC cursor tokens (CONTRACT-006 §Cursor token; ADR-025).
//!
//! Clients receive an opaque resume token instead of a raw `audit_id`. The token
//! is a pure function of `(format-version, sink, scope, audit_id)`, so it is
//! **stable across producer restarts and schema changes** (CONTRACT-006:148):
//! it embeds no timestamps, randomness, or schema-derived data. Decoding
//! validates the embedded scope against the request scope and **rejects a
//! mismatch** (CONTRACT-006:206 — the client must obtain a fresh cursor).
//!
//! Clients MUST treat the encoded string as opaque and never parse it.

use base64::Engine;
use serde::{Deserialize, Serialize};

use axon_core::error::AxonError;

/// Token wire-format version. Bump when the encoded shape changes; `decode`
/// rejects unknown versions so stale clients fail closed rather than
/// misinterpreting a token.
const TOKEN_FORMAT_VERSION: u8 = 1;

/// The scope a cursor token is bound to.
///
/// An empty string in any dimension means "unscoped" for that dimension (e.g. a
/// global, all-collections cursor uses an empty `collection`). Scope comparison
/// is exact, mirroring the `(sink, scope)` cursor identity in CONTRACT-006.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CursorScope {
    #[serde(default)]
    pub tenant: String,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub collection: String,
}

impl CursorScope {
    /// A fully-qualified scope.
    pub fn new(
        tenant: impl Into<String>,
        database: impl Into<String>,
        collection: impl Into<String>,
    ) -> Self {
        Self {
            tenant: tenant.into(),
            database: database.into(),
            collection: collection.into(),
        }
    }

    /// A scope bound only to a collection (tenant/database unscoped).
    pub fn collection(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            ..Default::default()
        }
    }
}

/// Errors from decoding or validating a [`CursorToken`].
///
/// Both variants are non-retryable rejections: the remediation is to obtain a
/// fresh cursor from a recent event (CONTRACT-006:206).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorTokenError {
    /// The token string is not a well-formed opaque cursor token (bad base64,
    /// bad JSON, or an unsupported format version).
    Malformed(String),
    /// The token's embedded scope does not match the request scope. Boxed to
    /// keep the error type small (`clippy::result_large_err`).
    ScopeMismatch {
        expected: Box<CursorScope>,
        found: Box<CursorScope>,
    },
}

impl std::fmt::Display for CursorTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CursorTokenError::Malformed(why) => write!(f, "malformed cursor token: {why}"),
            CursorTokenError::ScopeMismatch { expected, found } => write!(
                f,
                "cursor token scope {found:?} does not match request scope {expected:?}"
            ),
        }
    }
}

impl std::error::Error for CursorTokenError {}

impl From<CursorTokenError> for AxonError {
    fn from(e: CursorTokenError) -> Self {
        // Rejected, non-retryable (CONTRACT-006:206).
        AxonError::InvalidArgument(format!("invalid cursor token: {e}"))
    }
}

/// An opaque, scope-bound CDC resume token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorToken {
    #[serde(rename = "v")]
    format_version: u8,
    #[serde(rename = "s")]
    pub sink: String,
    #[serde(rename = "sc")]
    pub scope: CursorScope,
    #[serde(rename = "a")]
    pub audit_id: u64,
}

impl CursorToken {
    /// Build a token for `(sink, scope)` resuming at `audit_id`.
    pub fn new(sink: impl Into<String>, scope: CursorScope, audit_id: u64) -> Self {
        Self {
            format_version: TOKEN_FORMAT_VERSION,
            sink: sink.into(),
            scope,
            audit_id,
        }
    }

    /// Encode to an opaque, restart/schema-stable string.
    ///
    /// The encoding is a pure function of the token's fields (URL-safe base64 of
    /// a fixed-shape JSON), so the same inputs always produce the same token.
    /// Clients MUST treat the result as opaque.
    pub fn encode(&self) -> String {
        // A fixed-field struct serializes deterministically and cannot fail.
        let json = serde_json::to_vec(self).expect("CursorToken is always serializable");
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
    }

    /// Decode an opaque token, rejecting malformed input and unknown format
    /// versions.
    pub fn decode(token: &str) -> Result<Self, CursorTokenError> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|e| CursorTokenError::Malformed(format!("base64: {e}")))?;
        let parsed: CursorToken = serde_json::from_slice(&bytes)
            .map_err(|e| CursorTokenError::Malformed(format!("json: {e}")))?;
        if parsed.format_version != TOKEN_FORMAT_VERSION {
            return Err(CursorTokenError::Malformed(format!(
                "unsupported cursor token format version {}",
                parsed.format_version
            )));
        }
        Ok(parsed)
    }

    /// Decode and validate the token's scope against `expected`.
    ///
    /// Rejects a scope mismatch with [`CursorTokenError::ScopeMismatch`]
    /// (CONTRACT-006:206) so a token issued for one tenant/database/collection
    /// cannot be replayed against another.
    pub fn decode_for_scope(token: &str, expected: &CursorScope) -> Result<Self, CursorTokenError> {
        let parsed = Self::decode(token)?;
        if &parsed.scope != expected {
            return Err(CursorTokenError::ScopeMismatch {
                expected: Box::new(expected.clone()),
                found: Box::new(parsed.scope),
            });
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_encode_decode() {
        let token = CursorToken::new("kafka", CursorScope::new("acme", "prod", "invoices"), 12345);
        let encoded = token.encode();
        let decoded = CursorToken::decode(&encoded).expect("valid token decodes");
        assert_eq!(decoded, token);
        assert_eq!(decoded.audit_id, 12345);
        assert_eq!(decoded.sink, "kafka");
    }

    #[test]
    fn encoding_is_stable_across_calls_restart_safe() {
        // A pure function of the inputs: the same token always encodes
        // identically, so a producer restart resumes from the same string.
        let a = CursorToken::new("sse", CursorScope::collection("tasks"), 7).encode();
        let b = CursorToken::new("sse", CursorScope::collection("tasks"), 7).encode();
        assert_eq!(a, b);
    }

    #[test]
    fn token_is_opaque_not_plaintext() {
        // The raw audit_id / sink must not appear verbatim in the token.
        let encoded = CursorToken::new("kafka", CursorScope::collection("tasks"), 999).encode();
        assert!(!encoded.contains("kafka"));
        assert!(!encoded.contains("999"));
    }

    #[test]
    fn decode_for_scope_accepts_matching_scope() {
        let scope = CursorScope::new("acme", "prod", "invoices");
        let encoded = CursorToken::new("kafka", scope.clone(), 42).encode();
        let decoded = CursorToken::decode_for_scope(&encoded, &scope).expect("scope matches");
        assert_eq!(decoded.audit_id, 42);
    }

    #[test]
    fn decode_for_scope_rejects_mismatched_scope() {
        let issued = CursorScope::new("acme", "prod", "invoices");
        let encoded = CursorToken::new("kafka", issued, 42).encode();
        let other = CursorScope::new("evil", "prod", "invoices");
        let err = CursorToken::decode_for_scope(&encoded, &other)
            .expect_err("cross-tenant scope must be rejected");
        assert!(matches!(err, CursorTokenError::ScopeMismatch { .. }));
        // Maps to a non-retryable rejection.
        let axon: AxonError = err.into();
        assert!(matches!(axon, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn decode_rejects_malformed_tokens() {
        assert!(matches!(
            CursorToken::decode("not-valid-base64!!!"),
            Err(CursorTokenError::Malformed(_))
        ));
        // Valid base64 of non-token JSON.
        let junk = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{\"hello\":1}");
        assert!(matches!(
            CursorToken::decode(&junk),
            Err(CursorTokenError::Malformed(_))
        ));
    }

    #[test]
    fn decode_rejects_unknown_format_version() {
        // Hand-craft a token JSON with a future version.
        let future = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            br#"{"v":99,"s":"kafka","sc":{"tenant":"","database":"","collection":"tasks"},"a":1}"#,
        );
        let err = CursorToken::decode(&future).expect_err("unknown version rejected");
        assert!(matches!(err, CursorTokenError::Malformed(_)));
    }
}
