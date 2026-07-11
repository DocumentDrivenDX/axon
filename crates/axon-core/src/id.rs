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

mod system_seal {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Seal;
}

/// Typed class for reserved Axon-owned collection names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemCollectionClass {
    /// Forward link store formerly exposed as a pseudo-collection.
    LinkForwardStore,
    /// Reverse inbound-link index formerly exposed as a pseudo-collection.
    LinkReverseIndex,
    /// Durable CDC/client-projection checkpoint collection.
    CheckpointCursorStore,
    /// Synthetic audit subject for mutation-intent lifecycle events.
    MutationIntentAuditSubject,
    /// Axon-native bead/task collection.
    BeadCatalog,
    /// Stale policy pseudo-collection alias retained only for compatibility docs.
    LegacyPolicyAlias,
}

/// Sealed constructor surface for Axon-owned collection names.
///
/// `SystemCollection` is intentionally not constructible outside this module:
/// callers must choose one of the named constructors, which keeps every reserved
/// collection tied to a single typed class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemCollection {
    name: &'static str,
    class: SystemCollectionClass,
    _seal: system_seal::Seal,
}

impl SystemCollection {
    /// Internal forward link store (`__axon_links__`).
    pub const fn links() -> Self {
        Self::new("__axon_links__", SystemCollectionClass::LinkForwardStore)
    }

    /// Internal reverse inbound-link index (`__axon_links_rev__`).
    pub const fn links_rev() -> Self {
        Self::new(
            "__axon_links_rev__",
            SystemCollectionClass::LinkReverseIndex,
        )
    }

    /// Durable CDC/client-projection cursor checkpoints (`_cdc_cursors`).
    pub const fn cdc_cursors() -> Self {
        Self::new("_cdc_cursors", SystemCollectionClass::CheckpointCursorStore)
    }

    /// Synthetic audit subject for mutation-intent lifecycle events.
    pub const fn mutation_intents() -> Self {
        Self::new(
            "__mutation_intents",
            SystemCollectionClass::MutationIntentAuditSubject,
        )
    }

    /// Axon-native bead/task collection (`__axon_beads__`).
    pub const fn beads() -> Self {
        Self::new("__axon_beads__", SystemCollectionClass::BeadCatalog)
    }

    /// Stale policy pseudo-collection alias (`__axon_policies__`).
    pub const fn legacy_policies() -> Self {
        Self::new(
            "__axon_policies__",
            SystemCollectionClass::LegacyPolicyAlias,
        )
    }

    /// Resolve a reserved name into its single system class.
    pub fn from_reserved_name(name: &str) -> Option<Self> {
        match name {
            "__axon_links__" => Some(Self::links()),
            "__axon_links_rev__" => Some(Self::links_rev()),
            "_cdc_cursors" => Some(Self::cdc_cursors()),
            "__mutation_intents" => Some(Self::mutation_intents()),
            "__axon_beads__" => Some(Self::beads()),
            "__axon_policies__" => Some(Self::legacy_policies()),
            _ => None,
        }
    }

    /// Reserved collection name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Typed class assigned to this reserved collection.
    pub const fn class(&self) -> SystemCollectionClass {
        self.class
    }

    /// Materialize the reserved name as a storage-facing [`CollectionId`].
    pub fn collection_id(&self) -> CollectionId {
        CollectionId::new(self.name)
    }

    const fn new(name: &'static str, class: SystemCollectionClass) -> Self {
        Self {
            name,
            class,
            _seal: system_seal::Seal,
        }
    }
}

/// Typed class for audit-addressable subjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuditSubjectClass {
    /// User-authored collection state.
    UserCollection,
    /// Reserved Axon-owned collection state.
    SystemCollection(SystemCollectionClass),
    /// Physical storage catalog table or index.
    StorageCatalog,
    /// Auth/tenancy physical table.
    AuthCatalog,
    /// Audit log physical table.
    AuditLog,
    /// Idempotency state outside entity collections.
    Idempotency,
    /// Derived read/projection state.
    Projection,
}

/// Sealed audit subject constructor surface.
///
/// Public constructors validate or require typed inputs so a reserved
/// collection cannot be smuggled in as an ordinary user collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSubject {
    name: String,
    class: AuditSubjectClass,
    _seal: system_seal::Seal,
}

impl AuditSubject {
    /// Build an audit subject for user collection state.
    pub fn user_collection(collection: CollectionId) -> Option<Self> {
        if SystemCollection::from_reserved_name(collection.as_str()).is_some() {
            return None;
        }
        Some(Self::new(
            collection.as_str().to_owned(),
            AuditSubjectClass::UserCollection,
        ))
    }

    /// Build an audit subject for a reserved system collection.
    pub fn system_collection(collection: SystemCollection) -> Self {
        Self::new(
            collection.name().to_owned(),
            AuditSubjectClass::SystemCollection(collection.class()),
        )
    }

