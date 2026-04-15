//! SCN-011 — Cross-tenant isolation (path-based routing).
//!
//! Verifies that entities created under one `(tenant, database)` pair are not
//! visible when fetching via a different tenant or a different database.
//!
//! Architecture:
//!   - `TenantRouter::new(data_dir, default_handler)` — multi-tenant SQLite
//!     mode.  Each composite slug derived from the URL path (`{tenant}:{database}`)
//!     gets its own physically isolated SQLite file under `data_dir/tenants/`.
//!   - `resolve_tenant_handler` maps the URL path to the slug, so:
//!       `/tenants/acme/databases/default/…` → `"acme:default"` (handler A)
//!       `/tenants/beta/databases/default/…` → `"beta:default"` (handler B)
//!       `/tenants/acme/databases/extra/…`   → `"acme:extra"`   (handler C)
//!
//! Note: we use `databases/default` for the entity-creation path because
//! `qualify_collection_name` short-circuits for the `"default"` database name
//! (no namespace pre-initialisation required).  The READ paths for handlers B
//! and C are fresh databases with no data, so they return 404 regardless of
//! namespace state.
//!
//! Scenario:
//!   1. Create collection `orders` and entity `orders/order-1` under
//!      `acme/default`.
//!   2. Fetching the entity via `acme/default` → 200 OK.
//!   3. Fetching the same entity path via `beta/default` → 404 (different
//!      tenant, separate handler with no data).
//!   4. Fetching the same entity path via `acme/extra` → 404 (different
//!      database, separate handler with no data).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use serde_json::json;
use tokio::sync::Mutex;

/// Build a multi-tenant test server backed by a temporary directory so that
/// each `{tenant}:{database}` composite slug gets a physically isolated SQLite
/// database file.
fn make_multi_tenant_server(tmp: &std::path::Path) -> axum_test::TestServer {
    let default_storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let default_handler = Arc::new(Mutex::new(AxonHandler::new(default_storage)));
    let tenant_router = Arc::new(TenantRouter::new(tmp.to_path_buf(), default_handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// An entity created under `acme/default` is accessible to `acme/default` but
/// invisible to `beta/default` (different tenant) and `acme/extra` (different
/// database), confirming physical isolation between slugs.
#[tokio::test]
async fn cross_tenant_entity_is_not_visible_from_other_tenants() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let http = make_multi_tenant_server(tmp.path());

    // Step 1: create the collection and entity under acme/default.
    // Using `databases/default` avoids namespace pre-initialisation — the
    // `qualify_collection_name` function returns an unqualified collection ID
    // for the "default" database, which is always resolvable in a fresh
    // SQLite handler.
    http.post("/tenants/acme/databases/default/collections/orders")
        .json(&json!({
            "schema": {
                "collection": "orders",
                "version": 1
            }
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    http.post("/tenants/acme/databases/default/entities/orders/order-1")
        .json(&json!({"data": {"title": "acme order"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // Step 2: the entity is readable under its own tenant/database.
    http.get("/tenants/acme/databases/default/entities/orders/order-1")
        .await
        .assert_status_ok();

    // Step 3: the same entity path under a different tenant is not found.
    // `beta:default` is a fresh SQLite database (slug `"beta:default"`)
    // with no collections or entities.
    http.get("/tenants/beta/databases/default/entities/orders/order-1")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Step 4: the same entity path under a different database (same tenant)
    // is not found.  `acme:extra` (slug `"acme:extra"`) is also a fresh
    // SQLite database with no data.
    http.get("/tenants/acme/databases/extra/entities/orders/order-1")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}
