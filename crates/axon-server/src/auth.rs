//! Authentication and identity resolution for Axon server.
//!
//! In `--no-auth` mode, all requests succeed as admin with actor="anonymous"`.
//! When auth is enabled, identity is resolved from the incoming request via
//! Tailscale LocalAPI `whois`.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tailscale_localapi::{LocalApi, UnixStreamClient};
use tokio::sync::RwLock;

/// Authentication mode for the server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AuthMode {
    /// No authentication — all requests succeed as admin.
    /// Actor is recorded as `"anonymous"` in audit entries.
    #[default]
    NoAuth,
    /// Tailscale whois-based authentication.
    ///
    /// Resolves identity from the Tailscale local API (`/localapi/v0/whois`).
    /// Non-tailnet connections are rejected. The node name becomes the audit actor.
    /// Untagged nodes receive the default role.
    Tailscale {
        /// Default role for nodes without an ACL tag.
        default_role: Role,
    },
}

/// The role of an authenticated identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Write,
    Read,
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Resolved identity for a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// The actor name for audit log entries.
    pub actor: String,
    /// The role that determines access level.
    pub role: Role,
}

impl Identity {
    /// The anonymous admin identity used in `--no-auth` mode.
    pub fn anonymous_admin() -> Self {
        Self {
            actor: "anonymous".into(),
            role: Role::Admin,
        }
    }

    /// Check that this identity has at least read-level access.
    pub fn require_read(&self) -> Result<(), AuthError> {
        // All authenticated roles (Read, Write, Admin) can read.
        Ok(())
    }

    /// Check that this identity has at least write-level access.
    pub fn require_write(&self) -> Result<(), AuthError> {
        match self.role {
            Role::Write | Role::Admin => Ok(()),
            Role::Read => Err(AuthError::Forbidden(
                "permission denied: role 'read' cannot perform write operations (requires 'write')".into(),
            )),
        }
    }

    /// Check that this identity has admin-level access.
    pub fn require_admin(&self) -> Result<(), AuthError> {
        match self.role {
            Role::Admin => Ok(()),
            Role::Write => Err(AuthError::Forbidden(
                "permission denied: role 'write' cannot perform admin operations (requires 'admin')".into(),
            )),
            Role::Read => Err(AuthError::Forbidden(
                "permission denied: role 'read' cannot perform admin operations (requires 'admin')".into(),
            )),
        }
    }
}

/// Tailscale whois response (subset of fields we use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleWhoisResponse {
    /// The node's display name (e.g., "erik-laptop").
    pub node_name: String,
    /// The user's login name.
    pub user_login: String,
    /// ACL tags on this node (e.g., `tag:server`, `tag:admin`).
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingPeerAddress,
    Unauthorized(String),
    /// The caller is authenticated but lacks the required role.
    Forbidden(String),
    ProviderUnavailable(String),
}

impl AuthError {
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::MissingPeerAddress => "missing peer address",
            Self::Unauthorized(detail)
            | Self::Forbidden(detail)
            | Self::ProviderUnavailable(detail) => detail,
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.detail())
    }
}

impl std::error::Error for AuthError {}

#[derive(Debug, Clone)]
struct CachedIdentity {
    identity: Identity,
    expires_at: Instant,
}

pub(crate) trait TailscaleWhoisProvider: Send + Sync {
    fn verify(&self) -> BoxFuture<'_, Result<(), AuthError>>;

    fn whois(
        &self,
        address: SocketAddr,
    ) -> BoxFuture<'_, Result<TailscaleWhoisResponse, AuthError>>;
}

#[derive(Clone)]
struct LocalApiWhoisProvider {
    client: LocalApi<UnixStreamClient>,
}

impl LocalApiWhoisProvider {
    fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            client: LocalApi::new_with_socket_path(socket_path),
        }
    }
}

