//! GraphQL surface for control-plane administration.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_graphql::{
    Context, EmptySubscription, Error as GqlError, ErrorExtensions, Json as GqlJson, Object,
    Schema, SimpleObject,
};
use axon_core::auth::{
    AuthError as CoreAuthError, CredentialMetadata, Grants, JwtClaims, ResolvedIdentity,
    TenantDatabase, TenantId, TenantMember, User, UserId,
};
use axon_core::error::AxonError;
use axon_storage::StorageAdapter;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{AuthError as ServerAuthError, Identity};
use crate::auth_pipeline::JwtIssuer;
use crate::control_plane::Tenant;
use crate::control_plane_authz;
use crate::control_plane_routes::{
    is_unique_violation, is_valid_database_identifier, name_to_db_slug, now_iso8601,
    provision_tenant_database, tenant_role_from_str, tenant_role_to_str, ControlPlaneState,
};

type GqlResult<T> = async_graphql::Result<T>;
type SharedStorage = Arc<Mutex<Box<dyn StorageAdapter + Send + Sync>>>;

#[derive(Clone)]
struct MaybeResolvedIdentity(Option<ResolvedIdentity>);

#[derive(SimpleObject)]
struct ControlTenant {
    id: String,
    name: String,
    db_name: String,
    created_at: String,
    db_path: Option<String>,
}

impl ControlTenant {
    fn from_tenant(tenant: Tenant) -> Self {
        Self {
            id: tenant.id,
            name: tenant.name,
            db_name: tenant.db_name,
            created_at: tenant.created_at,
            db_path: None,
        }
    }
}

#[derive(SimpleObject)]
struct DeleteTenantPayload {
    deleted: bool,
    tenant_id: String,
    db_name: String,
}

#[derive(SimpleObject)]
struct ControlUser {
    id: String,
    display_name: String,
    email: Option<String>,
    created_at_ms: u64,
    suspended_at_ms: Option<u64>,
}

impl From<User> for ControlUser {
    fn from(user: User) -> Self {
        Self {
            id: user.id.0,
            display_name: user.display_name,
            email: user.email,
            created_at_ms: user.created_at_ms,
            suspended_at_ms: user.suspended_at_ms,
        }
    }
}

#[derive(SimpleObject)]
struct SuspendUserPayload {
    user_id: String,
    suspended: bool,
}

#[derive(SimpleObject)]
struct TenantMemberGql {
    tenant_id: String,
    user_id: String,
    role: String,
}

impl From<TenantMember> for TenantMemberGql {
    fn from(member: TenantMember) -> Self {
        Self {
            tenant_id: member.tenant_id.0,
            user_id: member.user_id.0,
            role: tenant_role_to_str(member.role).to_string(),
        }
    }
}

#[derive(SimpleObject)]
struct RemoveTenantMemberPayload {
    tenant_id: String,
    user_id: String,
    deleted: bool,
}

#[derive(SimpleObject)]
struct TenantDatabaseGql {
    tenant_id: String,
    name: String,
    created_at_ms: u64,
}

impl From<TenantDatabase> for TenantDatabaseGql {
    fn from(db: TenantDatabase) -> Self {
        Self {
            tenant_id: db.tenant_id.0,
            name: db.name,
            created_at_ms: db.created_at_ms,
        }
    }
}

#[derive(SimpleObject)]
struct DeleteTenantDatabasePayload {
    tenant_id: String,
    name: String,
    deleted: bool,
}

#[derive(SimpleObject)]
struct IssueCredentialPayload {
    jwt: String,
    jti: String,
    expires_at: u64,
}

#[derive(SimpleObject)]
struct CredentialMetadataGql {
    jti: String,
    user_id: String,
    tenant_id: String,
    issued_at_ms: i64,
    expires_at_ms: i64,
    revoked: bool,
    grants: GqlJson<serde_json::Value>,
}

impl From<CredentialMetadata> for CredentialMetadataGql {
    fn from(credential: CredentialMetadata) -> Self {
        let grants = serde_json::from_str::<serde_json::Value>(&credential.grants_json)
            .unwrap_or_else(|_| json!({ "databases": [] }));
        Self {
            jti: credential.jti,
            user_id: credential.user_id.0,
            tenant_id: credential.tenant_id.0,
            issued_at_ms: credential.issued_at_ms,
            expires_at_ms: credential.expires_at_ms,
            revoked: credential.revoked,
            grants: GqlJson(grants),
        }
    }
}

#[derive(SimpleObject)]
struct RevokeCredentialPayload {
    tenant_id: String,
    jti: String,
    revoked: bool,
}

