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
        tags.iter()
            .map(|t| Role::from_tag(t))
            .max()
            .unwrap_or(Role::None)
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

    /// Apply field-level masking to entity data based on mask policies.
    ///
    /// Admin users see all fields. For other roles, fields listed in a
    /// mask policy that requires a higher role than the caller's are
    /// removed from the data.
    pub fn apply_masks(&self, data: &mut serde_json::Value, policies: &[MaskPolicy]) {
        if self.role >= Role::Admin {
            return; // Admin sees all.
        }
        if let Some(obj) = data.as_object_mut() {
            for policy in policies {
                if self.role < policy.min_role {
                    obj.remove(&policy.field);
                }
            }
        }
    }
}

/// A field-level mask policy (US-046, FEAT-012).
///
/// Specifies that a field should be hidden from callers whose role is
/// below `min_role`. Admin always sees all fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskPolicy {
    /// The field name to mask.
    pub field: String,
    /// Minimum role required to see this field.
    pub min_role: Role,
}

// ── Attribute-based write control (US-047, FEAT-012) ────────────────────────

/// Per-collection write policy for attribute-based access control (ABAC).
///
/// Stored in the `__axon_policies__` pseudo-collection. Defines:
/// - Minimum role required to write to this collection
/// - Fields that are immutable after creation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WritePolicy {
    /// Collection this policy applies to.
    pub collection: String,
    /// Minimum role required for write operations.
    pub min_write_role: Role,
    /// Fields that cannot be modified after the initial create.
    #[serde(default)]
    pub immutable_fields: Vec<String>,
}

impl WritePolicy {
    /// Check if a caller has sufficient role to write to this collection.
    pub fn check_write(&self, caller: &CallerIdentity) -> Result<(), AxonError> {
        if caller.role >= self.min_write_role {
            Ok(())
        } else {
            Err(AxonError::InvalidOperation(format!(
                "write policy for '{}': role '{}' insufficient (requires '{}')",
                self.collection, caller.role, self.min_write_role
            )))
        }
    }

    /// Check if any immutable fields have been modified.
    ///
    /// Returns the list of field names that were illegally modified.
    pub fn check_immutable_fields(
        &self,
        old_data: &serde_json::Value,
        new_data: &serde_json::Value,
    ) -> Vec<String> {
        let mut violations = Vec::new();
        for field in &self.immutable_fields {
            let old_val = old_data.get(field);
            let new_val = new_data.get(field);
            // If the field existed before and is now different or missing, it's a violation.
            if old_val.is_some() && old_val != new_val {
                violations.push(field.clone());
            }
        }
        violations
    }
}

// ── Database-scoped access control (US-038, FEAT-014) ────────────────────────

/// A grant that scopes a role to a specific database.
///
/// Without a grant, users cannot access a database (unless they have a
/// wildcard `*` grant or admin role).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseGrant {
    /// The actor (user/node) this grant applies to. `"*"` means all users.
    pub actor: String,
    /// The database name. `"*"` means all databases.
    pub database: String,
    /// The role granted for this database.
    pub role: Role,
}

/// Registry of database-scoped grants.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GrantRegistry {
    grants: Vec<DatabaseGrant>,
}

impl GrantRegistry {
    /// Create a new empty grant registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a grant.
    pub fn add(&mut self, grant: DatabaseGrant) {
        self.grants.push(grant);
    }

    /// Remove all grants for an actor on a database.
    pub fn revoke(&mut self, actor: &str, database: &str) {
        self.grants
            .retain(|g| g.actor != actor || g.database != database);
    }

    /// Resolve the effective role for an actor on a database.
    ///
    /// Checks grants in order: exact match, then wildcard actor, then
    /// wildcard database. Returns the highest-privilege matching role.
    pub fn effective_role(&self, actor: &str, database: &str) -> Role {
        self.grants
            .iter()
            .filter(|g| {
                (g.actor == actor || g.actor == "*")
                    && (g.database == database || g.database == "*")
            })
            .map(|g| g.role)
            .max()
            .unwrap_or(Role::None)
    }

    /// Check if an actor can perform an operation on a database.
    pub fn check(
        &self,
        actor: &str,
        database: &str,
        operation: Operation,
    ) -> Result<(), AxonError> {
        let role = self.effective_role(actor, database);
        if role >= operation.required_role() {
            Ok(())
        } else {
            Err(AxonError::InvalidOperation(format!(
                "no grant for actor '{actor}' on database '{database}' with sufficient role (has '{role}', needs '{}')",
                operation.required_role()
            )))
        }
    }