impl TailscaleWhoisProvider for LocalApiWhoisProvider {
    fn verify(&self) -> BoxFuture<'_, Result<(), AuthError>> {
        Box::pin(async move {
            self.client
                .status()
                .await
                .map(|_| ())
                .map_err(map_localapi_error)
        })
    }

    fn whois(
        &self,
        address: SocketAddr,
    ) -> BoxFuture<'_, Result<TailscaleWhoisResponse, AuthError>> {
        Box::pin(async move {
            self.client
                .whois(address)
                .await
                .map(TailscaleWhoisResponse::from)
                .map_err(map_localapi_error)
        })
    }
}

/// Request authentication state shared by the HTTP and gRPC frontends.
#[derive(Clone)]
pub struct AuthContext {
    mode: AuthMode,
    provider: Option<Arc<dyn TailscaleWhoisProvider>>,
    cache: Arc<RwLock<HashMap<IpAddr, CachedIdentity>>>,
    cache_ttl: Duration,
}

impl Default for AuthContext {
    fn default() -> Self {
        Self::no_auth()
    }
}

impl AuthContext {
    #[must_use]
    pub fn no_auth() -> Self {
        Self {
            mode: AuthMode::NoAuth,
            provider: None,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(60),
        }
    }

    #[must_use]
    pub fn tailscale(
        default_role: Role,
        socket_path: impl Into<PathBuf>,
        cache_ttl: Duration,
    ) -> Self {
        let mode = AuthMode::Tailscale { default_role };
        let provider = Arc::new(LocalApiWhoisProvider::new(socket_path.into()));
        Self::with_provider(mode, provider, cache_ttl)
    }

    #[must_use]
    pub fn mode(&self) -> &AuthMode {
        &self.mode
    }

    pub async fn verify(&self) -> Result<(), AuthError> {
        match &self.provider {
            Some(provider) => provider.verify().await,
            None => Ok(()),
        }
    }

    pub async fn resolve_peer(&self, peer: Option<SocketAddr>) -> Result<Identity, AuthError> {
        match &self.mode {
            AuthMode::NoAuth => Ok(Identity::anonymous_admin()),
            AuthMode::Tailscale { default_role } => {
                let peer = peer.ok_or(AuthError::MissingPeerAddress)?;
                let peer_ip = peer.ip();

                if let Some(identity) = self.cached_identity(peer_ip).await {
                    return Ok(identity);
                }

                let provider = self.provider.as_ref().ok_or_else(|| {
                    AuthError::ProviderUnavailable(
                        "tailscale auth is enabled but no LocalAPI provider is configured".into(),
                    )
                })?;
                let identity = identity_from_tailscale(&provider.whois(peer).await?, default_role);
                self.store_cached_identity(peer_ip, identity.clone()).await;
                Ok(identity)
            }
        }
    }

    async fn cached_identity(&self, peer_ip: IpAddr) -> Option<Identity> {
        let cache = self.cache.read().await;
        cache.get(&peer_ip).and_then(|entry| {
            if entry.expires_at > Instant::now() {
                Some(entry.identity.clone())
            } else {
                None
            }
        })
    }

    async fn store_cached_identity(&self, peer_ip: IpAddr, identity: Identity) {
        self.cache.write().await.insert(
            peer_ip,
            CachedIdentity {
                identity,
                expires_at: Instant::now() + self.cache_ttl,
            },
        );
    }

    pub(crate) fn with_provider(
        mode: AuthMode,
        provider: Arc<dyn TailscaleWhoisProvider>,
        cache_ttl: Duration,
    ) -> Self {
        Self {
            mode,
            provider: Some(provider),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl,
        }
    }
}

/// Map Tailscale ACL tags to an Axon role.
///
/// - `tag:axon-admin` -> Admin
/// - `tag:axon-write` / `tag:axon-agent` -> Write
/// - `tag:axon-read` -> Read
/// - Multiple matching tags -> highest-privilege role wins
/// - No matching tags -> default_role
pub fn role_from_tags(tags: &[String], default_role: &Role) -> Role {
    let mut highest_role: Option<Role> = None;

    for tag in tags {
        if let Some(role) = role_for_tag(tag) {
            let should_replace = match &highest_role {
                Some(current) => role_priority(&role) > role_priority(current),
                None => true,
            };

            if should_replace {
                highest_role = Some(role);
            }
        }
    }

    match highest_role {
        Some(role) => role,
        None => default_role.clone(),
    }
}

