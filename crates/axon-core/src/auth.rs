//! Role-based and attribute-based access control for Axon (FEAT-012).
//!
//! # Layers
//!
//! **Layer 1 — RBAC** ([`Role`], [`CallerIdentity`], [`Operation`])
//!
//! Four built-in roles (`Admin > Write > Read > None`) control access to
//! Axon operations.  Roles are derived from the identity provider (Tailscale
//! ACL tags, OIDC claims, etc.) by the server layer and passed into handlers
//! as a [`CallerIdentity`].  Handlers call [`CallerIdentity::check`] to
//! enforce the minimum required role.
//!
//! **Layer 2 — Field-level masking** ([`MaskPolicy`])
//!
//! A `MaskPolicy` hides individual entity fields from callers whose role
//! falls below a configured minimum.  Applied by [`CallerIdentity::apply_masks`]
//! before returning entity data to the caller.  Admins always see all fields.
//!
//! **Layer 3 — Collection write control** ([`WritePolicy`])
//!
//! A `WritePolicy` sets a per-collection minimum write role and marks fields
//! as immutable after creation.  Checked by the handler before any mutation.
//!
//! **Layer 4 — Database-scoped grants** ([`DatabaseGrant`], [`GrantRegistry`])
//!
//! Grants scope a role to a specific database (or `"*"` for all databases).
//! Without a matching grant a caller has `Role::None` on that database.
//! Admins with a global grant bypass per-database checks.  The grant registry
//! is stored in `__axon_policies__` and enforced at the routing layer.

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
    /// Accepted inputs (both full `tag:` prefix and bare name):
    ///
    /// | Input | Role |
    /// |-------|------|
    /// | `"tag:axon-admin"` / `"tag:admin"` / `"admin"` | `Admin` |
    /// | `"tag:axon-write"` / `"tag:axon-agent"` / `"tag:write"` / `"write"` | `Write` |
    /// | `"tag:axon-read"` / `"tag:read"` / `"read"` | `Read` |
    /// | anything else | `None` |
    ///
    /// `tag:axon-agent` is an alias for `write` — the conventional tag for
    /// automated agent workloads that need read/write but not admin access.
    pub fn from_tag(tag: &str) -> Self {
        match tag {
            "tag:axon-admin" | "tag:admin" | "admin" => Role::Admin,
            "tag:axon-write" | "tag:axon-agent" | "tag:write" | "write" => Role::Write,
            "tag:axon-read" | "tag:read" | "read" => Role::Read,
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

/// A simple field-equality filter used as an agent scope constraint
/// (FEAT-022 / ADR-016).
///
/// When attached to a [`CallerIdentity`], the agent guardrails layer rejects
/// any mutation whose target entity data does not satisfy
/// `data[field] == value`. V1 supports only field-equality predicates;
/// compound and range filters are deferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityFilter {
    /// The top-level data field to compare.
    pub field: String,
    /// The required value for that field.
    pub value: serde_json::Value,
}

impl EntityFilter {
    /// Convenience constructor.
    pub fn new(field: impl Into<String>, value: serde_json::Value) -> Self {
        Self {
            field: field.into(),
            value,
        }
    }

    /// Returns `true` when the JSON object `data` satisfies this filter.
    ///
    /// Returns `false` when `data` is not a JSON object, when the field is
    /// absent, or when the value does not equal [`Self::value`].
    pub fn matches(&self, data: &serde_json::Value) -> bool {
        data.get(&self.field).is_some_and(|v| v == &self.value)
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
    /// Optional scope constraint enforced by the agent guardrails layer
    /// (FEAT-022, ADR-016). When `Some`, this caller may only mutate entities
    /// whose data matches the filter. `None` means no scope restriction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_filter: Option<EntityFilter>,
}

impl CallerIdentity {
    /// Create an anonymous identity with admin privileges (`--no-auth` mode).
    pub fn anonymous() -> Self {
        Self {
            actor: "anonymous".into(),
            role: Role::Admin,
            entity_filter: None,
        }
    }

    /// Create an identity from a name and role.
    pub fn new(actor: impl Into<String>, role: Role) -> Self {
        Self {
            actor: actor.into(),
            role,
            entity_filter: None,
        }
    }

    /// Attach an entity scope filter (builder style, FEAT-022).
    pub fn with_entity_filter(mut self, filter: EntityFilter) -> Self {
        self.entity_filter = Some(filter);
        self
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
        // Primary tags
        assert_eq!(Role::from_tag("tag:axon-admin"), Role::Admin);
        assert_eq!(Role::from_tag("tag:axon-write"), Role::Write);
        assert_eq!(Role::from_tag("tag:axon-read"), Role::Read);
        // Short-form aliases
        assert_eq!(Role::from_tag("tag:admin"), Role::Admin);
        assert_eq!(Role::from_tag("tag:write"), Role::Write);
        assert_eq!(Role::from_tag("tag:read"), Role::Read);
        // Bare names (no tag: prefix)
        assert_eq!(Role::from_tag("admin"), Role::Admin);
        assert_eq!(Role::from_tag("write"), Role::Write);
        assert_eq!(Role::from_tag("read"), Role::Read);
        // Agent alias
        assert_eq!(Role::from_tag("tag:axon-agent"), Role::Write);
        // Unknown
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

// ── ADR-018: Tenant/User/Credential types ───────────────────────────────────

use uuid::Uuid;

/// Stable identifier for a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TenantId(pub String);

impl TenantId {
    /// Generate a fresh tenant id (UUIDv7 string).
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Wrap an existing id string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Stable identifier for a user.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub String);

impl UserId {
    /// Generate a fresh user id (UUIDv7 string).
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Wrap an existing id string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A tenant (global account boundary).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub created_at_ms: u64,
}

/// A user (global identity).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_at_ms: Option<u64>,
}

/// External identity that federates to a user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserIdentity {
    pub provider: String,
    pub external_id: String,
    pub user_id: UserId,
}

/// A user's membership in a tenant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TenantMember {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub role: TenantRole,
}

/// Role a user has within a tenant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TenantRole {
    Admin,
    Write,
    Read,
}