    /// Build an audit subject for a physical storage catalog object.
    pub fn storage_catalog(name: &'static str) -> Self {
        Self::new(name.to_owned(), AuditSubjectClass::StorageCatalog)
    }

    /// Build an audit subject for a physical auth/tenancy object.
    pub fn auth_catalog(name: &'static str) -> Self {
        Self::new(name.to_owned(), AuditSubjectClass::AuthCatalog)
    }

    /// Build an audit subject for the physical audit log.
    pub fn audit_log(name: &'static str) -> Self {
        Self::new(name.to_owned(), AuditSubjectClass::AuditLog)
    }

    /// Build an audit subject for idempotency state.
    pub fn idempotency(name: &'static str) -> Self {
        Self::new(name.to_owned(), AuditSubjectClass::Idempotency)
    }

    /// Build an audit subject for derived projection state.
    pub fn projection(name: &'static str) -> Self {
        Self::new(name.to_owned(), AuditSubjectClass::Projection)
    }

    /// Subject name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Typed subject class.
    pub const fn class(&self) -> AuditSubjectClass {
        self.class
    }

    fn new(name: String, class: AuditSubjectClass) -> Self {
        Self {
            name,
            class,
            _seal: system_seal::Seal,
        }
    }
}

/// Sealed token identifying the governed entity-write path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GovernedWriteTx {
    _seal: system_seal::Seal,
}

/// Sealed token identifying storage migration authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationCapability {
    _seal: system_seal::Seal,
}

/// Sealed token identifying checkpoint/cursor write authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointCapability {
    _seal: system_seal::Seal,
}

#[cfg(test)]
impl GovernedWriteTx {
    pub(crate) const fn storage_adapter() -> Self {
        Self {
            _seal: system_seal::Seal,
        }
    }
}

#[cfg(test)]
impl MigrationCapability {
    pub(crate) const fn storage_migration() -> Self {
        Self {
            _seal: system_seal::Seal,
        }
    }
}

#[cfg(test)]
impl CheckpointCapability {
    pub(crate) const fn storage_checkpoint() -> Self {
        Self {
            _seal: system_seal::Seal,
        }
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
    fn system_collection_known_reserved_names_round_trip() {
        let known = [
            (
                SystemCollection::links(),
                "__axon_links__",
                SystemCollectionClass::LinkForwardStore,
            ),
            (
                SystemCollection::links_rev(),
                "__axon_links_rev__",
                SystemCollectionClass::LinkReverseIndex,
            ),
            (
                SystemCollection::cdc_cursors(),
                "_cdc_cursors",
                SystemCollectionClass::CheckpointCursorStore,
            ),
            (
                SystemCollection::mutation_intents(),
                "__mutation_intents",
                SystemCollectionClass::MutationIntentAuditSubject,
            ),
            (
                SystemCollection::beads(),
                "__axon_beads__",
                SystemCollectionClass::BeadCatalog,
            ),
            (
                SystemCollection::legacy_policies(),
                "__axon_policies__",
                SystemCollectionClass::LegacyPolicyAlias,
            ),
        ];

        for (collection, name, class) in known {
            assert_eq!(collection.name(), name);
            assert_eq!(collection.class(), class);
            assert_eq!(collection.collection_id().as_str(), name);
            assert_eq!(SystemCollection::from_reserved_name(name), Some(collection));
        }
    }

    #[test]
    fn system_collection_rejects_unmanifested_reserved_names() {
        assert!(SystemCollection::from_reserved_name("__axon_unknown__").is_none());
        assert!(SystemCollection::from_reserved_name("tasks").is_none());
    }

    #[test]
    fn system_collection_audit_subjects_are_typed() {
        let subject = AuditSubject::system_collection(SystemCollection::links());
        assert_eq!(subject.name(), "__axon_links__");
        assert_eq!(
            subject.class(),
            AuditSubjectClass::SystemCollection(SystemCollectionClass::LinkForwardStore)
        );
    }

    #[test]
    fn system_collection_reserved_names_cannot_be_user_audit_subjects() {
        assert!(AuditSubject::user_collection(CollectionId::new("tasks")).is_some());
        assert!(AuditSubject::user_collection(CollectionId::new("__axon_links__")).is_none());
        assert!(AuditSubject::user_collection(CollectionId::new("__axon_policies__")).is_none());
    }

    #[test]
    fn system_collection_capability_tokens_are_sealed() {
        let governed = GovernedWriteTx::storage_adapter();
        let migration = MigrationCapability::storage_migration();
        let checkpoint = CheckpointCapability::storage_checkpoint();

        assert_eq!(governed, GovernedWriteTx::storage_adapter());
        assert_eq!(migration, MigrationCapability::storage_migration());
        assert_eq!(checkpoint, CheckpointCapability::storage_checkpoint());
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