pub async fn control_graphql_handler(
    State(state): State<ControlPlaneState>,
    Extension(legacy): Extension<Identity>,
    resolved: Option<Extension<ResolvedIdentity>>,
    req: async_graphql_axum::GraphQLRequest,
) -> Response {
    let schema = Schema::build(ControlQuery, ControlMutation, EmptySubscription)
        .data(state)
        .data(legacy)
        .data(MaybeResolvedIdentity(resolved.map(|Extension(r)| r)))
        .finish();

    let response = schema.execute(req.into_inner()).await;
    async_graphql_axum::GraphQLResponse::from(response).into_response()
}

struct ControlQuery;

#[Object]
impl ControlQuery {
    async fn tenants(&self, ctx: &Context<'_>) -> GqlResult<Vec<ControlTenant>> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;

        let db = state.db.lock().await;
        let tenants = db.list_tenants().map_err(axon_error_to_gql)?;
        Ok(tenants
            .into_iter()
            .map(ControlTenant::from_tenant)
            .collect())
    }

    async fn tenant(&self, ctx: &Context<'_>, id: String) -> GqlResult<Option<ControlTenant>> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;

        let db = state.db.lock().await;
        match db.get_tenant(&id) {
            Ok(tenant) => Ok(Some(ControlTenant::from_tenant(tenant))),
            Err(AxonError::NotFound(_)) => Ok(None),
            Err(err) => Err(axon_error_to_gql(err)),
        }
    }

    async fn users(&self, ctx: &Context<'_>) -> GqlResult<Vec<ControlUser>> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;
        let storage = storage(&state)?;

        with_storage(&storage, |s| s.list_users())
            .map(|users| users.into_iter().map(ControlUser::from).collect())
            .map_err(axon_error_to_gql)
    }

    async fn tenant_members(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
    ) -> GqlResult<Vec<TenantMemberGql>> {
        let state = state(ctx)?;
        require_tenant_admin(ctx, &state, &tenant_id)?;
        let storage = storage(&state)?;
        let tenant_id = TenantId::new(&tenant_id);

        with_storage(&storage, |s| s.list_tenant_members(tenant_id))
            .map(|members| members.into_iter().map(TenantMemberGql::from).collect())
            .map_err(axon_error_to_gql)
    }

    async fn tenant_databases(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
    ) -> GqlResult<Vec<TenantDatabaseGql>> {
        let state = state(ctx)?;
        require_tenant_admin(ctx, &state, &tenant_id)?;
        let storage = storage(&state)?;
        let tenant_id = TenantId::new(&tenant_id);

        with_storage(&storage, |s| s.list_tenant_databases(tenant_id))
            .map(|databases| databases.into_iter().map(TenantDatabaseGql::from).collect())
            .map_err(axon_error_to_gql)
    }

    async fn credentials(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
    ) -> GqlResult<Vec<CredentialMetadataGql>> {
        let state = state(ctx)?;
        let tenant_id_value = TenantId::new(&tenant_id);
        let user_filter = credential_list_filter(ctx, &state, tenant_id_value.clone())?;
        let storage = storage(&state)?;

        with_storage(&storage, |s| {
            s.list_credentials(tenant_id_value, user_filter)
        })
        .map(|credentials| {
            credentials
                .into_iter()
                .map(CredentialMetadataGql::from)
                .collect()
        })
        .map_err(axon_error_to_gql)
    }
}

struct ControlMutation;

#[Object]
impl ControlMutation {
    async fn create_tenant(&self, ctx: &Context<'_>, name: String) -> GqlResult<ControlTenant> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;

        let id = Uuid::now_v7().to_string();
        let created_at = now_iso8601();
        let db_name = name_to_db_slug(&name, &id);

        let db = state.db.lock().await;
        match db.create_tenant(&id, &name, &db_name, &created_at) {
            Ok(()) => {
                let db_path = state.tenant_db_path(&db_name);
                if let Err(err) = provision_tenant_database(&db_path) {
                    let _ = db.delete_tenant(&id);
                    return Err(axon_error_to_gql(err));
                }
                Ok(ControlTenant {
                    id,
                    name,
                    db_name,
                    created_at,
                    db_path: Some(db_path.display().to_string()),
                })
            }
            Err(err) => {
                let msg = err.to_string();
                if is_unique_violation(&msg) {
                    Err(gql_error(
                        "already_exists",
                        format!("tenant with name '{name}' already exists"),
                    ))
                } else {
                    Err(gql_error("storage_error", msg))
                }
            }
        }
    }