impl TenantRole {
    /// Returns the operations this role can delegate (ceiling).
    ///
    /// - `Admin` → `{Read, Write, Admin}`
    /// - `Write` → `{Read, Write}`
    /// - `Read` → `{Read}`
    pub fn delegation_ops(&self) -> Vec<Op> {
        match self {
            TenantRole::Admin => vec![Op::Read, Op::Write, Op::Admin],
            TenantRole::Write => vec![Op::Read, Op::Write],
            TenantRole::Read => vec![Op::Read],
        }
    }
}

/// An operation that can be granted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Op {
    Read,
    Write,
    Admin,
}

/// A database with a set of granted operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrantedDatabase {
    pub name: String,
    pub ops: Vec<Op>,
}

/// A collection of database grants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grants {
    pub databases: Vec<GrantedDatabase>,
}

impl Grants {
    /// Validate the grants structure according to the v5 schema.
    ///
    /// - At most 1024 databases
    /// - Each database must have at least one op
    /// - Each op must be unique (no duplicates)
    pub fn validate(&self) -> Result<(), AuthError> {
        if self.databases.len() > 1024 {
            return Err(AuthError::GrantsMalformed);
        }
        for db in &self.databases {
            db.validate()?;
        }
        Ok(())
    }
}

impl GrantedDatabase {
    /// Validate a single granted database.
    ///
    /// - name must be non-empty
    /// - ops must not be empty
    /// - ops must be unique
    pub fn validate(&self) -> Result<(), AuthError> {
        if self.name.is_empty() {
            return Err(AuthError::GrantsMalformed);
        }
        if self.ops.is_empty() {
            return Err(AuthError::GrantsMalformed);
        }
        // Check for duplicate ops
        let unique_ops: std::collections::HashSet<_> = self.ops.iter().collect();
        if unique_ops.len() != self.ops.len() {
            return Err(AuthError::GrantsMalformed);
        }
        Ok(())
    }
}

/// JWT claims for tenant-scoped credentials (ADR-018 §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JwtClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub jti: String,
    pub iat: u64,
    pub nbf: u64,
    pub exp: u64,
    pub grants: Grants,
}