    /// List all grants.
    pub fn list(&self) -> &[DatabaseGrant] {
        &self.grants
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
        let err = reader
            .check(Operation::Write)
            .expect_err("read role should not be allowed to write");
        let msg = err.to_string();
        assert!(msg.contains("permission denied"), "msg: {msg}");
        assert!(msg.contains("read"), "msg: {msg}");
        assert!(msg.contains("write"), "msg: {msg}");
    }

    // ── Field-level masking tests (US-046) ──────────────────────────────

    #[test]
    fn admin_sees_all_fields() {
        let admin = CallerIdentity::new("admin", Role::Admin);
        let mut data = serde_json::json!({"name": "Alice", "salary": 100000});
        let policies = vec![MaskPolicy {
            field: "salary".into(),
            min_role: Role::Admin,
        }];
        admin.apply_masks(&mut data, &policies);
        assert!(data.get("salary").is_some(), "admin should see salary");
    }

    #[test]
    fn reader_cannot_see_admin_only_fields() {
        let reader = CallerIdentity::new("bob", Role::Read);
        let mut data =
            serde_json::json!({"name": "Alice", "salary": 100000, "email": "alice@co.com"});
        let policies = vec![MaskPolicy {
            field: "salary".into(),
            min_role: Role::Admin,
        }];
        reader.apply_masks(&mut data, &policies);
        assert!(data.get("salary").is_none(), "reader should not see salary");
        assert!(data.get("name").is_some(), "name is not masked");
        assert!(data.get("email").is_some(), "email is not masked");
    }

    #[test]
    fn writer_sees_write_level_fields() {
        let writer = CallerIdentity::new("charlie", Role::Write);
        let mut data = serde_json::json!({"name": "Alice", "internal_notes": "sensitive"});
        let policies = vec![MaskPolicy {
            field: "internal_notes".into(),
            min_role: Role::Write,
        }];
        writer.apply_masks(&mut data, &policies);
        assert!(
            data.get("internal_notes").is_some(),
            "writer should see write-level fields"
        );
    }

    #[test]
    fn reader_cannot_see_write_level_fields() {
        let reader = CallerIdentity::new("dave", Role::Read);
        let mut data = serde_json::json!({"name": "Alice", "internal_notes": "sensitive"});
        let policies = vec![MaskPolicy {
            field: "internal_notes".into(),
            min_role: Role::Write,
        }];
        reader.apply_masks(&mut data, &policies);
        assert!(
            data.get("internal_notes").is_none(),
            "reader should not see write-level fields"
        );
    }

    #[test]
    fn multiple_mask_policies() {
        let reader = CallerIdentity::new("eve", Role::Read);
        let mut data = serde_json::json!({"name": "Alice", "salary": 100000, "ssn": "123-45-6789"});
        let policies = vec![
            MaskPolicy {
                field: "salary".into(),
                min_role: Role::Admin,
            },
            MaskPolicy {
                field: "ssn".into(),
                min_role: Role::Write,
            },
        ];
        reader.apply_masks(&mut data, &policies);
        assert!(data.get("salary").is_none());
        assert!(data.get("ssn").is_none());
        assert!(data.get("name").is_some());
    }

    #[test]
    fn mask_on_non_object_is_noop() {
        let reader = CallerIdentity::new("f", Role::Read);
        let mut data = serde_json::json!("just a string");
        let policies = vec![MaskPolicy {
            field: "anything".into(),
            min_role: Role::Admin,
        }];
        reader.apply_masks(&mut data, &policies);
        assert_eq!(data, serde_json::json!("just a string"));
    }

    // ── ABAC write policy tests (US-047) ───────────────────────────────

    #[test]
    fn write_policy_allows_matching_role() {
        let policy = WritePolicy {
            collection: "tasks".into(),
            min_write_role: Role::Write,
            immutable_fields: vec![],
        };
        let caller = CallerIdentity::new("alice", Role::Write);
        assert!(policy.check_write(&caller).is_ok());
    }

    #[test]
    fn write_policy_denies_insufficient_role() {
        let policy = WritePolicy {
            collection: "tasks".into(),
            min_write_role: Role::Admin,
            immutable_fields: vec![],
        };
        let caller = CallerIdentity::new("bob", Role::Write);
        assert!(policy.check_write(&caller).is_err());
    }