    async fn delete_tenant(&self, ctx: &Context<'_>, id: String) -> GqlResult<DeleteTenantPayload> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;

        let db = state.db.lock().await;
        match db.delete_tenant(&id) {
            Ok(db_name) => {
                let path = state.tenant_db_path(&db_name);
                let _ = std::fs::remove_file(&path);
                Ok(DeleteTenantPayload {
                    deleted: true,
                    tenant_id: id,
                    db_name,
                })
            }
            Err(err) => Err(axon_error_to_gql(err)),
        }
    }

    async fn provision_user(
        &self,
        ctx: &Context<'_>,
        display_name: String,
        email: Option<String>,
    ) -> GqlResult<ControlUser> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;
        let storage = storage(&state)?;
        let user_id = UserId::generate();

        with_storage(&storage, |s| {
            s.create_user(&user_id, &display_name, email.as_deref())
        })
        .map(ControlUser::from)
        .map_err(axon_error_to_gql)
    }

    async fn suspend_user(
        &self,
        ctx: &Context<'_>,
        user_id: String,
    ) -> GqlResult<SuspendUserPayload> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;
        let storage = storage(&state)?;
        let user_id_value = UserId::new(&user_id);

        with_storage(&storage, |s| s.suspend_user(&user_id_value))
            .map(|suspended| SuspendUserPayload { user_id, suspended })
            .map_err(axon_error_to_gql)
    }

    async fn upsert_tenant_member(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
        user_id: String,
        role: String,
    ) -> GqlResult<TenantMemberGql> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;
        let role_value = tenant_role_from_str(&role)
            .ok_or_else(|| gql_error("invalid_role", format!("unknown role '{role}'")))?;
        let storage = storage(&state)?;

        with_storage(&storage, |s| {
            s.upsert_tenant_member(TenantId::new(&tenant_id), UserId::new(&user_id), role_value)
        })
        .map(TenantMemberGql::from)
        .map_err(axon_error_to_gql)
    }

    async fn remove_tenant_member(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
        user_id: String,
    ) -> GqlResult<RemoveTenantMemberPayload> {
        let state = state(ctx)?;
        require_deployment_admin(ctx, &state)?;
        let storage = storage(&state)?;
        let removed = with_storage(&storage, |s| {
            s.remove_tenant_member(TenantId::new(&tenant_id), UserId::new(&user_id))
        })
        .map_err(axon_error_to_gql)?;

        if !removed {
            return Err(gql_error(
                "not_found",
                format!("member {user_id} not found in tenant {tenant_id}"),
            ));
        }

        Ok(RemoveTenantMemberPayload {
            tenant_id,
            user_id,
            deleted: true,
        })
    }

    async fn create_tenant_database(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
        name: String,
    ) -> GqlResult<TenantDatabaseGql> {
        let state = state(ctx)?;
        require_tenant_admin(ctx, &state, &tenant_id)?;
        if !is_valid_database_identifier(&name) {
            return Err(gql_error(
                "invalid_identifier",
                format!(
                    "database name '{name}' is invalid: must be 1-63 ASCII characters [a-zA-Z0-9_-] and must not start with a digit"
                ),
            ));
        }
        let storage = storage(&state)?;

        with_storage(&storage, |s| {
            s.create_tenant_database(TenantId::new(&tenant_id), &name)
        })
        .map(TenantDatabaseGql::from)
        .map_err(axon_error_to_gql)
    }

    async fn delete_tenant_database(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
        name: String,
    ) -> GqlResult<DeleteTenantDatabasePayload> {
        let state = state(ctx)?;
        require_tenant_admin(ctx, &state, &tenant_id)?;
        let storage = storage(&state)?;
        let deleted = with_storage(&storage, |s| {
            s.delete_tenant_database(TenantId::new(&tenant_id), &name)
        })
        .map_err(axon_error_to_gql)?;

        if !deleted {
            return Err(gql_error(
                "not_found",
                format!("database '{name}' not found in tenant '{tenant_id}'"),
            ));
        }

        Ok(DeleteTenantDatabasePayload {
            tenant_id,
            name,
            deleted: true,
        })
    }

    async fn issue_credential(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
        target_user: String,
        grants: GqlJson<Grants>,
        ttl_seconds: u64,
    ) -> GqlResult<IssueCredentialPayload> {
        let state = state(ctx)?;
        let grants = grants.0;
        let target_user_id = UserId::new(&target_user);
        let tenant_id_value = TenantId::new(&tenant_id);

        let caller_user_id = match resolved_identity(ctx)? {
            Some(identity) => {
                let is_deployment_admin =
                    control_plane_authz::require_deployment_admin(&identity, &state.user_roles)
                        .is_ok();
                if !is_deployment_admin && identity.user_id != target_user_id {
                    return Err(gql_error(
                        "forbidden",
                        "deployment admin or self-issue required",
                    ));
                }
                Some(identity.user_id.clone())
            }
            None => {
                require_legacy_admin(ctx)?;
                None
            }
        };

        let storage = storage(&state)?;
        let issuer = state
            .jwt_issuer
            .as_ref()
            .ok_or_else(|| gql_error("not_configured", "JWT issuer not configured"))?;

        let target_member = with_storage(&storage, |s| {
            s.get_tenant_member(tenant_id_value.clone(), target_user_id.clone())
        })
        .map_err(axon_error_to_gql)?;
        let target_member = target_member.ok_or_else(|| {
            gql_error(
                "not_a_tenant_member",
                format!("user '{target_user}' is not a member of tenant '{tenant_id}'"),
            )
        })?;

        if target_member.role.enforce_ceiling(&grants).is_err() {
            return Err(core_auth_error_to_gql(CoreAuthError::GrantsExceedRole));
        }

        if let Some(caller_id) = caller_user_id.as_ref() {
            if caller_id == &target_user_id {
                let caller_member = with_storage(&storage, |s| {
                    s.get_tenant_member(tenant_id_value.clone(), caller_id.clone())
                })
                .map_err(axon_error_to_gql)?;
                let caller_member = caller_member.ok_or_else(|| {
                    gql_error("forbidden", "caller is not a member of this tenant")
                })?;
                if caller_member.role.enforce_ceiling(&grants).is_err() {
                    return Err(core_auth_error_to_gql(CoreAuthError::GrantsExceedRole));
                }
            }
        }

        let now = now_secs();
        let jti = Uuid::now_v7();
        let expires_at = now + ttl_seconds;
        let claims = JwtClaims {
            iss: issuer.issuer_id.clone(),
            sub: target_user.clone(),
            aud: tenant_id.clone(),
            jti: jti.to_string(),
            iat: now,
            nbf: now,
            exp: expires_at,
            grants: grants.clone(),
        };
        let jwt = issue_jwt(issuer, &claims)?;
        let grants_json = serde_json::to_string(&grants)
            .map_err(|err| gql_error("serialization_error", err.to_string()))?;

        with_storage(&storage, |s| {
            s.track_credential_issuance(
                jti,
                target_user_id,
                tenant_id_value,
                now as i64 * 1000,
                expires_at as i64 * 1000,
                &grants_json,
            )
        })
        .map_err(axon_error_to_gql)?;

        Ok(IssueCredentialPayload {
            jwt,
            jti: claims.jti,
            expires_at,
        })
    }

    async fn revoke_credential(
        &self,
        ctx: &Context<'_>,
        tenant_id: String,
        jti: String,
    ) -> GqlResult<RevokeCredentialPayload> {
        let state = state(ctx)?;
        let tenant_id_value = TenantId::new(&tenant_id);
        let jti_uuid = jti
            .parse::<Uuid>()
            .map_err(|_| gql_error("invalid_jti", "jti must be a valid UUID"))?;
        let storage = storage(&state)?;

        let credentials = with_storage(&storage, |s| {
            s.list_credentials(tenant_id_value.clone(), None)
        })
        .map_err(axon_error_to_gql)?;
        let credential = credentials
            .into_iter()
            .find(|credential| credential.jti == jti);
        let credential = credential
            .ok_or_else(|| gql_error("not_found", format!("credential '{jti}' not found")))?;

        let revoked_by = match resolved_identity(ctx)? {
            Some(identity) => {
                let is_admin =
                    control_plane_authz::require_deployment_admin(&identity, &state.user_roles)
                        .is_ok()
                        || control_plane_authz::require_tenant_admin(
                            &identity,
                            tenant_id_value.clone(),
                            &state.user_roles,
                        )
                        .is_ok();
                let is_owner = identity.user_id == credential.user_id;
                if !is_admin && !is_owner {
                    return Err(gql_error(
                        "forbidden",
                        "tenant admin or credential owner required",
                    ));
                }
                identity.user_id.clone()
            }
            None => {
                require_legacy_admin(ctx)?;
                UserId::new("legacy-admin")
            }
        };

        with_storage(&storage, |s| s.revoke_credential(jti_uuid, revoked_by))
            .map_err(axon_error_to_gql)?;

        Ok(RevokeCredentialPayload {
            tenant_id,
            jti,
            revoked: true,
        })
    }
}

