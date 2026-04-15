//! Default-tenant auto-bootstrap per ADR-018 §6.
//!
//! On a fresh deployment with zero tenants, the first authenticated request
//! creates:
//!   1. A "default" tenant row in the auth_schema tenants table
//!   2. A "default" database in tenant_databases
//!   3. A tenant_users row adding the caller as admin
//!
//! Idempotent — runs only when storage.count_tenants() returns 0, and
//! concurrency-safe via ON CONFLICT DO NOTHING.

use axon_core::auth::{TenantId, TenantRole, UserId};
use axon_core::error::AxonError;
use axon_storage::StorageAdapter;

/// Handle returned by a successful [`ensure_default_tenant`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultTenantHandle {
    pub tenant_id: TenantId,
    pub database_name: String,
}

/// Ensure the default tenant exists for a zero-tenant deployment.
///
/// Steps:
/// 1. `upsert_default_tenant("default")` — atomically insert-or-ignore and
///    return the winning `TenantId`.  This is concurrency-safe: N racing
///    callers converge on one row.
/// 2. `count_tenants()` — if > 1, other explicit tenants exist and this is
///    NOT a bootstrap deployment; return `Err(AxonError::NotFound)` so the
///    caller uses the explicit tenant URL instead.
/// 3. `create_tenant_database(tenant_id, "default")` — idempotent; ignore
///    `AlreadyExists`.
/// 4. `upsert_tenant_member(tenant_id, caller_user_id, TenantRole::Admin)` —
///    idempotent.
/// 5. Return [`DefaultTenantHandle`].
///
/// The caller identity must already exist as a row in the `users` table
/// before this function is called (guaranteed by B1/B3 in the auth pipeline).
pub fn ensure_default_tenant(
    storage: &dyn StorageAdapter,
    caller_user_id: UserId,
) -> Result<DefaultTenantHandle, AxonError> {
    // Step 1 — upsert the "default" tenant (convergent under concurrency).
    let tenant_id = storage.upsert_default_tenant("default")?;

    // Step 2 — non-bootstrap guard.
    // After the upsert, if count > 1 there are non-default tenants present:
    // this is not a bootstrap deployment.
    let n = storage.count_tenants()?;
    if n > 1 {
        return Err(AxonError::NotFound(
            "deployment has existing tenants; use explicit tenant URL".into(),
        ));
    }

    // Step 3 — register the "default" database (idempotent).
    match storage.create_tenant_database(tenant_id.clone(), "default") {
        Ok(_) | Err(AxonError::AlreadyExists(_)) => {}
        Err(e) => return Err(e),
    }

    // Step 4 — add the caller as admin (idempotent upsert).
    storage.upsert_tenant_member(tenant_id.clone(), caller_user_id, TenantRole::Admin)?;

    Ok(DefaultTenantHandle {
        tenant_id,
        database_name: "default".to_string(),
    })
}
