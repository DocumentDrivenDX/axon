//! Path-router primitive: extracts (tenant, database) from data-plane URLs.
//!
//! This module provides:
//! - [`extract_tenant_database`] — pure function, parses a URL path
//! - [`ResolvedPath`] — typed extension installed into the request
//! - [`path_router_layer`] — thin axum middleware that calls the above

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Resolved (tenant, database) pair extracted from the URL path.
///
/// Installed as a request extension by [`path_router_layer`] for any
/// data-plane path that matches `/tenants/{tenant}/databases/{database}/…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub tenant: String,
    pub database: String,
}

/// Reserved path prefixes that bypass extraction (return `None` cleanly).
const RESERVED_PREFIXES: &[&str] = &[
    "/health",
    "/metrics",
    "/ui",
    "/control",
    "/favicon.ico",
    "/robots.txt",
];

/// Validate an identifier segment against the data-plane naming rules:
/// - 1–63 characters
/// - ASCII only: `[a-zA-Z0-9_-]`
/// - Cannot start with a digit
/// - Cannot be empty
fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() || s.len() > 63 {
        return false;
    }
    let mut chars = s.chars();
    // First character must not be a digit
    match chars.next() {
        Some(c) if c.is_ascii_digit() => return false,
        Some(c) if !matches!(c, 'a'..='z' | 'A'..='Z' | '_' | '-') => return false,
        None => return false,
        _ => {}
    }
    // Remaining characters: alphanumeric, underscore, or hyphen
    chars.all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-'))
}

/// Extract `(tenant, database)` from a data-plane URL path.
///
/// Matches paths of the form `/tenants/{tenant}/databases/{database}(/…)?`.
///
/// Returns `None` for:
/// - Reserved prefixes (`/health`, `/metrics`, `/ui`, `/control`, `/favicon.ico`, `/robots.txt`)
/// - Paths that do not start with `/tenants/`
/// - Paths that match `/tenants/…` but are malformed (missing segments, wrong literals)
/// - Paths with identifiers that fail the naming rule
///
/// The caller is responsible for emitting 404 when the extension is absent.
pub fn extract_tenant_database(path: &str) -> Option<(String, String)> {
    // Check reserved prefixes first
    for prefix in RESERVED_PREFIXES {
        if path == *prefix || path.starts_with(&format!("{}/", prefix)) {
            return None;
        }
    }

    // Must start with /tenants/
    let rest = path.strip_prefix("/tenants/")?;

    // Extract tenant segment (up to next '/')
    let (tenant, after_tenant) = rest.split_once('/')?;

    if !is_valid_identifier(tenant) {
        return None;
    }

    // Must have "databases/" next
    let after_databases = after_tenant.strip_prefix("databases/")?;

    // Extract database segment (up to next '/' or end of string)
    let database = match after_databases.split_once('/') {
        Some((db, _rest)) => db,
        None => after_databases,
    };

    if !is_valid_identifier(database) {
        return None;
    }

    Some((tenant.to_string(), database.to_string()))
}

/// Returns `true` when `path` has the data-plane *shape*
/// `/tenants/{t}/databases/{d}(/…)?` — regardless of whether the `{t}`/`{d}`
/// segments are valid identifiers.
///
/// This distinguishes a *malformed* data-plane request (which must be rejected
/// with 404 rather than silently fall back to the default/master database)
/// from a genuinely non-data-plane path (health, metrics, ui, control, …).
pub fn is_data_plane_path(path: &str) -> bool {
    for prefix in RESERVED_PREFIXES {
        if path == *prefix || path.starts_with(&format!("{}/", prefix)) {
            return false;
        }
    }

    let Some(rest) = path.strip_prefix("/tenants/") else {
        return false;
    };
    let Some((tenant, after_tenant)) = rest.split_once('/') else {
        return false;
    };
    if tenant.is_empty() {
        return false;
    }
    let Some(after_databases) = after_tenant.strip_prefix("databases/") else {
        return false;
    };
    let database = match after_databases.split_once('/') {
        Some((db, _rest)) => db,
        None => after_databases,
    };
    !database.is_empty()
}

/// Axum middleware layer that extracts `(tenant, database)` from the URL path
/// and installs a [`ResolvedPath`] extension for downstream handlers.
///
/// This layer **always** calls `next` — it never short-circuits with an error.
/// Routing decisions (e.g. 404 for unknown `/tenants/*` paths) are handled
/// by the router's route table, not here.
pub async fn path_router_layer(mut req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if let Some((tenant, database)) = extract_tenant_database(&path) {
        req.extensions_mut()
            .insert(ResolvedPath { tenant, database });
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_data_plane_path_extracts_and_has_shape() {
        let path = "/tenants/acme/databases/default/collections/users";
        assert_eq!(
            extract_tenant_database(path),
            Some(("acme".to_string(), "default".to_string()))
        );
        assert!(is_data_plane_path(path));
    }

    #[test]
    fn overlong_tenant_segment_is_shape_but_does_not_extract() {
        let long = "a".repeat(64);
        let path = format!("/tenants/{long}/databases/default/collections/users");
        // > 63 chars: fails the identifier rule, so extraction returns None…
        assert_eq!(extract_tenant_database(&path), None);
        // …but it is unmistakably a data-plane request and must be rejected,
        // not routed to the default/master database.
        assert!(is_data_plane_path(&path));
    }

    #[test]
    fn invalid_char_tenant_segment_is_shape_but_does_not_extract() {
        let path = "/tenants/bad%20tenant!/databases/default/x";
        assert_eq!(extract_tenant_database(path), None);
        assert!(is_data_plane_path(path));
    }

    #[test]
    fn non_data_plane_paths_are_not_shape() {
        for p in [
            "/health",
            "/metrics",
            "/ui/x",
            "/control/tenants",
            "/favicon.ico",
            "/tenants",
            "/tenants/acme",
            "/tenants/acme/foo",
            "/tenants/acme/databases/",
        ] {
            assert!(!is_data_plane_path(p), "{p} should not be data-plane shape");
            assert_eq!(extract_tenant_database(p), None, "{p} should not extract");
        }
    }
}
