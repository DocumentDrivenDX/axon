use std::collections::HashMap;

use axon_core::auth::{AuthError, Grants, Op, TenantRole};
use axon_core::error::AxonError;
use axon_core::id::CollectionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Authenticated caller context for typed administrative audit APIs.
///
/// This is deliberately separate from generic collection access. Callers must
/// carry explicit administrative authority for the requested tenant/database or
/// deployment-wide administrative authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdministrativeAuditCaller {
    /// Stable user identity for the authenticated principal.
    pub user_id: String,
    /// Tenant bound to the credential or session.
    pub tenant_id: String,
    /// Tenant role resolved at request start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_role: Option<TenantRole>,
    /// Database grants bound to the credential.
    pub grants: Grants,
    /// Deployment administrators may query administrative audit across tenants.
    #[serde(default)]
    pub deployment_admin: bool,
}

/// Administrative capability that authorized a typed system-audit query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAuditAuthorization {
    TenantAdmin,
    DeploymentAdmin,
}

/// Error returned by typed system-audit queries.
#[derive(Debug)]
pub enum SystemAuditQueryError {
    Auth(AuthError),
    Query(AxonError),
}

impl std::fmt::Display for SystemAuditQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(err) => write!(f, "{err}"),
            Self::Query(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SystemAuditQueryError {}

impl From<AuthError> for SystemAuditQueryError {
    fn from(value: AuthError) -> Self {
        Self::Auth(value)
    }
}

impl From<AxonError> for SystemAuditQueryError {
    fn from(value: AxonError) -> Self {
        Self::Query(value)
    }
}

/// Authorize a typed system-audit query without consulting generic collection
/// access rules.
pub fn authorize_system_audit_query(
    caller: Option<&AdministrativeAuditCaller>,
    tenant_id: &str,
    database: &str,
) -> Result<SystemAuditAuthorization, AuthError> {
    let caller = caller.ok_or(AuthError::Unauthenticated)?;

    if caller.deployment_admin {
        return Ok(SystemAuditAuthorization::DeploymentAdmin);
    }

    if caller.tenant_id != tenant_id {
        return Err(AuthError::CredentialWrongTenant);
    }

    if caller.tenant_role != Some(TenantRole::Admin) {
        return Err(AuthError::OpNotGranted);
    }

    let database_grant = caller
        .grants
        .find_database(database)
        .ok_or(AuthError::DatabaseNotGranted)?;
    if !database_grant.has_op(Op::Admin) {
        return Err(AuthError::OpNotGranted);
    }

    Ok(SystemAuditAuthorization::TenantAdmin)
}

/// Request-local subject snapshot used for FEAT-029 policy evaluation.
///
/// The snapshot is resolved once at request start. Collection-backed
/// attributes are deliberately stored on this value instead of a handler-level
/// cache so subject data cannot leak between requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicySubjectSnapshot {
    /// Audit-facing actor name for this request.
    pub actor: String,
    /// Canonical and policy-declared subject bindings.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub bindings: HashMap<String, Value>,
    /// Request-scoped `subject.attributes.*` values resolved from identity
    /// attribute sources.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, Value>,
}

/// Schema/policy version snapshot used by one AxonHandler operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyRequestSnapshot {
    /// Collection the operation was evaluated against.
    pub collection: CollectionId,
    /// Schema namespace active at request start.
    pub namespace: String,
    /// Database context active at request start.
    pub database_id: String,
    /// Tenant identity from authenticated attribution, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    /// Collection schema version active at request start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    /// Policy version active at request start. In ADR-019 V1 this follows the
    /// collection schema version when `access_control` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version: Option<u32>,
    /// Resolved policy subject for this request.
    pub subject: PolicySubjectSnapshot,
}