fn state(ctx: &Context<'_>) -> GqlResult<ControlPlaneState> {
    ctx.data::<ControlPlaneState>()
        .cloned()
        .map_err(|_| gql_error("internal_error", "control-plane state missing"))
}

fn storage(state: &ControlPlaneState) -> GqlResult<SharedStorage> {
    state.storage.clone().ok_or_else(|| {
        gql_error(
            "not_configured",
            "storage adapter not configured for this endpoint",
        )
    })
}

fn with_storage<T>(
    storage: &SharedStorage,
    f: impl FnOnce(&dyn StorageAdapter) -> Result<T, AxonError>,
) -> Result<T, AxonError> {
    let guard = storage
        .lock()
        .map_err(|_| AxonError::Storage("storage mutex poisoned".to_string()))?;
    f(guard.as_ref())
}

fn resolved_identity(ctx: &Context<'_>) -> GqlResult<Option<ResolvedIdentity>> {
    ctx.data::<MaybeResolvedIdentity>()
        .map(|resolved| resolved.0.clone())
        .map_err(|_| gql_error("internal_error", "resolved identity context missing"))
}

fn require_legacy_admin(ctx: &Context<'_>) -> GqlResult<()> {
    let legacy = ctx
        .data::<Identity>()
        .map_err(|_| gql_error("internal_error", "legacy identity missing"))?;
    legacy.require_admin().map_err(server_auth_error_to_gql)
}

