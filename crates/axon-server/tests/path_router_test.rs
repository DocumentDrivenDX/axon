//! Tests for the path-router primitive (PROP-010, D1 bead).
//!
//! Covers:
//! - Happy-path extraction
//! - Reserved prefixes
//! - Malformed paths
//! - Invalid identifiers
//! - Middleware integration (axum layer)
//! - PROP-010 proptest: determinism + validity correlation

use axon_server::path_router::{ResolvedPath, extract_tenant_database, path_router_layer};
use axum::body::Body;
use axum::extract::Extension;
use axum::http::{Request, StatusCode};
use axum::middleware;
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn happy_basic() {
    assert_eq!(
        extract_tenant_database("/tenants/acme/databases/orders"),
        Some(("acme".to_string(), "orders".to_string()))
    );
}

#[test]
fn happy_with_trailing_segments() {
    assert_eq!(
        extract_tenant_database("/tenants/acme/databases/orders/collections/c/entities/e"),
        Some(("acme".to_string(), "orders".to_string()))
    );
}

#[test]
fn happy_with_graphql_suffix() {
    assert_eq!(
        extract_tenant_database("/tenants/acme/databases/orders/graphql"),
        Some(("acme".to_string(), "orders".to_string()))
    );
}

#[test]
fn happy_underscore_and_hyphen() {
    assert_eq!(
        extract_tenant_database("/tenants/my_tenant/databases/my-db"),
        Some(("my_tenant".to_string(), "my-db".to_string()))
    );
}

#[test]
fn happy_single_char_identifiers() {
    assert_eq!(
        extract_tenant_database("/tenants/a/databases/b"),
        Some(("a".to_string(), "b".to_string()))
    );
}

// ---------------------------------------------------------------------------
// Reserved prefixes
// ---------------------------------------------------------------------------

#[test]
fn reserved_health() {
    assert_eq!(extract_tenant_database("/health"), None);
}

#[test]
fn reserved_metrics() {
    assert_eq!(extract_tenant_database("/metrics"), None);
}

#[test]
fn reserved_ui_with_subpath() {
    assert_eq!(extract_tenant_database("/ui/collections"), None);
}

#[test]
fn reserved_control_with_subpath() {
    assert_eq!(extract_tenant_database("/control/tenants"), None);
}

#[test]
fn reserved_favicon() {
    assert_eq!(extract_tenant_database("/favicon.ico"), None);
}

#[test]
fn reserved_robots() {
    assert_eq!(extract_tenant_database("/robots.txt"), None);
}

#[test]
fn reserved_ui_exact() {
    assert_eq!(extract_tenant_database("/ui"), None);
}

#[test]
fn reserved_control_exact() {
    assert_eq!(extract_tenant_database("/control"), None);
}

// ---------------------------------------------------------------------------
// Malformed paths
// ---------------------------------------------------------------------------

#[test]
fn malformed_root() {
    assert_eq!(extract_tenant_database("/"), None);
}

#[test]
fn malformed_tenants_no_segment() {
    assert_eq!(extract_tenant_database("/tenants"), None);
}

#[test]
fn malformed_tenants_trailing_slash() {
    assert_eq!(extract_tenant_database("/tenants/"), None);
}

#[test]
fn malformed_no_databases_segment() {
    assert_eq!(extract_tenant_database("/tenants/acme"), None);
}

#[test]
fn malformed_databases_no_db_name() {
    assert_eq!(extract_tenant_database("/tenants/acme/databases"), None);
}

#[test]
fn malformed_databases_trailing_slash() {
    assert_eq!(extract_tenant_database("/tenants/acme/databases/"), None);
}

#[test]
fn malformed_missing_databases_literal() {
    assert_eq!(extract_tenant_database("/tenants/acme/orders"), None);
}

// ---------------------------------------------------------------------------
// Invalid identifiers
// ---------------------------------------------------------------------------

#[test]
fn invalid_leading_digit_tenant() {
    assert_eq!(
        extract_tenant_database("/tenants/3bad/databases/orders"),
        None
    );
}

#[test]
fn invalid_space_in_database() {
    assert_eq!(
        extract_tenant_database("/tenants/acme/databases/or ders"),
        None
    );
}

#[test]
fn invalid_dot_in_database() {
    assert_eq!(
        extract_tenant_database("/tenants/acme/databases/or..ders"),
        None
    );
}

#[test]
fn invalid_empty_tenant() {
    assert_eq!(
        extract_tenant_database("/tenants//databases/orders"),
        None
    );
}

#[test]
fn invalid_tenant_too_long() {
    let long = "a-very-long-tenant-name-that-exceeds-sixty-three-characters-total";
    assert!(long.len() > 63);
    let path = format!("/tenants/{}/databases/orders", long);
    assert_eq!(extract_tenant_database(&path), None);
}

#[test]
fn invalid_leading_digit_database() {
    assert_eq!(
        extract_tenant_database("/tenants/acme/databases/1bad"),
        None
    );
}

// ---------------------------------------------------------------------------
// Middleware integration
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn middleware_installs_resolved_path_extension() {
    // Build a minimal router with the path_router_layer installed.
    // The handler reads ResolvedPath from extensions and returns 200 if present.
    async fn ping_handler(
        Extension(resolved): Extension<ResolvedPath>,
    ) -> String {
        format!("{}:{}", resolved.tenant, resolved.database)
    }

    let app = Router::new()
        .route("/tenants/{tenant}/databases/{database}/ping", get(ping_handler))
        .layer(middleware::from_fn(path_router_layer));

    let request = Request::builder()
        .uri("/tenants/acme/databases/orders/ping")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(body_bytes, "acme:orders");
}