fn role_for_tag(tag: &str) -> Option<Role> {
    match tag {
        "tag:admin" | "tag:axon-admin" => Some(Role::Admin),
        "tag:write" | "tag:axon-write" | "tag:axon-agent" => Some(Role::Write),
        "tag:read" | "tag:axon-read" => Some(Role::Read),
        _ => None,
    }
}

const fn role_priority(role: &Role) -> u8 {
    match role {
        Role::Admin => 3,
        Role::Write => 2,
        Role::Read => 1,
    }
}

/// Resolve identity from a Tailscale whois response.
pub fn identity_from_tailscale(whois: &TailscaleWhoisResponse, default_role: &Role) -> Identity {
    Identity {
        actor: whois.node_name.clone(),
        role: role_from_tags(&whois.tags, default_role),
    }
}

impl From<tailscale_localapi::Whois> for TailscaleWhoisResponse {
    fn from(whois: tailscale_localapi::Whois) -> Self {
        Self {
            node_name: preferred_node_name(&whois.node),
            user_login: whois.user_profile.login_name,
            tags: whois.node.tags,
        }
    }
}

fn preferred_node_name(node: &tailscale_localapi::Node) -> String {
    if !node.computed_name.is_empty() {
        return node.computed_name.clone();
    }

    if let Some(hostname) = &node.hostinfo.hostname {
        if !hostname.is_empty() {
            return hostname.clone();
        }
    }

    if !node.computed_name_with_host.is_empty() {
        return node.computed_name_with_host.clone();
    }

    short_node_name(&node.name)
}

fn short_node_name(name: &str) -> String {
    match name.split('.').next() {
        Some(short) if !short.is_empty() => short.to_string(),
        _ => name.to_string(),
    }
}

