//! Authentication and identity resolution for Axon server.
//!
//! In `--no-auth` mode, all requests succeed as admin with actor="anonymous"`.
//! When auth is enabled, identity is resolved from the incoming request via
//! Tailscale LocalAPI `whois`.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io::Read as _;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Buf;
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::net::UnixStream;
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
    /// Guest mode — unauthenticated requests are allowed with a fixed role.
    ///
    /// The actor is recorded as `"guest"` in audit entries. This is opt-in and
    /// intended for scenarios where Tailscale is unavailable but limited access
    /// is acceptable.
    Guest {
        /// The role assigned to all guest requests.
        role: Role,
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

    /// A guest identity with the given role, used in `--auth guest` mode.
    pub fn guest(role: Role) -> Self {
        Self {
            actor: "guest".into(),
            role,
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
                "permission denied: role 'read' cannot perform write operations (requires 'write')"
                    .into(),
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

/// Abstraction over the Tailscale LocalAPI, enabling test doubles.
///
/// The production implementation ([`LocalApiWhoisProvider`]) contacts the
/// real Tailscale daemon over its Unix socket.  Tests use `FakeWhoisProvider`
/// to inject pre-canned responses without needing a running Tailscale daemon.
pub(crate) trait TailscaleWhoisProvider: Send + Sync {
    /// Verify that the provider is reachable.
    ///
    /// Called once at server startup via [`AuthContext::verify`] to surface
    /// misconfigurations early (e.g., wrong socket path, daemon not running).
    fn verify(&self) -> BoxFuture<'_, Result<(), AuthError>>;

    /// Resolve the identity of the given peer address.
    ///
    /// Returns [`AuthError::Unauthorized`] for non-tailnet addresses and
    /// [`AuthError::ProviderUnavailable`] if the daemon cannot be reached.
    fn whois(
        &self,
        address: SocketAddr,
    ) -> BoxFuture<'_, Result<TailscaleWhoisResponse, AuthError>>;
}

// ── Tailscale LocalAPI JSON response types ───────────────────────────
//
// Minimal serde structs matching the PascalCase JSON returned by the
// Tailscale daemon's `/localapi/v0/whois` endpoint.  Only the fields
// Axon inspects are declared; serde silently ignores the rest.

/// Subset of the Tailscale `Hostinfo` object -- we only need `Hostname`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TsHostinfo {
    pub hostname: Option<String>,
}

/// Subset of the Tailscale `Node` object returned by `/localapi/v0/whois`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TsNode {
    pub name: String,
    pub hostinfo: TsHostinfo,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub computed_name: String,
    #[serde(default)]
    pub computed_name_with_host: String,
}

/// Subset of the Tailscale `UserProfile` returned by `/localapi/v0/whois`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TsUserProfile {
    pub login_name: String,
}

/// Top-level JSON returned by `GET /localapi/v0/whois?addr=...`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TsWhoisResponse {
    pub node: TsNode,
    pub user_profile: TsUserProfile,
}

impl From<TsWhoisResponse> for TailscaleWhoisResponse {
    fn from(raw: TsWhoisResponse) -> Self {
        Self {
            node_name: preferred_node_name(&raw.node),
            user_login: raw.user_profile.login_name,
            tags: raw.node.tags,
        }
    }
}

// ── Direct Unix-socket HTTP client for the Tailscale LocalAPI ────────
//
// Tailscale does not bind a TCP port for its LocalAPI — it only listens on
// a Unix domain socket.  This provider opens a new connection for every
// request (no connection pooling) because whois calls are rare and caching
// makes per-request cost negligible.  Each call issues one HTTP/1.1 GET
// and reads the full response body before closing the socket.

#[derive(Clone)]
struct LocalApiWhoisProvider {
    socket_path: PathBuf,
}