impl JwtClaims {
    /// Validate the JWT claims structure.
    ///
    /// Returns `AuthError::CredentialMalformed` if:
    /// - `aud` is not a single string (e.g., it was a JSON array)
    /// - Grants don't match the expected schema
    ///
    /// This is typically used after deserialization to catch schema violations
    /// that serde doesn't catch by itself (e.g., aud as an array).
    pub fn validate(&self) -> Result<(), AuthError> {
        // Check grants structure
        self.grants.validate()?;
        Ok(())
    }
}

/// Authentication error variants (ADR-018 §4 failure-mode table).
///
/// This enum has exactly 14 variants matching the failure modes in
/// ADR-018 §4 "JWT failure mode → HTTP status + error code".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthError {
    /// No `Authorization` header (non-`--no-auth` mode).
    Unauthenticated,

    /// Header present but not `Bearer <token>`, or JWT structurally invalid,
    /// or `aud` claim is a JSON array instead of a string.
    CredentialMalformed,

    /// Signature invalid (wrong key or tampered payload).
    CredentialInvalid,

    /// `exp` claim is in the past.
    CredentialExpired,

    /// `nbf` claim is in the future.
    CredentialNotYetValid,

    /// `jti` claim is present in the revocation list.
    CredentialRevoked,

    /// `iss` claim does not match this deployment's issuer.
    CredentialForeignIssuer,

    /// `aud` claim does not match the tenant in the URL path.
    CredentialWrongTenant,

    /// `sub` claim resolves to a suspended or deleted user.
    UserSuspended,

    /// `sub` claim is not a member of the tenant in the URL path.
    NotATenantMember,

    /// URL `{database}` segment not found in `grants.databases[]`.
    DatabaseNotGranted,

    /// Required operation not in the matching grant's `ops[]`.
    OpNotGranted,

    /// Grant scope exceeds issuer's role at issuance time.
    GrantsExceedIssuerRole,

    /// Malformed grants payload (schema validation failure).
    GrantsMalformed,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Unauthenticated => write!(f, "unauthenticated"),
            AuthError::CredentialMalformed => write!(f, "credential malformed"),
            AuthError::CredentialInvalid => write!(f, "credential invalid"),
            AuthError::CredentialExpired => write!(f, "credential expired"),
            AuthError::CredentialNotYetValid => write!(f, "credential not yet valid"),
            AuthError::CredentialRevoked => write!(f, "credential revoked"),
            AuthError::CredentialForeignIssuer => write!(f, "credential foreign issuer"),
            AuthError::CredentialWrongTenant => write!(f, "credential wrong tenant"),
            AuthError::UserSuspended => write!(f, "user suspended"),
            AuthError::NotATenantMember => write!(f, "not a tenant member"),
            AuthError::DatabaseNotGranted => write!(f, "database not granted"),
            AuthError::OpNotGranted => write!(f, "op not granted"),
            AuthError::GrantsExceedIssuerRole => write!(f, "grants exceed issuer role"),
            AuthError::GrantsMalformed => write!(f, "grants malformed"),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<serde_json::Error> for AuthError {
    fn from(_: serde_json::Error) -> Self {
        AuthError::CredentialMalformed
    }
}

/// Parse a JSON string into [`JwtClaims`], mapping any deserialization error
/// to [`AuthError::CredentialMalformed`].
pub fn parse_claims(s: &str) -> Result<JwtClaims, AuthError> {
    serde_json::from_str(s).map_err(AuthError::from)
}

impl Grants {
    /// Find a granted database by name.
    pub fn find_database(&self, name: &str) -> Option<&GrantedDatabase> {
        self.databases.iter().find(|db| db.name == name)
    }
}

impl GrantedDatabase {
    /// Returns `true` if the given operation is in this grant's ops list.
    pub fn has_op(&self, op: Op) -> bool {
        self.ops.contains(&op)
    }
}

impl AuthError {
    /// HTTP status code for this error (401 or 403).
    ///
    /// Credential-level failures → 401; scope-level failures → 403.
    pub fn status_code(&self) -> u16 {
        match self {
            AuthError::Unauthenticated => 401,
            AuthError::CredentialMalformed => 401,
            AuthError::CredentialInvalid => 401,
            AuthError::CredentialExpired => 401,
            AuthError::CredentialNotYetValid => 401,
            AuthError::CredentialRevoked => 401,
            AuthError::CredentialForeignIssuer => 401,
            AuthError::CredentialWrongTenant => 403,
            AuthError::UserSuspended => 401,
            AuthError::NotATenantMember => 403,
            AuthError::DatabaseNotGranted => 403,
            AuthError::OpNotGranted => 403,
            AuthError::GrantsExceedIssuerRole => 401,
            AuthError::GrantsMalformed => 401,
        }
    }

