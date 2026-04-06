use serde::{Deserialize, Serialize};
use std::fmt;

/// Default database name for single-tenant deployments.
pub const DEFAULT_DATABASE: &str = "default";

/// Default schema name within a database.
pub const DEFAULT_SCHEMA: &str = "default";

/// A fully qualified namespace: `{database}.{schema}`.
///
/// Single-tenant deployments use `default.default` automatically.
/// Collection names are resolved against a namespace to produce
/// a fully qualified name: `{database}.{schema}.{collection}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Namespace {
    pub database: String,
    pub schema: String,
}

impl Namespace {
    /// Create a new namespace.
    pub fn new(database: impl Into<String>, schema: impl Into<String>) -> Self {
        Self {
            database: database.into(),
            schema: schema.into(),
        }
    }

    /// The default namespace (`default.default`) for single-tenant use.
    pub fn default_ns() -> Self {
        Self {
            database: DEFAULT_DATABASE.into(),
            schema: DEFAULT_SCHEMA.into(),
        }
    }

    /// Resolve a collection name against this namespace to produce a
    /// fully qualified name `{database}.{schema}.{collection}`.
    pub fn qualify(&self, collection: &str) -> String {
        format!("{}.{}.{}", self.database, self.schema, collection)
    }

    /// Parse a potentially qualified name into `(Namespace, collection)`.
    ///
    /// Accepts:
    /// - `"beads"` -> `(default.default, "beads")`
    /// - `"mydb.public.beads"` -> `(mydb.public, "beads")`
    ///
    /// Two-part names like `"mydb.beads"` resolve to `(mydb.default, "beads")`.
    pub fn parse(name: &str) -> (Self, String) {
        let parts: Vec<&str> = name.split('.').collect();
        match parts.len() {
            1 => (Self::default_ns(), parts[0].to_string()),
            2 => (
                Self::new(parts[0], DEFAULT_SCHEMA),
                parts[1].to_string(),
            ),
            _ => (
                Self::new(parts[0], parts[1]),
                parts[2..].join("."),
            ),
        }
    }
}

impl Default for Namespace {
    fn default() -> Self {
        Self::default_ns()
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.database, self.schema)
    }
}

/// Identifies a collection within an Axon instance.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CollectionId(String);

impl CollectionId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CollectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifies an entity within a collection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(String);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifies a typed link between two entities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LinkId(String);

impl LinkId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LinkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_id_roundtrip() {
        let id = CollectionId::new("tasks");
        assert_eq!(id.as_str(), "tasks");
        assert_eq!(id.to_string(), "tasks");
    }

    #[test]
    fn entity_id_roundtrip() {
        let id = EntityId::new("ent-001");
        assert_eq!(id.as_str(), "ent-001");
    }

    #[test]
    fn link_id_roundtrip() {
        let id = LinkId::new("link-001");
        assert_eq!(id.as_str(), "link-001");
    }

    // ── Namespace tests (US-037) ────────────────────────────────────────

    #[test]
    fn default_namespace() {
        let ns = Namespace::default_ns();
        assert_eq!(ns.database, "default");
        assert_eq!(ns.schema, "default");
        assert_eq!(ns.to_string(), "default.default");
    }

    #[test]
    fn namespace_default_trait() {
        let ns = Namespace::default();
        assert_eq!(ns, Namespace::default_ns());
    }

    #[test]
    fn qualify_collection_name() {
        let ns = Namespace::default_ns();
        assert_eq!(ns.qualify("beads"), "default.default.beads");

        let ns = Namespace::new("prod", "public");
        assert_eq!(ns.qualify("tasks"), "prod.public.tasks");
    }

    #[test]
    fn parse_unqualified_name_uses_default() {
        let (ns, collection) = Namespace::parse("beads");
        assert_eq!(ns, Namespace::default_ns());
        assert_eq!(collection, "beads");
    }

    #[test]
    fn parse_two_part_name() {
        let (ns, collection) = Namespace::parse("mydb.beads");
        assert_eq!(ns.database, "mydb");
        assert_eq!(ns.schema, "default");
        assert_eq!(collection, "beads");
    }

    #[test]
    fn parse_three_part_name() {
        let (ns, collection) = Namespace::parse("prod.public.beads");
        assert_eq!(ns.database, "prod");
        assert_eq!(ns.schema, "public");
        assert_eq!(collection, "beads");
    }

    #[test]
    fn no_config_required_for_single_tenant() {
        // This verifies the zero-config requirement: creating a namespace
        // with Default gives you the standard single-tenant namespace.
        let ns = Namespace::default();
        assert_eq!(ns.qualify("beads"), "default.default.beads");
    }
}
