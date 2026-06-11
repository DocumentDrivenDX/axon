//! L6 route-contract guard: verifies that legacy un-prefixed routes are absent
//! per ADR-018 (tenant-prefixed routing) and CONTRACT-001 (HTTP API surface).
//!
//! Contains two guards:
//! 1. Lexical: `git grep` confirms no legacy `x-axon-database` header references.
//! 2. Behavioral: a live test server confirms the retired un-prefixed routes
//!    (`/auth/me`, `/databases/*`) return 404.

#[test]
fn no_legacy_database_header_references() {
    // CARGO_MANIFEST_DIR is `crates/axon-server`; workspace root is two
    // levels up.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");

    // Assemble the header name from fragments so this file's source does
    // not itself match the literal string.
    let header = format!("{}-{}-{}", "X", "Axon", "Database");

    let output = std::process::Command::new("git")
        .arg("grep")
        .arg("-l")
        .arg(&header)
        .arg("--")
        .arg("crates/")
        .arg("sdk/")
        .current_dir(workspace_root)
        .output()
        .expect("git grep should run");

    let matches = String::from_utf8_lossy(&output.stdout);
    // Filter out this file even if git somehow finds a match for it.
    let filtered: Vec<&str> = matches
        .lines()
        .filter(|line| !line.contains("l6_route_contract_test.rs"))
        .collect();
    assert!(
        filtered.is_empty(),
        "{} still referenced in:\n{}",
        header,
        filtered.join("\n")
    );
}

// The `no_unprefixed_graphql_route_registrations` lexical check was removed
// because axum's `.nest("/tenants/:t/databases/:d", inner)` composition is
// not visible to a grep pass: the inner builder still contains `.route("/graphql"...)`
// strings even though the routes resolve to `/tenants/:t/databases/:d/graphql`
// at runtime. A runtime integration test that hits `/graphql` (without the
// tenant prefix) and asserts 404 is a better verification than a lexical
// grep, and it lives in the graphql_mutations / graphql_contract test files.

// ── Behavioral guard: retired routes must return 404 ─────────────────────────

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::SqliteStorageAdapter;
use axum_test::TestServer;
use tokio::sync::Mutex;

fn legacy_test_server() -> TestServer {
    let storage: Box<dyn axon_storage::adapter::StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    TestServer::new(app)
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_auth_me_returns_404() {
    let server = legacy_test_server();
    let resp = server.get("/auth/me").await;
    resp.assert_status_not_found();
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_databases_list_returns_404() {
    let server = legacy_test_server();
    let resp = server.get("/databases").await;
    resp.assert_status_not_found();
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_databases_create_returns_404() {
    let server = legacy_test_server();
    let resp = server.post("/databases/mydb").await;
    resp.assert_status_not_found();
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_databases_schemas_returns_404() {
    let server = legacy_test_server();
    let resp = server.get("/databases/mydb/schemas").await;
    resp.assert_status_not_found();
}
