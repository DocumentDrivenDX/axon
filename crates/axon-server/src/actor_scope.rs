//! Per-actor collection scope constraints.
//!
//! When configured, restricts which collections an actor may write to.
//! Actors with the `Admin` role always bypass scope constraints.
//! Actors without a configured scope are unrestricted (opt-in model).

use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::{AuthError, Role};

/// Maps actor names to the set of collections they are allowed to write to.
///
/// - If an actor has no entry, they are unrestricted (opt-in restriction).
/// - A collection list containing `"*"` means all collections are allowed.
/// - `Admin`-role actors always bypass scope constraints regardless of config.
#[derive(Debug, Clone, Default)]
pub struct ActorScopeConfig {
    /// Map from exact actor name to allowed collection names.
    scopes: HashMap<String, Vec<String>>,
}

impl ActorScopeConfig {
    /// Create an empty config (no restrictions for any actor).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a config from an existing map of actor names to collection lists.
    #[must_use]
    pub fn from_map(scopes: HashMap<String, Vec<String>>) -> Self {
        Self { scopes }
    }

    /// Add a scope restriction for a specific actor.
    pub fn add_actor(&mut self, actor: impl Into<String>, collections: Vec<String>) {
        self.scopes.insert(actor.into(), collections);
    }
}

/// Shared state wrapping the actor scope config, suitable for use as an axum Extension.
#[derive(Clone)]
pub struct ActorScopeGuard {
    config: Arc<ActorScopeConfig>,
}

impl Default for ActorScopeGuard {
    fn default() -> Self {
        Self::new(ActorScopeConfig::default())
    }
}

impl ActorScopeGuard {
    /// Create a new guard with the given configuration.
    #[must_use]
    pub fn new(config: ActorScopeConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Check whether the given actor is allowed to write to the specified collection.
    ///
    /// - `Admin` role always passes.
    /// - If the actor has no configured scope, they are allowed (opt-in restriction).
    /// - If the actor's scope contains `"*"`, all collections are allowed.
    /// - Otherwise the collection must appear in the actor's allowed list.
    pub fn check(&self, actor: &str, collection: &str, role: &Role) -> Result<(), AuthError> {
        // Admin always bypasses scope constraints.
        if *role == Role::Admin {
            return Ok(());
        }

        // If no scope is configured for this actor, allow (opt-in).
        let allowed = match self.config.scopes.get(actor) {
            Some(collections) => collections,
            None => return Ok(()),
        };

        // Wildcard means all collections are allowed.
        if allowed.iter().any(|c| c == "*") {
            return Ok(());
        }

        // Check for exact match on the collection name.
        if allowed.iter().any(|c| c == collection) {
            return Ok(());
        }

        Err(AuthError::Forbidden(format!(
            "actor '{actor}' is not allowed to write to collection '{collection}'"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_bypasses_scope_constraints() {
        let mut config = ActorScopeConfig::new();
        config.add_actor("alice", vec!["orders".into()]);
        let guard = ActorScopeGuard::new(config);

        // Admin can write to any collection, even one not in their scope.
        assert!(guard.check("alice", "users", &Role::Admin).is_ok());
    }

    #[test]
    fn unconfigured_actor_is_unrestricted() {
        let config = ActorScopeConfig::new();
        let guard = ActorScopeGuard::new(config);

        // No scope configured for "bob" — allowed to write anywhere.
        assert!(guard.check("bob", "anything", &Role::Write).is_ok());
    }

    #[test]
    fn actor_allowed_for_listed_collection() {
        let mut config = ActorScopeConfig::new();
        config.add_actor("agent-1", vec!["tasks".into(), "notes".into()]);
        let guard = ActorScopeGuard::new(config);

        assert!(guard.check("agent-1", "tasks", &Role::Write).is_ok());
        assert!(guard.check("agent-1", "notes", &Role::Write).is_ok());
    }

    #[test]
    fn actor_denied_for_unlisted_collection() {
        let mut config = ActorScopeConfig::new();
        config.add_actor("agent-1", vec!["tasks".into()]);
        let guard = ActorScopeGuard::new(config);

        let err = guard
            .check("agent-1", "secrets", &Role::Write)
            .expect_err("should be denied");
        match err {
            AuthError::Forbidden(msg) => {
                assert!(msg.contains("agent-1"), "msg: {msg}");
                assert!(msg.contains("secrets"), "msg: {msg}");
            }
            other => panic!("expected Forbidden, got: {other:?}"),
        }
    }

    #[test]
    fn wildcard_allows_all_collections() {
        let mut config = ActorScopeConfig::new();
        config.add_actor("agent-2", vec!["*".into()]);
        let guard = ActorScopeGuard::new(config);

        assert!(guard.check("agent-2", "anything", &Role::Write).is_ok());
        assert!(guard.check("agent-2", "other", &Role::Write).is_ok());
    }

    #[test]
    fn empty_scope_denies_all() {
        let mut config = ActorScopeConfig::new();
        config.add_actor("locked-out", vec![]);
        let guard = ActorScopeGuard::new(config);

        assert!(guard
            .check("locked-out", "anything", &Role::Write)
            .is_err());
    }

    #[test]
    fn read_role_with_scope_is_still_checked() {
        let mut config = ActorScopeConfig::new();
        config.add_actor("reader", vec!["public".into()]);
        let guard = ActorScopeGuard::new(config);

        // Even though reads don't typically go through scope checks in the gateway,
        // the guard itself enforces the constraint for any role except Admin.
        assert!(guard.check("reader", "public", &Role::Read).is_ok());
        assert!(guard.check("reader", "private", &Role::Read).is_err());
    }

    #[test]
    fn from_map_constructor() {
        let mut map = HashMap::new();
        map.insert("agent".into(), vec!["col-a".into(), "col-b".into()]);
        let config = ActorScopeConfig::from_map(map);
        let guard = ActorScopeGuard::new(config);

        assert!(guard.check("agent", "col-a", &Role::Write).is_ok());
        assert!(guard.check("agent", "col-c", &Role::Write).is_err());
    }

    #[test]
    fn default_guard_allows_everything() {
        let guard = ActorScopeGuard::default();
        assert!(guard.check("anyone", "anything", &Role::Write).is_ok());
    }
}