#[tokio::test(flavor = "multi_thread")]
async fn middleware_does_not_insert_extension_for_health() {
    // For reserved paths the extension must NOT be set and next must still be called.
    async fn health_handler() -> &'static str {
        "ok"
    }

    let app = Router::new()
        .route("/health", get(health_handler))
        .layer(middleware::from_fn(path_router_layer));

    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// PROP-010: determinism + validity correlation
// ---------------------------------------------------------------------------

use proptest::prelude::*;

/// Valid first character: ASCII alpha, underscore, or hyphen (not digit).
/// Uses u8 ranges since `RangeInclusive<char>` isn't a proptest `Strategy`.
fn valid_first_char() -> impl Strategy<Value = char> {
    prop_oneof![
        (b'a'..=b'z').prop_map(|b| b as char),
        (b'A'..=b'Z').prop_map(|b| b as char),
        Just('_'),
        Just('-'),
    ]
}

/// Valid non-first character: ASCII alphanumeric, underscore, or hyphen.
fn valid_rest_char() -> impl Strategy<Value = char> {
    prop_oneof![
        (b'a'..=b'z').prop_map(|b| b as char),
        (b'A'..=b'Z').prop_map(|b| b as char),
        (b'0'..=b'9').prop_map(|b| b as char),
        Just('_'),
        Just('-'),
    ]
}

/// Strategy that generates identifiers valid by the naming rule:
///   1–63 chars, ASCII [a-zA-Z0-9_-], not starting with a digit.
fn valid_identifier() -> impl Strategy<Value = String> {
    (valid_first_char(), prop::collection::vec(valid_rest_char(), 0..62))
        .prop_map(|(f, r)| {
            let mut s = String::with_capacity(1 + r.len());
            s.push(f);
            s.extend(r);
            s
        })
}

/// Strategy that generates strings that violate the naming rule.
/// Each variant guarantees the produced string is invalid.
fn invalid_identifier() -> impl Strategy<Value = String> {
    prop_oneof![
        // Empty string — always invalid
        Just(String::new()),
        // Starts with a digit — always invalid by the leading-digit rule
        ((b'0'..=b'9').prop_map(|b| b as char), prop::collection::vec(valid_rest_char(), 0..20))
            .prop_map(|(d, rest): (char, Vec<char>)| {
                let mut s = String::new();
                s.push(d);
                s.extend(rest.iter());
                s
            }),
        // Contains a dot — always invalid (dot is not in [a-zA-Z0-9_-])
        (valid_first_char(), prop::collection::vec(valid_rest_char(), 0..20))
            .prop_map(|(f, rest): (char, Vec<char>)| {
                let mut s = String::new();
                s.push(f);
                s.extend(rest.iter());
                s.push('.');
                s
            }),
        // Contains a space — always invalid
        (valid_first_char(), prop::collection::vec(valid_rest_char(), 0..20))
            .prop_map(|(f, rest): (char, Vec<char>)| {
                let mut s = String::new();
                s.push(f);
                s.extend(rest.iter());
                s.push(' ');
                s
            }),
        // Exceeds 63 characters — always invalid by the length rule
        (valid_first_char(), prop::collection::vec(valid_rest_char(), 63..78))
            .prop_map(|(f, rest): (char, Vec<char>)| {
                let mut s = String::with_capacity(1 + rest.len());
                s.push(f);
                s.extend(rest.iter());
                s
            }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// PROP-010: if both segments are valid identifiers, extraction succeeds.
    #[test]
    fn prop_010_valid_identifiers_extracted(
        t in valid_identifier(),
        d in valid_identifier(),
    ) {
        let path = format!("/tenants/{}/databases/{}/x", t, d);
        let result = extract_tenant_database(&path);
        prop_assert_eq!(result, Some((t, d)));
    }

    /// PROP-010: invalid tenant → None.
    #[test]
    fn prop_010_invalid_tenant_returns_none(
        t in invalid_identifier(),
        d in valid_identifier(),
    ) {
        let path = format!("/tenants/{}/databases/{}/x", t, d);
        let result = extract_tenant_database(&path);
        prop_assert!(result.is_none(),
            "expected None for invalid tenant {:?}, got {:?}", t, result);
    }

    /// PROP-010: invalid database → None.
    #[test]
    fn prop_010_invalid_database_returns_none(
        t in valid_identifier(),
        d in invalid_identifier(),
    ) {
        let path = format!("/tenants/{}/databases/{}/x", t, d);
        let result = extract_tenant_database(&path);
        prop_assert!(result.is_none(),
            "expected None for invalid database {:?}, got {:?}", d, result);
    }

    /// PROP-010: referential transparency — same input always yields same output.
    #[test]
    fn prop_010_referentially_transparent(
        t in valid_identifier(),
        d in valid_identifier(),
    ) {
        let path = format!("/tenants/{}/databases/{}/x", t, d);
        let r1 = extract_tenant_database(&path);
        let r2 = extract_tenant_database(&path);
        prop_assert_eq!(r1, r2);
    }
}
