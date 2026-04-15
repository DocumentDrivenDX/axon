//! Authorization helpers for the control plane (axon-c6908e78, ADR-018).
//!
//! Two distinct authorization levels are supported:
//!
//! - **Deployment admin** – a global role recorded in the `user_roles` store.
//!   Any user whose `user_id` string maps to `Role::Admin` in that store is
//!   considered a deployment administrator.
//!
//! - **Tenant admin** – a user who either is a deployment admin *or* holds
//!   `Op::Admin` on any database within the target tenant according to their
//!   JWT grants.

use axon_core::auth::{Op, ResolvedIdentity, TenantId};
use axon_core::error::AxonError;

use crate::auth::Role;
use crate::user_roles::UserRoleStore;

/// Check that the caller is a deployment-level administrator.
///
/// Looks up `identity.user_id.0` in `user_roles`; if the resolved role is
/// [`Role::Admin`] the call succeeds, otherwise it returns
/// [`AxonError::Forbidden`].
pub fn require_deployment_admin(
    identity: &ResolvedIdentity,
    user_roles: &UserRoleStore,
) -> Result<(), AxonError> {
    match user_roles.get(&identity.user_id.0) {
        Some(Role::Admin) => Ok(()),
        _ => Err(AxonError::Forbidden("deployment admin required".into())),
    }
}

/// Check that the caller is an administrator of the named tenant.
///
/// The check passes when the caller is a deployment admin **or** when their
/// JWT grants include [`Op::Admin`] on any database for the specified tenant.
pub fn require_tenant_admin(
    identity: &ResolvedIdentity,
    tenant_id: TenantId,
    user_roles: &UserRoleStore,
) -> Result<(), AxonError> {
    // Deployment admin always passes.
    if require_deployment_admin(identity, user_roles).is_ok() {
        return Ok(());
    }

    // Tenant-scoped admin: identity's tenant must match and at least one grant
    // must carry Op::Admin.
    if identity.tenant_id == tenant_id
        && identity
            .grants
            .databases
            .iter()
            .any(|db| db.has_op(Op::Admin))
    {
        return Ok(());
    }

    Err(AxonError::Forbidden("tenant admin required".into()))
}

/// Check that the caller is either a deployment admin OR the named target user.
///
/// Used by credential issuance to allow self-issue: a non-admin user may issue
/// credentials to themselves, but not to other users.
pub fn require_deployment_admin_or_self(
    identity: &ResolvedIdentity,
    target_user_id: &axon_core::auth::UserId,
    user_roles: &UserRoleStore,
) -> Result<(), AxonError> {
    if require_deployment_admin(identity, user_roles).is_ok() {
        return Ok(());
    }
    if &identity.user_id == target_user_id {
        return Ok(());
    }
    Err(AxonError::Forbidden(
        "deployment admin or self-issue required".into(),
    ))
}
