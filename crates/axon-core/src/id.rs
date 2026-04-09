use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Default database name for single-tenant deployments.
pub const DEFAULT_DATABASE: &str = "default";

/// Default schema name within a database.
pub const DEFAULT_SCHEMA: &str = "default";

/// A fully qualified namespace: `{database}.{schema}`.
///
/// Single-tenant deployments use `default.default` automatically.
/// Collection names are resolved against a namespace to produce
/// a fully qualified name: `{database}.{schema}.{collection}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    /// - `"billing.invoices"` -> `(default.billing, "invoices")`
    /// - `"mydb.public.beads"` -> `(mydb.public, "beads")`
    ///
    /// Two-part names like `"billing.invoices"` resolve to
    /// `(default.billing, "invoices")`.
    pub fn parse(name: &str) -> (Self, String) {
        Self::parse_with_database(name, DEFAULT_DATABASE)
    }

    /// Parse a potentially qualified name using a request-scoped current database.
    ///
    /// Accepts:
    /// - `"beads"` -> `({current_db}.default, "beads")`
    /// - `"billing.invoices"` -> `({current_db}.billing, "invoices")`
    /// - `"mydb.public.beads"` -> `(mydb.public, "beads")`
    pub fn parse_with_database(name: &str, current_database: &str) -> (Self, String) {
        let parts: Vec<&str> = name.split('.').collect();
        match parts.len() {
            1 => (
                Self::new(current_database, DEFAULT_SCHEMA),
                parts[0].to_string(),
            ),
            2 => (Self::new(current_database, parts[0]), parts[1].to_string()),
            _ => (Self::new(parts[0], parts[1]), parts[2..].join(".")),
        }
    }

    /// Resolve a collection name to a fully qualified string using a
    /// request-scoped current database for one-part and two-part names.
    pub fn qualify_with_database(name: &str, current_database: &str) -> String {
        let (namespace, collection) = Self::parse_with_database(name, current_database);
        namespace.qualify(&collection)
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

/// A namespace-qualified collection identifier: `{database}.{schema}.{collection}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct QualifiedCollectionId {
    pub namespace: Namespace,
    pub collection: CollectionId,
}

impl QualifiedCollectionId {
    pub fn new(namespace: Namespace, collection: CollectionId) -> Self {
        Self {
            namespace,
            collection,
        }
    }

    pub fn from_parts(namespace: &Namespace, collection: &CollectionId) -> Self {
        Self::new(namespace.clone(), collection.clone())
    }

    pub fn qualified_name(&self) -> String {
        self.namespace.qualify(self.collection.as_str())
    }
}

impl fmt::Display for QualifiedCollectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.qualified_name())
    }
}

/// UUID v5 namespace for deterministic ID generation from non-UUID strings (ADR-010).
///
/// Chosen as a well-known constant so that the same string always produces the
/// same UUID across all Axon instances.
pub const AXON_UUID_NAMESPACE: Uuid = Uuid::from_bytes([
    0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x47, 0x89, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78,
]);