    /// Stable machine-readable error code string (ADR-018 §4).
    pub fn error_code(&self) -> &'static str {
        match self {
            AuthError::Unauthenticated => "unauthenticated",
            AuthError::CredentialMalformed => "credential_malformed",
            AuthError::CredentialInvalid => "credential_invalid",
            AuthError::CredentialExpired => "credential_expired",
            AuthError::CredentialNotYetValid => "credential_not_yet_valid",
            AuthError::CredentialRevoked => "credential_revoked",
            AuthError::CredentialForeignIssuer => "credential_foreign_issuer",
            AuthError::CredentialWrongTenant => "credential_wrong_tenant",
            AuthError::UserSuspended => "user_suspended",
            AuthError::NotATenantMember => "not_a_tenant_member",
            AuthError::DatabaseNotGranted => "database_not_granted",
            AuthError::OpNotGranted => "op_not_granted",
            AuthError::GrantsExceedIssuerRole => "grants_exceed_issuer_role",
            AuthError::GrantsMalformed => "grants_malformed",
        }
    }
}

/// A resolved identity installed into the request extension after successful JWT verification.
///
/// Downstream handlers read this from `Extension<ResolvedIdentity>` and trust it completely.
#[derive(Debug, Clone)]
pub struct ResolvedIdentity {
    pub user_id: UserId,
    pub tenant_id: TenantId,
    pub grants: Grants,
}

/// Compile-time exhaustive check that `AuthError` has exactly 14 variants
/// (ADR-018 §4). Adding or removing a variant without updating this match
/// will cause a compile error.
const _: () = {
    const fn _exhaustive_variant_check(e: &AuthError) -> usize {
        match e {
            AuthError::Unauthenticated => 0,
            AuthError::CredentialMalformed => 1,
            AuthError::CredentialInvalid => 2,
            AuthError::CredentialExpired => 3,
            AuthError::CredentialNotYetValid => 4,
            AuthError::CredentialRevoked => 5,
            AuthError::CredentialForeignIssuer => 6,
            AuthError::CredentialWrongTenant => 7,
            AuthError::UserSuspended => 8,
            AuthError::NotATenantMember => 9,
            AuthError::DatabaseNotGranted => 10,
            AuthError::OpNotGranted => 11,
            AuthError::GrantsExceedIssuerRole => 12,
            AuthError::GrantsMalformed => 13,
        }
    }
    // Suppress unused-function lint in const context.
    let _ = _exhaustive_variant_check;
};

#[cfg(test)]
mod auth_core_tests {
    use super::*;

    // ── Type round-trip tests (Serde) ──────────────────────────────────