fn require_deployment_admin(ctx: &Context<'_>, state: &ControlPlaneState) -> GqlResult<()> {
    match resolved_identity(ctx)? {
        Some(identity) => {
            control_plane_authz::require_deployment_admin(&identity, &state.user_roles)
                .map_err(|err| gql_error("forbidden", err.to_string()))
        }
        None => require_legacy_admin(ctx),
    }
}

fn require_tenant_admin(
    ctx: &Context<'_>,
    state: &ControlPlaneState,
    tenant_id: &str,
) -> GqlResult<()> {
    match resolved_identity(ctx)? {
        Some(identity) => {
            let tenant_id = TenantId::new(tenant_id);
            let ok =
                control_plane_authz::require_tenant_admin(&identity, tenant_id, &state.user_roles)
                    .is_ok()
                    || control_plane_authz::require_deployment_admin(&identity, &state.user_roles)
                        .is_ok();
            if ok {
                Ok(())
            } else {
                Err(gql_error(
                    "forbidden",
                    "tenant admin or deployment admin required",
                ))
            }
        }
        None => require_legacy_admin(ctx),
    }
}

fn credential_list_filter(
    ctx: &Context<'_>,
    state: &ControlPlaneState,
    tenant_id: TenantId,
) -> GqlResult<Option<UserId>> {
    match resolved_identity(ctx)? {
        Some(identity) => {
            let is_admin =
                control_plane_authz::require_deployment_admin(&identity, &state.user_roles).is_ok()
                    || control_plane_authz::require_tenant_admin(
                        &identity,
                        tenant_id,
                        &state.user_roles,
                    )
                    .is_ok();
            if is_admin {
                Ok(None)
            } else {
                Ok(Some(identity.user_id.clone()))
            }
        }
        None => {
            require_legacy_admin(ctx)?;
            Ok(None)
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn issue_jwt(issuer: &JwtIssuer, claims: &JwtClaims) -> GqlResult<String> {
    issuer
        .issue(claims)
        .map_err(|err| gql_error("signing_error", err.to_string()))
}

fn axon_error_to_gql(err: AxonError) -> GqlError {
    match err {
        AxonError::NotFound(msg) => gql_error("not_found", format!("not found: {msg}")),
        AxonError::AlreadyExists(msg) => {
            gql_error("already_exists", format!("already exists: {msg}"))
        }
        AxonError::Forbidden(msg) => gql_error("forbidden", msg),
        AxonError::InvalidOperation(msg) => gql_error("invalid_operation", msg),
        AxonError::Storage(msg) => gql_error("storage_error", msg),
        other => gql_error("internal_error", other.to_string()),
    }
}

fn core_auth_error_to_gql(err: CoreAuthError) -> GqlError {
    gql_error(err.error_code(), err.to_string())
}

fn server_auth_error_to_gql(err: ServerAuthError) -> GqlError {
    match err {
        ServerAuthError::Unauthorized(msg) => gql_error("unauthorized", msg),
        ServerAuthError::Forbidden(msg) => gql_error("forbidden", msg),
        ServerAuthError::MissingPeerAddress => gql_error("unauthorized", err.to_string()),
        ServerAuthError::ProviderUnavailable(msg) => gql_error("provider_unavailable", msg),
    }
}

fn gql_error(code: &'static str, message: impl Into<String>) -> GqlError {
    GqlError::new(message.into()).extend_with(move |_err, ext| {
        ext.set("code", code);
    })
}