impl LocalApiWhoisProvider {
    fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    /// Issue an HTTP/1.1 GET to the Tailscale daemon over the Unix socket and
    /// return the response body bytes.
    async fn get(&self, path: &str) -> Result<hyper::body::Bytes, AuthError> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| AuthError::ProviderUnavailable(format!("connection failed: {e}")))?;

        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| AuthError::ProviderUnavailable(format!("handshake failed: {e}")))?;

        // Drive the HTTP connection in the background.
        tokio::spawn(async move {
            let _ = conn.await;
        });

        let uri: http::Uri = path
            .parse()
            .map_err(|e| AuthError::ProviderUnavailable(format!("invalid URI: {e}")))?;

        let request = http::Request::builder()
            .method("GET")
            .header("Host", "local-tailscaled.sock")
            .uri(uri)
            .body(http_body_util::Empty::<hyper::body::Bytes>::new())
            .map_err(|e| AuthError::ProviderUnavailable(format!("request build failed: {e}")))?;

        let response = sender
            .send_request(request)
            .await
            .map_err(|e| AuthError::ProviderUnavailable(format!("request failed: {e}")))?;

        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|e| AuthError::ProviderUnavailable(format!("body read failed: {e}")))?
            .aggregate();

        if status == http::StatusCode::OK {
            let mut buf = vec![0u8; body.remaining()];
            body.reader().read_exact(&mut buf).map_err(|e| {
                AuthError::ProviderUnavailable(format!("body aggregation failed: {e}"))
            })?;
            Ok(hyper::body::Bytes::from(buf))
        } else if status == http::StatusCode::NOT_FOUND
            || status == http::StatusCode::UNPROCESSABLE_ENTITY
        {
            // 404: address not known to this tailnet (observed on Tailscale ≥ 1.x).
            // 422: legacy "unprocessable entity" returned by older daemon versions.
            // Both mean the peer is not a tailnet member — not a provider fault.
            Err(AuthError::Unauthorized(
                "peer is not a recognized tailnet address".into(),
            ))
        } else {
            Err(AuthError::ProviderUnavailable(format!(
                "unexpected status from tailscaled: {status}"
            )))
        }
    }
}

impl TailscaleWhoisProvider for LocalApiWhoisProvider {
    fn verify(&self) -> BoxFuture<'_, Result<(), AuthError>> {
        Box::pin(async move {
            // A successful GET /localapi/v0/status proves the daemon is reachable.
            let _ = self.get("/localapi/v0/status").await?;
            Ok(())
        })
    }

    fn whois(
        &self,
        address: SocketAddr,
    ) -> BoxFuture<'_, Result<TailscaleWhoisResponse, AuthError>> {
        Box::pin(async move {
            let path = format!("/localapi/v0/whois?addr={address}");
            let body = self.get(&path).await?;
            let raw: TsWhoisResponse = serde_json::from_slice(&body).map_err(|e| {
                AuthError::ProviderUnavailable(format!("failed to parse whois response: {e}"))
            })?;
            Ok(TailscaleWhoisResponse::from(raw))
        })
    }
}