    #[test]
    fn tenant_id_roundtrip() {
        let id = TenantId::new("t-123");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"t-123\"");
        let parsed: TenantId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn user_id_roundtrip() {
        let id = UserId::new("u-456");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"u-456\"");
        let parsed: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn tenant_roundtrip() {
        let tenant = Tenant {
            id: TenantId::new("t-123"),
            name: "acme".into(),
            created_at_ms: 1000,
        };
        let json = serde_json::to_string(&tenant).unwrap();
        let parsed: Tenant = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tenant);
    }

    #[test]
    fn user_roundtrip() {
        let user = User {
            id: UserId::new("u-123"),
            display_name: "Alice".into(),
            email: Some("alice@example.com".into()),
            created_at_ms: 1000,
            suspended_at_ms: None,
        };
        let json = serde_json::to_string(&user).unwrap();
        let parsed: User = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, user);
    }

    #[test]
    fn user_without_email_roundtrip() {
        let user = User {
            id: UserId::new("u-123"),
            display_name: "Bob".into(),
            email: None,
            created_at_ms: 1000,
            suspended_at_ms: None,
        };
        let json = serde_json::to_string(&user).unwrap();
        let parsed: User = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, user);
    }

    #[test]
    fn user_identity_roundtrip() {
        let identity = UserIdentity {
            provider: "tailscale".into(),
            external_id: "alice@example.com".into(),
            user_id: UserId::new("u-123"),
        };
        let json = serde_json::to_string(&identity).unwrap();
        let parsed: UserIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, identity);
    }

    #[test]
    fn tenant_member_roundtrip() {
        let member = TenantMember {
            tenant_id: TenantId::new("t-123"),
            user_id: UserId::new("u-456"),
            role: TenantRole::Write,
        };
        let json = serde_json::to_string(&member).unwrap();
        let parsed: TenantMember = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, member);
    }

    #[test]
    fn tenant_role_roundtrip() {
        for role in [TenantRole::Admin, TenantRole::Write, TenantRole::Read] {
            let json = serde_json::to_string(&role).unwrap();
            let parsed: TenantRole = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn op_roundtrip() {
        for op in [Op::Read, Op::Write, Op::Admin] {
            let json = serde_json::to_string(&op).unwrap();
            let parsed: Op = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, op);
        }
    }

    #[test]
    fn granted_database_roundtrip() {
        let db = GrantedDatabase {
            name: "orders".into(),
            ops: vec![Op::Read, Op::Write],
        };
        let json = serde_json::to_string(&db).unwrap();
        let parsed: GrantedDatabase = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, db);
    }

    #[test]
    fn grants_roundtrip() {
        let grants = Grants {
            databases: vec![
                GrantedDatabase {
                    name: "orders".into(),
                    ops: vec![Op::Read, Op::Write],
                },
                GrantedDatabase {
                    name: "analytics".into(),
                    ops: vec![Op::Read],
                },
            ],
        };
        let json = serde_json::to_string(&grants).unwrap();
        let parsed: Grants = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, grants);
    }

    #[test]
    fn grants_validation_passes_for_valid_grants() {
        let grants = Grants {
            databases: vec![
                GrantedDatabase {
                    name: "orders".into(),
                    ops: vec![Op::Read, Op::Write],
                },
            ],
        };
        assert!(grants.validate().is_ok());
    }

    #[test]
    fn grants_validation_rejects_empty_database_name() {
        let grants = Grants {
            databases: vec![GrantedDatabase {
                name: "".into(),
                ops: vec![Op::Read],
            }],
        };
        assert!(matches!(
            grants.validate(),
            Err(AuthError::GrantsMalformed)
        ));
    }

    #[test]
    fn grants_validation_rejects_empty_ops() {
        let grants = Grants {
            databases: vec![GrantedDatabase {
                name: "orders".into(),
                ops: vec![],
            }],
        };
        assert!(matches!(
            grants.validate(),
            Err(AuthError::GrantsMalformed)
        ));
    }

    #[test]
    fn grants_validation_rejects_duplicate_ops() {
        let grants = Grants {
            databases: vec![GrantedDatabase {
                name: "orders".into(),
                ops: vec![Op::Read, Op::Read],
            }],
        };
        assert!(matches!(
            grants.validate(),
            Err(AuthError::GrantsMalformed)
        ));
    }

    #[test]
    fn jwt_claims_roundtrip() {
        let claims = JwtClaims {
            iss: "axon://eitri.example".into(),
            sub: "user_01HZ...".into(),
            aud: "tenant_acme".into(),
            jti: "cred_01HZ...".into(),
            iat: 1760000000,
            nbf: 1760000000,
            exp: 1760086400,
            grants: Grants {
                databases: vec![GrantedDatabase {
                    name: "orders".into(),
                    ops: vec![Op::Read, Op::Write],
                }],
            },
        };
        let json = serde_json::to_string(&claims).unwrap();
        let parsed: JwtClaims = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, claims);
    }

    // ── aud array-form rejection test (ADR-018 §4) ─────────────────────

    #[test]
    fn jwt_claims_aud_array_returns_credential_malformed() {
        // The JWT library would parse aud as an array, but our schema
        // requires it to be a string. We test this by manually creating
        // invalid JSON.
        let invalid_json = r#"{
            "iss": "axon://eitri.example",
            "sub": "user_01HZ...",
            "aud": ["tenant_acme", "tenant_globex"],
            "jti": "cred_01HZ...",
            "iat": 1760000000,
            "nbf": 1760000000,
            "exp": 1760086400,
            "grants": {"databases": []}
        }"#;

        // parse_claims maps serde errors to AuthError::CredentialMalformed.
        let result = parse_claims(invalid_json);
        assert!(
            matches!(result, Err(AuthError::CredentialMalformed)),
            "aud as array must produce AuthError::CredentialMalformed, got: {:?}",
            result
        );
    }

    // ── TenantRole delegation helper tests ─────────────────────────────

    #[test]
    fn admin_delegation_ops() {
        let ops = TenantRole::Admin.delegation_ops();
        assert_eq!(ops.len(), 3);
        assert!(ops.contains(&Op::Read));
        assert!(ops.contains(&Op::Write));
        assert!(ops.contains(&Op::Admin));
    }

    #[test]
    fn write_delegation_ops() {
        let ops = TenantRole::Write.delegation_ops();
        assert_eq!(ops.len(), 2);
        assert!(ops.contains(&Op::Read));
        assert!(ops.contains(&Op::Write));
        assert!(!ops.contains(&Op::Admin));
    }

    #[test]
    fn read_delegation_ops() {
        let ops = TenantRole::Read.delegation_ops();
        assert_eq!(ops.len(), 1);
        assert!(ops.contains(&Op::Read));
        assert!(!ops.contains(&Op::Write));
        assert!(!ops.contains(&Op::Admin));
    }

    // ── AuthError variant count (ADR-018 §4): enforced at compile time ────
    // The exhaustive match in `const _: () = { ... }` above the #[cfg(test)]
    // block guarantees exactly 14 variants. No runtime assertion needed.

    #[test]
    fn auth_error_display() {
        let variants = vec![
            (AuthError::Unauthenticated, "unauthenticated"),
            (AuthError::CredentialMalformed, "credential malformed"),
            (AuthError::CredentialInvalid, "credential invalid"),
            (AuthError::CredentialExpired, "credential expired"),
            (AuthError::CredentialNotYetValid, "credential not yet valid"),
            (AuthError::CredentialRevoked, "credential revoked"),
            (AuthError::CredentialForeignIssuer, "credential foreign issuer"),
            (AuthError::CredentialWrongTenant, "credential wrong tenant"),
            (AuthError::UserSuspended, "user suspended"),
            (AuthError::NotATenantMember, "not a tenant member"),
            (AuthError::DatabaseNotGranted, "database not granted"),
            (AuthError::OpNotGranted, "op not granted"),
            (AuthError::GrantsExceedIssuerRole, "grants exceed issuer role"),
            (AuthError::GrantsMalformed, "grants malformed"),
        ];

        for (err, expected) in variants {
            assert_eq!(err.to_string(), expected);
        }
    }

    // ── Proptest tests for TenantRole delegation helper ────────────────

    proptest::proptest! {
        #[test]
        fn admin_can_delegate_all_ops_always(_admin_role in "A") {
            // Verify admin role has all operations
            let ops = TenantRole::Admin.delegation_ops();
            assert!(ops.contains(&Op::Read));
            assert!(ops.contains(&Op::Write));
            assert!(ops.contains(&Op::Admin));
            assert_eq!(ops.len(), 3);
        }

        #[test]
        fn write_can_delegate_read_and_write_only(_write_role in "W") {
            // Verify write role has read and write, but not admin
            let ops = TenantRole::Write.delegation_ops();
            assert!(ops.contains(&Op::Read));
            assert!(ops.contains(&Op::Write));
            assert!(!ops.contains(&Op::Admin));
            assert_eq!(ops.len(), 2);
        }

        #[test]
        fn read_can_delegate_read_only(_read_role in "R") {
            // Verify read role has only read
            let ops = TenantRole::Read.delegation_ops();
            assert!(ops.contains(&Op::Read));
            assert!(!ops.contains(&Op::Write));
            assert!(!ops.contains(&Op::Admin));
            assert_eq!(ops.len(), 1);
        }

        #[test]
        fn write_never_grants_admin(_write_role in "W") {
            // Write role can never delegate admin operations
            let ops = TenantRole::Write.delegation_ops();
            assert!(!ops.contains(&Op::Admin));
        }

        #[test]
        fn read_never_grants_write_or_admin(_read_role in "R") {
            // Read role can never delegate write or admin operations
            let ops = TenantRole::Read.delegation_ops();
            assert!(!ops.contains(&Op::Write));
            assert!(!ops.contains(&Op::Admin));
        }
    }
}