    #[test]
    fn immutable_fields_detected() {
        let policy = WritePolicy {
            collection: "tasks".into(),
            min_write_role: Role::Write,
            immutable_fields: vec!["created_by".into(), "source".into()],
        };

        let old_data = serde_json::json!({"title": "T", "created_by": "alice", "source": "api"});
        let new_data = serde_json::json!({"title": "T2", "created_by": "alice", "source": "api"});
        let violations = policy.check_immutable_fields(&old_data, &new_data);
        assert!(violations.is_empty());

        let changed = serde_json::json!({"title": "T2", "created_by": "bob", "source": "api"});
        let violations = policy.check_immutable_fields(&old_data, &changed);
        assert_eq!(violations, vec!["created_by"]);
    }

    #[test]
    fn immutable_field_added_is_ok() {
        // Adding an immutable field to an entity that didn't have it is OK.
        let policy = WritePolicy {
            collection: "tasks".into(),
            min_write_role: Role::Write,
            immutable_fields: vec!["locked".into()],
        };
        let old_data = serde_json::json!({"title": "T"});
        let new_data = serde_json::json!({"title": "T", "locked": true});
        let violations = policy.check_immutable_fields(&old_data, &new_data);
        assert!(violations.is_empty());
    }

    #[test]
    fn immutable_field_removed_is_violation() {
        let policy = WritePolicy {
            collection: "tasks".into(),
            min_write_role: Role::Write,
            immutable_fields: vec!["source".into()],
        };
        let old_data = serde_json::json!({"title": "T", "source": "api"});
        let new_data = serde_json::json!({"title": "T2"});
        let violations = policy.check_immutable_fields(&old_data, &new_data);
        assert_eq!(violations, vec!["source"]);
    }

    // ── Database grant tests (US-038) ──────────────────────────────────

    #[test]
    fn grant_registry_empty_denies_all() {
        let registry = GrantRegistry::new();
        assert_eq!(registry.effective_role("alice", "prod"), Role::None);
        assert!(registry.check("alice", "prod", Operation::Read).is_err());
    }

    #[test]
    fn exact_grant_allows_access() {
        let mut registry = GrantRegistry::new();
        registry.add(DatabaseGrant {
            actor: "alice".into(),
            database: "prod".into(),
            role: Role::Write,
        });
        assert_eq!(registry.effective_role("alice", "prod"), Role::Write);
        assert!(registry.check("alice", "prod", Operation::Write).is_ok());
        assert!(registry.check("alice", "prod", Operation::Admin).is_err());
    }

    #[test]
    fn wildcard_actor_grant() {
        let mut registry = GrantRegistry::new();
        registry.add(DatabaseGrant {
            actor: "*".into(),
            database: "public".into(),
            role: Role::Read,
        });
        assert_eq!(registry.effective_role("anyone", "public"), Role::Read);
        assert_eq!(registry.effective_role("alice", "public"), Role::Read);
    }

    #[test]
    fn wildcard_database_grant() {
        let mut registry = GrantRegistry::new();
        registry.add(DatabaseGrant {
            actor: "admin-bot".into(),
            database: "*".into(),
            role: Role::Admin,
        });
        assert_eq!(
            registry.effective_role("admin-bot", "any-database"),
            Role::Admin
        );
    }

    #[test]
    fn highest_privilege_wins() {
        let mut registry = GrantRegistry::new();
        registry.add(DatabaseGrant {
            actor: "alice".into(),
            database: "prod".into(),
            role: Role::Read,
        });
        registry.add(DatabaseGrant {
            actor: "alice".into(),
            database: "prod".into(),
            role: Role::Admin,
        });
        assert_eq!(registry.effective_role("alice", "prod"), Role::Admin);
    }

    #[test]
    fn no_cross_database_access_without_grant() {
        let mut registry = GrantRegistry::new();
        registry.add(DatabaseGrant {
            actor: "alice".into(),
            database: "dev".into(),
            role: Role::Admin,
        });
        // alice has no grant on prod
        assert_eq!(registry.effective_role("alice", "prod"), Role::None);
    }

    #[test]
    fn revoke_removes_grants() {
        let mut registry = GrantRegistry::new();
        registry.add(DatabaseGrant {
            actor: "alice".into(),
            database: "prod".into(),
            role: Role::Write,
        });
        registry.revoke("alice", "prod");
        assert_eq!(registry.effective_role("alice", "prod"), Role::None);
    }
}
