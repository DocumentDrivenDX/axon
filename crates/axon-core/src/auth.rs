//! Role-based access control (RBAC) for Axon (US-044, FEAT-012).
//!
//! Four built-in roles control access to Axon operations. Roles are derived
//! from the identity provider (Tailscale ACL tags, OIDC claims, etc.) and
//! checked at the API handler level before executing each operation.

use serde::{Deserialize, Serialize};

use crate::error::AxonError;

/// Built-in roles for Axon RBAC (FEAT-012).
///
/// Roles are ordered by privilege: `Admin > Write > Read > None`.
/// When a user has multiple roles (e.g., from multiple ACL tags),
/// the highest-privilege role wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Role {
    /// No access. Explicitly denied.
    None = 0,
    /// Read entities, query, traverse, browse audit log.
    Read = 1,
    /// Create, update, delete entities and links in any collection.
    Write = 2,
    /// All operations including drop and schema changes.
    Admin = 3,
}

impl Role {
    /// Parse a role from a Tailscale ACL tag or string name.
    ///
    /// Accepted inputs:
    /// - `"tag:axon-admin"` or `"admin"` -> `Admin`
    /// - `"tag:axon-write"` or `"write"` -> `Write`
    /// - `"tag:axon-read"` or `"read"` -> `Read`
    /// - anything else -> `None`
    pub fn from_tag(tag: &str) -> Self {
        match tag {
            "tag:axon-admin" | "admin" => Role::Admin,
            "tag:axon-write" | "write" => Role::Write,
            "tag:axon-read" | "read" => Role::Read,
            _ => Role::None,
        }
    }

    /// Determine the highest-privilege role from a set of tags.
    pub fn from_tags(tags: &[String]) -> Self {
        tags.iter().map(|t| Role::from_tag(t)).max().unwrap_or(Role::None)
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::None => write!(f, "none"),
            Role::Read => write!(f, "read"),
            Role::Write => write!(f, "write"),
            Role::Admin => write!(f, "admin"),
        }
    }
}

/// The kind of operation being attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Read an entity, query, traverse, audit.
    Read,
    /// Create, update, delete entities/links.
    Write,
    /// Drop collections, modify schemas, manage namespaces.
    Admin,
}

impl Operation {
    /// The minimum role required for this operation.
    pub fn required_role(self) -> Role {
        match self {
            Operation::Read => Role::Read,
            Operation::Write => Role::Write,
            Operation::Admin => Role::Admin,
        }
    }
}

/// Identity context for a request.
///
/// Populated by the auth middleware from the identity provider.
/// In `--no-auth` mode, all requests get `CallerIdentity::anonymous()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerIdentity {
    /// Actor name for audit log entries.
    pub actor: String,
    /// The caller's effective role (highest privilege from all their tags).
    pub role: Role,
}

impl CallerIdentity {
    /// Create an anonymous identity with admin privileges (`--no-auth` mode).
    pub fn anonymous() -> Self {
        Self {
            actor: "anonymous".into(),
            role: Role::Admin,
        }
    }

    /// Create an identity from a name and role.
    pub fn new(actor: impl Into<String>, role: Role) -> Self {
        Self {
            actor: actor.into(),
            role,
        }
    }

    /// Check whether this caller is authorized for the given operation.
    ///
    /// Returns `Ok(())` if authorized, or `Err(AxonError::InvalidOperation)`
    /// with a descriptive message if not.
    pub fn check(&self, op: Operation) -> Result<(), AxonError> {
        let required = op.required_role();
        if self.role >= required {
            Ok(())
        } else {
            Err(AxonError::InvalidOperation(format!(
                "permission denied: role '{}' cannot perform {:?} operations (requires '{}')",
                self.role, op, required
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ordering() {
        assert!(Role::Admin > Role::Write);
        assert!(Role::Write > Role::Read);
        assert!(Role::Read > Role::None);
    }

    #[test]
    fn role_from_tag() {
        assert_eq!(Role::from_tag("tag:axon-admin"), Role::Admin);
        assert_eq!(Role::from_tag("tag:axon-write"), Role::Write);
        assert_eq!(Role::from_tag("tag:axon-read"), Role::Read);
        assert_eq!(Role::from_tag("unknown"), Role::None);
    }

    #[test]
    fn role_from_tags_picks_highest() {
        let tags = vec!["tag:axon-read".into(), "tag:axon-write".into()];
        assert_eq!(Role::from_tags(&tags), Role::Write);
    }

    #[test]
    fn role_from_empty_tags() {
        assert_eq!(Role::from_tags(&[]), Role::None);
    }

    #[test]
    fn anonymous_has_admin() {
        let id = CallerIdentity::anonymous();
        assert_eq!(id.role, Role::Admin);
        assert_eq!(id.actor, "anonymous");
    }

    #[test]
    fn admin_can_do_everything() {
        let admin = CallerIdentity::new("alice", Role::Admin);
        assert!(admin.check(Operation::Read).is_ok());
        assert!(admin.check(Operation::Write).is_ok());
        assert!(admin.check(Operation::Admin).is_ok());
    }

    #[test]
    fn write_can_read_and_write_but_not_admin() {
        let writer = CallerIdentity::new("bob", Role::Write);
        assert!(writer.check(Operation::Read).is_ok());
        assert!(writer.check(Operation::Write).is_ok());
        assert!(writer.check(Operation::Admin).is_err());
    }

    #[test]
    fn read_can_only_read() {
        let reader = CallerIdentity::new("charlie", Role::Read);
        assert!(reader.check(Operation::Read).is_ok());
        assert!(reader.check(Operation::Write).is_err());
        assert!(reader.check(Operation::Admin).is_err());
    }

    #[test]
    fn none_cannot_do_anything() {
        let nobody = CallerIdentity::new("nobody", Role::None);
        assert!(nobody.check(Operation::Read).is_err());
        assert!(nobody.check(Operation::Write).is_err());
        assert!(nobody.check(Operation::Admin).is_err());
    }

    #[test]
    fn write_denied_error_is_descriptive() {
        let reader = CallerIdentity::new("charlie", Role::Read);
        let err = reader.check(Operation::Write).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("permission denied"), "msg: {msg}");
        assert!(msg.contains("read"), "msg: {msg}");
        assert!(msg.contains("write"), "msg: {msg}");
    }
}