/// Identifies an entity within a collection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(String);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Generate a new entity ID using UUIDv7 (time-sortable).
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Try to parse this entity ID as a UUID.
    ///
    /// Returns `Some(Uuid)` if the string is a valid UUID, `None` otherwise.
    pub fn as_uuid(&self) -> Option<Uuid> {
        Uuid::parse_str(&self.0).ok()
    }

    /// Convert to a UUID, generating a deterministic v5 UUID if the string
    /// is not already a valid UUID.
    ///
    /// Non-UUID strings are mapped to a stable UUID v5 using [`AXON_UUID_NAMESPACE`]
    /// so the same string always produces the same UUID.
    pub fn to_uuid(&self) -> Uuid {
        self.as_uuid()
            .unwrap_or_else(|| Uuid::new_v5(&AXON_UUID_NAMESPACE, self.0.as_bytes()))
    }

    /// Returns true if this entity ID is already a valid UUID.
    pub fn is_uuid(&self) -> bool {
        Uuid::parse_str(&self.0).is_ok()
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
    fn qualified_collection_id_roundtrip() {
        let qualified = QualifiedCollectionId::new(
            Namespace::new("prod", "billing"),
            CollectionId::new("invoices"),
        );
        assert_eq!(qualified.qualified_name(), "prod.billing.invoices");
        assert_eq!(qualified.to_string(), "prod.billing.invoices");
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
        let (ns, collection) = Namespace::parse("billing.invoices");
        assert_eq!(ns.database, "default");
        assert_eq!(ns.schema, "billing");
        assert_eq!(collection, "invoices");
    }

    #[test]
    fn parse_three_part_name() {
        let (ns, collection) = Namespace::parse("prod.public.beads");
        assert_eq!(ns.database, "prod");
        assert_eq!(ns.schema, "public");
        assert_eq!(collection, "beads");
    }

    #[test]
    fn parse_unqualified_name_uses_current_database_when_provided() {
        let (ns, collection) = Namespace::parse_with_database("beads", "prod");
        assert_eq!(ns, Namespace::new("prod", "default"));
        assert_eq!(collection, "beads");
    }

    #[test]
    fn parse_two_part_name_uses_current_database_when_provided() {
        let (ns, collection) = Namespace::parse_with_database("billing.invoices", "prod");
        assert_eq!(ns, Namespace::new("prod", "billing"));
        assert_eq!(collection, "invoices");
    }

    #[test]
    fn qualify_with_database_resolves_unqualified_and_two_part_names() {
        assert_eq!(
            Namespace::qualify_with_database("beads", "prod"),
            "prod.default.beads"
        );
        assert_eq!(
            Namespace::qualify_with_database("billing.invoices", "prod"),
            "prod.billing.invoices"
        );
        assert_eq!(
            Namespace::qualify_with_database("stage.analytics.rollups", "prod"),
            "stage.analytics.rollups"
        );
    }

    #[test]
    fn no_config_required_for_single_tenant() {
        // This verifies the zero-config requirement: creating a namespace
        // with Default gives you the standard single-tenant namespace.
        let ns = Namespace::default();
        assert_eq!(ns.qualify("beads"), "default.default.beads");
    }

    // ── Entity UUID tests (ADR-010) ────────────────────────────────────

    #[test]
    fn generate_produces_valid_uuid() {
        let id = EntityId::generate();
        assert!(id.is_uuid(), "generated id should be a valid UUID");
        let uuid = id
            .as_uuid()
            .expect("generated entity id should parse as a UUID");
        assert_eq!(uuid.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn generate_produces_unique_ids() {
        let a = EntityId::generate();
        let b = EntityId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn as_uuid_parses_valid_uuid_string() {
        let id = EntityId::new("550e8400-e29b-41d4-a716-446655440000");
        assert!(id.is_uuid());
        assert!(id.as_uuid().is_some());
    }

    #[test]
    fn as_uuid_returns_none_for_non_uuid() {
        let id = EntityId::new("ent-001");
        assert!(!id.is_uuid());
        assert!(id.as_uuid().is_none());
    }

    #[test]
    fn to_uuid_returns_v5_for_non_uuid_string() {
        let id = EntityId::new("ent-001");
        let uuid = id.to_uuid();
        assert_eq!(uuid.get_version(), Some(uuid::Version::Sha1));
    }

    #[test]
    fn to_uuid_is_deterministic_for_same_string() {
        let a = EntityId::new("task-42").to_uuid();
        let b = EntityId::new("task-42").to_uuid();
        assert_eq!(a, b);
    }

    #[test]
    fn to_uuid_passthrough_for_valid_uuid() {
        let raw = "550e8400-e29b-41d4-a716-446655440000";
        let id = EntityId::new(raw);
        let uuid = id.to_uuid();
        assert_eq!(uuid.to_string(), raw);
    }

    #[test]
    fn existing_string_ids_still_work() {
        // Backward compatibility: existing string-based EntityIds remain functional.
        let id = EntityId::new("my-old-entity");
        assert_eq!(id.as_str(), "my-old-entity");
        assert_eq!(id.to_string(), "my-old-entity");
    }
}