/// Request authentication state shared by the HTTP and gRPC frontends.
///
/// `AuthContext` is cheaply `Clone`d (it wraps `Arc` internally) and intended
/// to be inserted as Axum router state and tonic service state.
///
/// Resolved identities are cached by peer IP for [`cache_ttl`] to avoid a
/// Unix socket round-trip on every request.  A cache hit requires only an
/// `RwLock` read — no I/O.  The cache is never actively evicted; stale entries
/// are ignored on the next lookup if their TTL has expired.
///
/// # Configuration examples
///
/// ```rust,ignore
/// // Local development — no auth required
/// let auth = AuthContext::no_auth();
///
/// // Tailscale (production default)
/// let auth = AuthContext::tailscale(
///     Role::Read,                                // default role for untagged nodes
///     "/run/tailscale/tailscaled.sock",
///     Duration::from_secs(60),                   // identity cache TTL
/// );
///
/// // Guest mode — unauthenticated callers get read access
/// let auth = AuthContext::guest(Role::Read);
/// ```
#[derive(Clone)]
pub struct AuthContext {
    mode: AuthMode,
    provider: Option<Arc<dyn TailscaleWhoisProvider>>,
    cache: Arc<RwLock<HashMap<IpAddr, CachedIdentity>>>,
    cache_ttl: Duration,
    /// Per-principal role registry.  Overrides tag-based role resolution.
    user_roles: crate::user_roles::UserRoleStore,
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
            user_roles: crate::user_roles::UserRoleStore::default(),
        }
    }

    #[must_use]
    pub fn guest(role: Role) -> Self {
        Self {
            mode: AuthMode::Guest { role },
            provider: None,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(0),
            user_roles: crate::user_roles::UserRoleStore::default(),
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

    /// Replace the user-role store with `store`.
    ///
    /// Both the returned `AuthContext` and the original `store` handle share
    /// the same underlying `Arc`, so mutations through either are immediately
    /// visible.
    #[must_use]
    pub fn with_user_roles(mut self, store: crate::user_roles::UserRoleStore) -> Self {
        self.user_roles = store;
        self
    }

    /// Return a reference to the shared user-role store.
    pub fn user_roles(&self) -> &crate::user_roles::UserRoleStore {
        &self.user_roles
    }

    #[must_use]
    pub fn mode(&self) -> &AuthMode {
        &self.mode
    }

    /// Verify that the auth provider is reachable.
    ///
    /// Called once at server startup before accepting connections.  In
    /// `NoAuth` and `Guest` modes this is a no-op.  In `Tailscale` mode it
    /// issues a test request to the Tailscale daemon so a misconfigured
    /// socket path or a stopped daemon surfaces immediately at startup rather
    /// than on the first real request.
    pub async fn verify(&self) -> Result<(), AuthError> {
        match &self.provider {
            Some(provider) => provider.verify().await,
            None => Ok(()),
        }
    }

    pub async fn resolve_peer(&self, peer: Option<SocketAddr>) -> Result<Identity, AuthError> {
        match &self.mode {
            AuthMode::NoAuth => Ok(Identity::anonymous_admin()),
            AuthMode::Guest { role } => Ok(Identity::guest(role.clone())),
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
                let whois = provider.whois(peer).await?;

                // Priority: (1) Axon user-role registry by login, (2) ACL tags,
                // (3) --tailscale-default-role.
                let role = if whois.user_login.is_empty() {
                    role_from_tags(&whois.tags, default_role)
                } else {
                    self.user_roles
                        .get(&whois.user_login)
                        .unwrap_or_else(|| role_from_tags(&whois.tags, default_role))
                };
                let actor = if whois.user_login.is_empty() {
                    whois.node_name.clone()
                } else {
                    whois.user_login.clone()
                };
                let identity = Identity { actor, role };
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
            user_roles: crate::user_roles::UserRoleStore::default(),
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
///
/// The actor is the user's login name (e.g. `"erik@example.com"`), which
/// identifies the *person* in audit log entries.  For tagged service nodes
/// whose login name is empty, the node name is used as a fallback so that
/// automated agents still produce a meaningful actor string.
pub fn identity_from_tailscale(whois: &TailscaleWhoisResponse, default_role: &Role) -> Identity {
    let actor = if whois.user_login.is_empty() {
        whois.node_name.clone()
    } else {
        whois.user_login.clone()
    };
    Identity {
        actor,
        role: role_from_tags(&whois.tags, default_role),
    }
}

/// Pick the most human-readable name for a Tailscale node.
///
/// Preference order: `ComputedName` → `Hostinfo.Hostname` →
/// `ComputedNameWithHost` → first label of the FQDN `Name`.
fn preferred_node_name(node: &TsNode) -> String {
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

    #[tokio::test(flavor = "multi_thread")]
    async fn no_auth_returns_anonymous_admin() {
        let context = AuthContext::no_auth();
        let identity = context.resolve_peer(None).await.expect("no auth succeeds");
        assert_eq!(identity.actor, "anonymous");
        assert_eq!(identity.role, Role::Admin);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn guest_mode_returns_guest_identity() {
        let context = AuthContext::guest(Role::Read);
        let identity = context
            .resolve_peer(None)
            .await
            .expect("guest auth succeeds");
        assert_eq!(identity.actor, "guest");
        assert_eq!(identity.role, Role::Read);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn guest_mode_with_write_role() {
        let context = AuthContext::guest(Role::Write);
        let identity = context
            .resolve_peer(None)
            .await
            .expect("guest auth succeeds");
        assert_eq!(identity.actor, "guest");
        assert_eq!(identity.role, Role::Write);
    }

    #[test]
    fn guest_identity_constructor() {
        let id = Identity::guest(Role::Read);
        assert_eq!(id.actor, "guest");
        assert_eq!(id.role, Role::Read);
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
        assert_eq!(id.actor, "erik@example.com");
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
        assert_eq!(id.actor, "user@example.com");
        assert_eq!(id.role, Role::Read);
    }

    #[test]
    fn identity_from_tailscale_falls_back_to_node_name_when_login_empty() {
        // Tagged service nodes may have an empty user_login; fall back to node_name.
        let whois = TailscaleWhoisResponse {
            node_name: "agent-worker-1".into(),
            user_login: String::new(),
            tags: vec!["tag:axon-agent".into()],
        };
        let id = identity_from_tailscale(&whois, &Role::Read);
        assert_eq!(id.actor, "agent-worker-1");
        assert_eq!(id.role, Role::Write);
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

    #[tokio::test(flavor = "multi_thread")]
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

        assert_eq!(first.actor, "erik@example.com");
        assert_eq!(first.role, Role::Admin);
        assert_eq!(second, first);
        assert_eq!(provider.calls(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
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

    #[tokio::test(flavor = "multi_thread")]
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

    // ── User-role registry tests (US-048) ─────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn user_role_registry_overrides_tag_based_role() {
        use crate::user_roles::{UserRoleEntry, UserRoleStore};

        let address = SocketAddr::from(([100, 101, 102, 103], 443));
        // Tailscale says the node has no axon tags → default role would be Read.
        let provider = Arc::new(FakeWhoisProvider::with_result(
            address,
            Ok(TailscaleWhoisResponse {
                node_name: "erikd-laptop".into(),
                user_login: "erik@example.com".into(),
                tags: vec![],
            }),
        ));
        let store = UserRoleStore::default();
        store.load_from_entries(vec![UserRoleEntry {
            login: "erik@example.com".into(),
            role: Role::Write,
        }]);
        let context = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            provider,
            Duration::from_secs(60),
        )
        .with_user_roles(store);

        let identity = context
            .resolve_peer(Some(address))
            .await
            .expect("should resolve");
        assert_eq!(identity.actor, "erik@example.com");
        // Registry grants Write, overriding the Read default.
        assert_eq!(identity.role, Role::Write);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn user_role_registry_overrides_acl_tag_role() {
        use crate::user_roles::{UserRoleEntry, UserRoleStore};

        let address = SocketAddr::from(([100, 101, 102, 104], 443));
        // Tailscale tags say Admin, but registry says Read.
        let provider = Arc::new(FakeWhoisProvider::with_result(
            address,
            Ok(TailscaleWhoisResponse {
                node_name: "erikd-laptop".into(),
                user_login: "restricted@example.com".into(),
                tags: vec!["tag:axon-admin".into()],
            }),
        ));
        let store = UserRoleStore::default();
        store.load_from_entries(vec![UserRoleEntry {
            login: "restricted@example.com".into(),
            role: Role::Read,
        }]);
        let context = AuthContext::with_provider(
            AuthMode::Tailscale {
                default_role: Role::Read,
            },
            provider,
            Duration::from_secs(60),
        )
        .with_user_roles(store);

        let identity = context
            .resolve_peer(Some(address))
            .await
            .expect("should resolve");
        assert_eq!(identity.role, Role::Read);
    }
}