fn map_localapi_error(error: tailscale_localapi::Error) -> AuthError {
    match error {
        tailscale_localapi::Error::UnprocessableEntity => {
            AuthError::Unauthorized("peer is not a recognized tailnet address".into())
        }
        other => AuthError::ProviderUnavailable(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    use super::*;

    struct FakeWhoisProvider {
        calls: AtomicUsize,
        verification: StdMutex<Result<(), AuthError>>,
        results: StdMutex<HashMap<SocketAddr, Result<TailscaleWhoisResponse, AuthError>>>,
    }

    impl FakeWhoisProvider {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                verification: StdMutex::new(Ok(())),
                results: StdMutex::new(HashMap::new()),
            }
        }

        fn with_result(
            address: SocketAddr,
            result: Result<TailscaleWhoisResponse, AuthError>,
        ) -> Self {
            let mut results = HashMap::new();
            results.insert(address, result);
            Self {
                calls: AtomicUsize::new(0),
                verification: StdMutex::new(Ok(())),
                results: StdMutex::new(results),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl TailscaleWhoisProvider for FakeWhoisProvider {
        fn verify(&self) -> BoxFuture<'_, Result<(), AuthError>> {
            Box::pin(async move {
                let verification = match self.verification.lock() {
                    Ok(verification) => verification,
                    Err(poisoned) => poisoned.into_inner(),
                };
                verification.clone()
            })
        }

        fn whois(
            &self,
            address: SocketAddr,
        ) -> BoxFuture<'_, Result<TailscaleWhoisResponse, AuthError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let results = match self.results.lock() {
                    Ok(results) => results,
                    Err(poisoned) => poisoned.into_inner(),
                };
                results.get(&address).cloned().unwrap_or_else(|| {
                    Err(AuthError::Unauthorized(
                        "peer is not a recognized tailnet address".into(),
                    ))
                })
            })
        }
    }

    #[tokio::test]
    async fn no_auth_returns_anonymous_admin() {
        let context = AuthContext::no_auth();
        let identity = context.resolve_peer(None).await.expect("no auth succeeds");
        assert_eq!(identity.actor, "anonymous");
        assert_eq!(identity.role, Role::Admin);
    }

    #[test]
    fn default_auth_mode_is_no_auth() {
        assert_eq!(AuthMode::default(), AuthMode::NoAuth);
    }

    #[test]
    fn anonymous_admin_identity() {
        let id = Identity::anonymous_admin();
        assert_eq!(id.actor, "anonymous");
        assert_eq!(id.role, Role::Admin);
    }

    // ── Tailscale auth tests (US-043) ──────────────────────────────────

    #[test]
    fn role_from_admin_tag() {
        let role = role_from_tags(&["tag:admin".into()], &Role::Read);
        assert_eq!(role, Role::Admin);
    }

    #[test]
    fn role_from_write_tag() {
        let role = role_from_tags(&["tag:write".into()], &Role::Read);
        assert_eq!(role, Role::Write);
    }

    #[test]
    fn role_from_read_tag() {
        let role = role_from_tags(&["tag:read".into()], &Role::Admin);
        assert_eq!(role, Role::Read);
    }

    #[test]
    fn role_from_axon_agent_tag_maps_to_write() {
        let role = role_from_tags(&["tag:axon-agent".into()], &Role::Read);
        assert_eq!(role, Role::Write);
    }

    #[test]
    fn role_from_no_tags_uses_default() {
        let role = role_from_tags(&[], &Role::Write);
        assert_eq!(role, Role::Write);
    }

    #[test]
    fn role_from_unknown_tags_uses_default() {
        let role = role_from_tags(&["tag:custom".into()], &Role::Read);
        assert_eq!(role, Role::Read);
    }

    #[test]
    fn role_from_mixed_tags_prefers_admin_regardless_of_order() {
        let read_then_admin = role_from_tags(
            &["tag:axon-read".into(), "tag:axon-admin".into()],
            &Role::Read,
        );
        let admin_then_read = role_from_tags(
            &["tag:axon-admin".into(), "tag:axon-read".into()],
            &Role::Read,
        );

        assert_eq!(read_then_admin, Role::Admin);
        assert_eq!(admin_then_read, Role::Admin);
    }

    #[test]
    fn role_from_mixed_tags_prefers_write_regardless_of_order() {
        let read_then_write = role_from_tags(
            &["tag:axon-read".into(), "tag:axon-write".into()],
            &Role::Admin,
        );
        let write_then_read = role_from_tags(
            &["tag:axon-write".into(), "tag:axon-read".into()],
            &Role::Admin,
        );

        assert_eq!(read_then_write, Role::Write);
        assert_eq!(write_then_read, Role::Write);
    }

    #[test]
    fn identity_from_tailscale_whois() {
        let whois = TailscaleWhoisResponse {
            node_name: "erik-laptop".into(),
            user_login: "erik@example.com".into(),
            tags: vec!["tag:admin".into()],
        };
        let id = identity_from_tailscale(&whois, &Role::Read);
        assert_eq!(id.actor, "erik-laptop");
        assert_eq!(id.role, Role::Admin);
    }

    #[test]
    fn identity_from_tailscale_untagged_node() {
        let whois = TailscaleWhoisResponse {
            node_name: "phone".into(),
            user_login: "user@example.com".into(),
            tags: vec![],
        };
        let id = identity_from_tailscale(&whois, &Role::Read);
        assert_eq!(id.actor, "phone");
        assert_eq!(id.role, Role::Read);
    }

    #[test]
    fn tailscale_whois_serialization() {
        let whois = TailscaleWhoisResponse {
            node_name: "server".into(),
            user_login: "svc@example.com".into(),
            tags: vec!["tag:write".into()],
        };
        let json = serde_json::to_string(&whois).expect("whois should serialize");
        let parsed: TailscaleWhoisResponse =
            serde_json::from_str(&json).expect("whois should deserialize");
        assert_eq!(parsed.node_name, "server");
        assert_eq!(parsed.tags, vec!["tag:write"]);
    }

    #[tokio::test]
    async fn tailscale_mode_resolves_and_caches_identity() {
        let address = SocketAddr::from(([100, 101, 102, 103], 443));
        let provider = Arc::new(FakeWhoisProvider::with_result(
            address,
            Ok(TailscaleWhoisResponse {
                node_name: "erik-laptop".into(),
                user_login: "erik@example.com".into(),
                tags: vec!["tag:axon-admin".into()],
            }),
        ));
        let context = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            provider.clone(),
            Duration::from_secs(60),
        );

        let first = context
            .resolve_peer(Some(address))
            .await
            .expect("whois should resolve");
        let second = context
            .resolve_peer(Some(address))
            .await
            .expect("cached whois should resolve");

        assert_eq!(first.actor, "erik-laptop");
        assert_eq!(first.role, Role::Admin);
        assert_eq!(second, first);
        assert_eq!(provider.calls(), 1);
    }

    #[tokio::test]
    async fn reject_non_tailnet_connection() {
        let address = SocketAddr::from(([127, 0, 0, 1], 3000));
        let provider = Arc::new(FakeWhoisProvider::with_result(
            address,
            Err(AuthError::Unauthorized(
                "peer is not a recognized tailnet address".into(),
            )),
        ));
        let context = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            provider,
            Duration::from_secs(60),
        );

        let error = context
            .resolve_peer(Some(address))
            .await
            .expect_err("non-tailnet peers must be rejected");
        assert_eq!(
            error,
            AuthError::Unauthorized("peer is not a recognized tailnet address".into())
        );
    }

    #[tokio::test]
    async fn verify_surfaces_provider_unavailability() {
        let provider = FakeWhoisProvider::new();
        {
            let mut verification = match provider.verification.lock() {
                Ok(verification) => verification,
                Err(poisoned) => poisoned.into_inner(),
            };
            *verification = Err(AuthError::ProviderUnavailable(
                "tailscaled unavailable".into(),
            ));
        }
        let context = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            Arc::new(provider),
            Duration::from_secs(60),
        );

        let error = context
            .verify()
            .await
            .expect_err("verification should fail");
        assert_eq!(
            error,
            AuthError::ProviderUnavailable("tailscaled unavailable".into())
        );
    }

    // ── RBAC role enforcement tests (US-044) ──────────────────────────

    #[test]
    fn admin_passes_all_checks() {
        let id = Identity {
            actor: "alice".into(),
            role: Role::Admin,
        };
        assert!(id.require_read().is_ok());
        assert!(id.require_write().is_ok());
        assert!(id.require_admin().is_ok());
    }

    #[test]
    fn write_can_read_and_write_but_not_admin() {
        let id = Identity {
            actor: "bob".into(),
            role: Role::Write,
        };
        assert!(id.require_read().is_ok());
        assert!(id.require_write().is_ok());
        assert!(id.require_admin().is_err());
    }

    #[test]
    fn read_can_only_read() {
        let id = Identity {
            actor: "charlie".into(),
            role: Role::Read,
        };
        assert!(id.require_read().is_ok());
        assert!(id.require_write().is_err());
        assert!(id.require_admin().is_err());
    }

    #[test]
    fn forbidden_error_is_descriptive() {
        let id = Identity {
            actor: "reader".into(),
            role: Role::Read,
        };
        let err = id.require_write().expect_err("read should not write");
        match err {
            AuthError::Forbidden(msg) => {
                assert!(msg.contains("permission denied"), "msg: {msg}");
                assert!(msg.contains("read"), "msg: {msg}");
                assert!(msg.contains("write"), "msg: {msg}");
            }
            other => panic!("expected Forbidden, got: {other:?}"),
        }
    }

    #[test]
    fn anonymous_admin_passes_all_checks() {
        let id = Identity::anonymous_admin();
        assert!(id.require_read().is_ok());
        assert!(id.require_write().is_ok());
        assert!(id.require_admin().is_ok());
    }
}
