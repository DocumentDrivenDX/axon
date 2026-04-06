//! Authentication and identity resolution for Axon server.
//!
//! In `--no-auth` mode, all requests succeed as admin with actor="anonymous".
//! When auth is enabled (future: Tailscale whois), identity is resolved from
//! the incoming request.

use serde::{Deserialize, Serialize};

/// Authentication mode for the server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AuthMode {
    /// No authentication — all requests succeed as admin.
    /// Actor is recorded as `"anonymous"` in audit entries.
    #[default]
    NoAuth,
    // Future: Tailscale { ... }
}

/// The role of an authenticated identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Write,
    Read,
}

/// Resolved identity for a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Resolve identity based on auth mode.
///
/// In `NoAuth` mode, always returns the anonymous admin identity.
pub fn resolve_identity(mode: &AuthMode) -> Identity {
    match mode {
        AuthMode::NoAuth => Identity::anonymous_admin(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_auth_returns_anonymous_admin() {
        let identity = resolve_identity(&AuthMode::NoAuth);
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

    #[test]
    fn no_auth_all_requests_succeed_as_admin() {
        // Verify that in NoAuth mode, resolve_identity always returns admin.
        // This satisfies the acceptance criterion: "All requests succeed as admin."
        for _ in 0..10 {
            let id = resolve_identity(&AuthMode::NoAuth);
            assert_eq!(id.role, Role::Admin);
        }
    }

    #[test]
    fn audit_actor_is_anonymous_in_no_auth() {
        // Verify the audit actor name is "anonymous" in no-auth mode.
        let id = resolve_identity(&AuthMode::NoAuth);
        assert_eq!(id.actor, "anonymous");
    }
}
