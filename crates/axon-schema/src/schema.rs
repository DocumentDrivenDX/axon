use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_core::error::AxonError;
use axon_core::id::CollectionId;

use crate::access_control::AccessControlPolicy;
use crate::rules::ValidationRule;

/// Cardinality constraint for a link type (ADR-002).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

/// Definition of a single link type within a collection schema (ADR-002, Layer 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkTypeDef {
    /// The collection that target entities must belong to.
    pub target_collection: String,
    /// Cardinality constraint for this link type.
    pub cardinality: Cardinality,
    /// Whether at least one link of this type must exist for every entity.
    #[serde(default)]
    pub required: bool,
    /// Optional JSON Schema 2020-12 for validating link metadata.
    pub metadata_schema: Option<Value>,
}

/// Lifecycle definition for a state-machine field (ESF Layer 6).
///
/// Describes valid states and allowed transitions for a single field
/// (e.g. `status`). Used by `transition_lifecycle` to validate state changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleDef {
    /// The entity field this lifecycle governs (e.g. `"status"`).
    pub field: String,
    /// The initial state for new entities.
    pub initial: String,
    /// Map from state name to the list of states reachable from it.
    pub transitions: HashMap<String, Vec<String>>,
}

/// Gate definition declared in the schema (ESF Layer 5).
///
/// Gates group validation rules by purpose. The `save` gate blocks persistence;
/// custom gates (e.g. `complete`, `review`) allow saves but track readiness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateDef {
    /// Human-readable description of what this gate means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Other gates whose rules are included in this gate.
    /// e.g., `review` includes `complete` means all complete rules
    /// must also pass for review to pass.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
}

/// Value type for an index field (ESF Layer 4).
///
/// Determines which EAV index table the value is stored in and how
/// comparisons (equality, range) are performed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexType {
    String,
    Integer,
    Float,
    Datetime,
    Boolean,
}

impl std::fmt::Display for IndexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexType::String => write!(f, "string"),
            IndexType::Integer => write!(f, "integer"),
            IndexType::Float => write!(f, "float"),
            IndexType::Datetime => write!(f, "datetime"),
            IndexType::Boolean => write!(f, "boolean"),
        }
    }
}

/// A single-field secondary index declaration (ESF Layer 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    /// JSON field path to index (e.g. `"status"`, `"address.city"`).
    pub field: String,
    /// Value type for this index.
    #[serde(rename = "type")]
    pub index_type: IndexType,
    /// When `true`, the index enforces uniqueness: no two entities in
    /// the same collection may share the same indexed value.
    #[serde(default)]
    pub unique: bool,
}

/// A single field within a compound index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexField {
    /// JSON field path.
    pub field: String,
    /// Value type.
    #[serde(rename = "type")]
    pub index_type: IndexType,
}

/// A compound (multi-field) secondary index declaration (ESF Layer 4, US-033).
///
/// Compound indexes accelerate queries that filter on multiple fields.
/// The field order matters: leftmost prefix matching is supported (a
/// compound index on `[status, priority]` also accelerates queries
/// filtering on `status` alone).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundIndexDef {
    /// Ordered list of fields in the compound key.
    pub fields: Vec<CompoundIndexField>,
    /// When `true`, the combination of all indexed field values must be unique.
    #[serde(default)]
    pub unique: bool,
}

/// A named schema-declared graph query (FEAT-009 / ADR-021).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedQueryDef {
    /// Human-readable description surfaced through generated API metadata.
    pub description: String,
    /// Read-only openCypher subset query text.
    pub cypher: String,
    /// Caller-supplied parameters accepted by the query.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<NamedQueryParameter>,
}

/// A single named-query parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedQueryParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
}

/// Defines the structure and constraints for entities in a collection.
///
/// The `entity_schema` field holds a JSON Schema 2020-12 document (Layer 1 of ESF)
/// that validates entity bodies on every create/update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionSchema {
    /// The collection this schema governs.
    pub collection: CollectionId,
    /// Human-readable description.
    pub description: Option<String>,
    /// Schema version; incremented on each migration.
    pub version: u32,
    /// The JSON Schema 2020-12 document for entity body validation (Layer 1 of ESF).
    /// When `None`, no structural validation is enforced (all entities are accepted).
    pub entity_schema: Option<Value>,
    /// Link-type definitions (Layer 2 of ESF). Keys are link-type names.
    #[serde(default)]
    pub link_types: HashMap<String, LinkTypeDef>,
    /// Data-layer access-control policy metadata (FEAT-029 / ADR-019).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_control: Option<AccessControlPolicy>,
    /// Gate definitions (ESF Layer 5). Keys are gate names.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub gates: HashMap<String, GateDef>,
    /// Validation rules (ESF Layer 5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_rules: Vec<ValidationRule>,
    /// Secondary index declarations (ESF Layer 4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexDef>,
    /// Compound index declarations (ESF Layer 4, US-033).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_indexes: Vec<CompoundIndexDef>,
    /// Schema-declared named graph queries (FEAT-009 / US-075).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub queries: HashMap<String, NamedQueryDef>,
    /// Lifecycle definitions (ESF Layer 6). Keys are lifecycle names.
    #[serde(default)]
    pub lifecycles: HashMap<String, LifecycleDef>,
}

/// Presentation metadata for a collection, versioned independently from the
/// validation schema.
///
/// Markdown templates are a rendering concern. Keeping them in a sibling type
/// avoids inflating schema versions when only presentation changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionView {
    /// The collection this view describes.
    pub collection: CollectionId,
    /// Optional human-readable description for the view itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Markdown template used to render entities in the collection.
    pub markdown_template: String,
    /// View version; incremented on each template update.
    pub version: u32,
    /// Nanoseconds since Unix epoch when the view was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ns: Option<u64>,
    /// Actor who last updated the view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

impl CollectionView {
    pub fn new(collection: CollectionId, markdown_template: impl Into<String>) -> Self {
        Self {
            collection,
            description: None,
            markdown_template: markdown_template.into(),
            version: 1,
            updated_at_ns: None,
            updated_by: None,
        }
    }
}

impl CollectionSchema {
    pub fn new(collection: CollectionId) -> Self {
        Self {
            collection,
            description: None,
            version: 1,
            entity_schema: None,
            link_types: HashMap::new(),
            access_control: None,
            gates: HashMap::new(),
            validation_rules: Vec::new(),
            indexes: Vec::new(),
            compound_indexes: Vec::new(),
            queries: HashMap::new(),
            lifecycles: HashMap::new(),
        }
    }
}

pub const JSON_LD_RESERVED_KEYWORDS: [&str; 4] = ["@context", "@graph", "@id", "@type"];

pub fn json_ld_reserved_field_aliases(schema: &CollectionSchema) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let Some(properties) = schema
        .entity_schema
        .as_ref()
        .and_then(|entity_schema| entity_schema.get("properties"))
        .and_then(Value::as_object)
    else {
        return aliases;
    };

    let mut occupied: BTreeSet<String> = properties
        .keys()
        .filter(|name| !is_json_ld_reserved_keyword(name))
        .cloned()
        .collect();

    let mut reserved: Vec<&String> = properties
        .keys()
        .filter(|name| is_json_ld_reserved_keyword(name))
        .collect();
    reserved.sort();

    for name in reserved {
        let base = match name.as_str() {
            "@context" => "axon_context",
            "@graph" => "axon_graph",
            "@id" => "axon_id",
            "@type" => "axon_type",
            _ => "axon_field",
        };
        let alias = unique_json_ld_alias(base, &mut occupied);
        aliases.insert(name.clone(), alias);
    }

    aliases
}

pub fn json_ld_reserved_field_warnings(schema: &CollectionSchema) -> Vec<String> {
    json_ld_reserved_field_aliases(schema)
        .into_iter()
        .map(|(field, alias)| {
            format!(
                "field '{field}' collides with a JSON-LD reserved keyword and will be exposed as '{alias}' in JSON-LD contexts"
            )
        })
        .collect()
}

pub fn is_json_ld_reserved_keyword(name: &str) -> bool {
    JSON_LD_RESERVED_KEYWORDS.contains(&name)
}

fn unique_json_ld_alias(base: &str, occupied: &mut BTreeSet<String>) -> String {
    if occupied.insert(base.to_string()) {
        return base.to_string();
    }

    for suffix in 2.. {
        let candidate = format!("{base}_{suffix}");
        if occupied.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded suffix search should always return")
}

/// A parsed Entity Schema Format (ESF) document.
///
/// ESF uses a three-layer model (see ADR-002):
/// - Layer 1: JSON Schema 2020-12 for entity body validation
/// - Layer 2: Axon link-type definitions
/// - Layer 3: Axon validation rules with severity levels
///
/// Parsing is YAML-first; JSON is also accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsfDocument {
    /// ESF format version (e.g., `"1.0"`).
    pub esf_version: String,
    /// The collection this schema governs.
    pub collection: String,
    /// Layer 1: JSON Schema 2020-12 document governing entity bodies.
    pub entity_schema: Option<Value>,
    /// Layer 2: Link-type definitions (Axon vocabulary). Stored as raw JSON for now.
    pub link_types: Option<Value>,
    /// Schema-adjacent data-layer access-control policy metadata.
    pub access_control: Option<Value>,
    /// Schema-declared named graph queries.
    pub queries: Option<Value>,
    /// Layer 3: Custom validation rules with severity. Stored as raw JSON for now.
    pub validation_rules: Option<Value>,
    /// Layer 6: Lifecycle definitions. Stored as raw JSON for now.
    pub lifecycles: Option<Value>,
}

impl EsfDocument {
    /// Parse an ESF document from a YAML (or JSON) string.
    ///
    /// Returns `AxonError::SchemaValidation` if the input cannot be parsed or
    /// is missing required top-level fields.
    pub fn parse(input: &str) -> Result<Self, AxonError> {
        let value: Value = serde_yaml::from_str(input)
            .map_err(|e| AxonError::SchemaValidation(format!("ESF parse error: {e}")))?;
        serde_json::from_value(value)
            .map_err(|e| AxonError::SchemaValidation(format!("ESF structure error: {e}")))
    }

    /// Convert this ESF document into a [`CollectionSchema`] using the collection
    /// name from the document, the Layer 1 JSON Schema, and Layer 2 link-type
    /// definitions.
    pub fn into_collection_schema(self) -> Result<CollectionSchema, AxonError> {
        let link_types: HashMap<String, LinkTypeDef> = match self.link_types {
            Some(val) => serde_json::from_value(val).map_err(|e| {
                AxonError::SchemaValidation(format!("invalid link_types definition: {e}"))
            })?,
            None => HashMap::new(),
        };
        let lifecycles: HashMap<String, LifecycleDef> = match self.lifecycles {
            Some(val) => serde_json::from_value(val).map_err(|e| {
                AxonError::SchemaValidation(format!("invalid lifecycles definition: {e}"))
            })?,
            None => HashMap::new(),
        };
        let access_control: Option<AccessControlPolicy> = match self.access_control {
            Some(val) => Some(serde_json::from_value(val).map_err(|e| {
                AxonError::SchemaValidation(format!("invalid access_control definition: {e}"))
            })?),
            None => None,
        };
        let queries: HashMap<String, NamedQueryDef> = match self.queries {
            Some(val) => serde_json::from_value(val).map_err(|e| {
                AxonError::SchemaValidation(format!("invalid queries definition: {e}"))
            })?,
            None => HashMap::new(),
        };
        Ok(CollectionSchema {
            collection: CollectionId::new(self.collection),
            description: None,
            version: 1,
            entity_schema: self.entity_schema,
            link_types,
            access_control,
            gates: HashMap::new(),
            validation_rules: Vec::new(),
            indexes: Vec::new(),
            compound_indexes: Vec::new(),
            queries,
            lifecycles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_new_starts_at_version_one() {
        let schema = CollectionSchema::new(CollectionId::new("tasks"));
        assert_eq!(schema.version, 1);
        assert!(schema.description.is_none());
        assert!(schema.entity_schema.is_none());
    }

    #[test]
    fn collection_view_new_starts_at_version_one() {
        let view = CollectionView::new(CollectionId::new("tasks"), "# {{title}}");
        assert_eq!(view.version, 1);
        assert_eq!(view.markdown_template, "# {{title}}");
        assert!(view.updated_at_ns.is_none());
        assert!(view.updated_by.is_none());
    }

    /// Sample ESF document derived from ADR-002 (invoice collection).
    const INVOICE_ESF: &str = r#"
esf_version: "1.0"
collection: invoices
entity_schema:
  type: object
  required:
    - vendor_id
    - amount
    - status
  properties:
    vendor_id:
      type: string
    amount:
      type: object
      properties:
        value:
          type: number
          minimum: 0
        currency:
          type: string
          enum: [USD, EUR, GBP]
    status:
      type: string
      enum: [draft, submitted, approved, paid, reconciled]
    line_items:
      type: array
      items:
        type: object
        properties:
          description:
            type: string
          quantity:
            type: integer
            minimum: 1
          unit_price:
            type: number
            minimum: 0
"#;

    const ACCESS_CONTROL_ESF: &str = r#"
esf_version: "1.0"
collection: engagements
entity_schema:
  type: object
  required: [name, status, members]
  properties:
    name: { type: string }
    status: { type: string, enum: [draft, active, closed] }
    budget_cents: { type: integer }
    amount_cents: { type: integer }
    lead_partner_id: { type: string }
    members:
      type: array
      items:
        type: object
        required: [user_id, role]
        properties:
          user_id: { type: string }
          role: { type: string }

access_control:
  identity:
    user_id: subject.user_id
    role: subject.attributes.user_role

  read:
    allow:
      - name: firm-admins-see-all
        when: { subject: role, in: [admin, partner] }
      - name: assigned-consultants-see-own
        when: { subject: role, in: [consultant, contractor] }
        where:
          field: members[].user_id
          contains_subject: user_id

  write:
    allow:
      - name: admins-write-all
        when: { subject: role, eq: admin }
      - name: partners-write-led-engagements
        when: { subject: role, eq: partner }
        where:
          field: lead_partner_id
          eq_subject: user_id

  fields:
    budget_cents:
      read:
        deny:
          - name: contractors-do-not-see-budget
            when: { subject: role, eq: contractor }
            redact_as: null
      write:
        allow:
          - name: admins-only-budget
            when: { subject: role, eq: admin }

  transitions:
    status:
      activate:
        allow:
          - name: partners-activate-led-engagements
            when: { subject: role, eq: partner }
            where: { field: lead_partner_id, eq_subject: user_id }

  envelopes:
    write:
      - name: auto-small-engagement-adjustment
        when:
          all:
            - { operation: update }
            - { field: amount_cents, lte: 1000000 }
            - { subject: role, in: [finance, admin] }
        decision: allow
      - name: approve-large-engagement-adjustment
        when:
          all:
            - { operation: update }
            - { field: amount_cents, gt: 1000000 }
        decision: needs_approval
        approval:
          role: finance_approver
          reason_required: true
"#;

    #[test]
    fn parse_esf_from_adr_002() {
        let doc = EsfDocument::parse(INVOICE_ESF).expect("invoice ESF fixture should parse");
        assert_eq!(doc.esf_version, "1.0");
        assert_eq!(doc.collection, "invoices");
        assert!(
            doc.entity_schema.is_some(),
            "entity_schema should be present"
        );
        let schema = doc
            .entity_schema
            .as_ref()
            .expect("invoice ESF fixture should include an entity schema");
        let required = schema["required"]
            .as_array()
            .expect("invoice ESF fixture should mark required fields");
        assert!(required.iter().any(|v| v == "vendor_id"));
        assert!(required.iter().any(|v| v == "amount"));
        assert!(required.iter().any(|v| v == "status"));
    }

    #[test]
    fn esf_into_collection_schema() {
        let doc = EsfDocument::parse(INVOICE_ESF).expect("invoice ESF fixture should parse");
        let schema = doc
            .into_collection_schema()
            .expect("invoice ESF fixture should convert to collection schema");
        assert_eq!(schema.collection.as_str(), "invoices");
        assert_eq!(schema.version, 1);
        assert!(schema.entity_schema.is_some());
    }

    #[test]
    fn esf_parses_access_control_metadata() {
        let doc = EsfDocument::parse(ACCESS_CONTROL_ESF).expect("access-control ESF should parse");
        let schema = doc
            .into_collection_schema()
            .expect("access-control ESF should convert to collection schema");
        let policy = schema
            .access_control
            .expect("access-control policy should be present");

        let identity = policy.identity.expect("identity metadata missing");
        assert_eq!(
            identity.aliases.get("user_id").map(String::as_str),
            Some("subject.user_id")
        );
        assert_eq!(
            identity.aliases.get("role").map(String::as_str),
            Some("subject.attributes.user_role")
        );

        let read_policy = policy.read.expect("read policy missing");
        assert_eq!(read_policy.allow.len(), 2);
        assert_eq!(
            read_policy.allow[0].name.as_deref(),
            Some("firm-admins-see-all")
        );
        assert!(read_policy.allow[1].where_clause.is_some());

        let budget_policy = policy
            .fields
            .get("budget_cents")
            .expect("budget field policy missing");
        let read = budget_policy
            .read
            .as_ref()
            .expect("field read policy missing");
        assert_eq!(read.deny.len(), 1);
        assert_eq!(
            read.deny[0].redact_as.as_ref(),
            Some(&serde_json::Value::Null)
        );

        let status_transitions = policy
            .transitions
            .get("status")
            .expect("status transition policies missing");
        assert!(status_transitions.contains_key("activate"));

        let write_envelopes = policy
            .envelopes
            .get(&crate::access_control::PolicyOperation::Write)
            .expect("write envelopes missing");
        assert_eq!(write_envelopes.len(), 2);
        assert_eq!(
            write_envelopes[1].decision,
            crate::access_control::PolicyDecision::NeedsApproval
        );
        assert_eq!(
            write_envelopes[1]
                .approval
                .as_ref()
                .and_then(|route| route.role.as_deref()),
            Some("finance_approver")
        );
    }

    #[test]
    fn access_control_round_trips_through_collection_schema_json() {
        let schema = EsfDocument::parse(ACCESS_CONTROL_ESF)
            .expect("access-control ESF should parse")
            .into_collection_schema()
            .expect("access-control ESF should convert to collection schema");

        let json = serde_json::to_string(&schema).expect("schema should serialize");
        let restored: CollectionSchema =
            serde_json::from_str(&json).expect("schema should deserialize");

        assert_eq!(restored.access_control, schema.access_control);
    }

    #[test]
    fn json_esf_parses_access_control_metadata() {
        let yaml_doc =
            EsfDocument::parse(ACCESS_CONTROL_ESF).expect("access-control ESF should parse");
        let json_esf = serde_json::to_string(&yaml_doc).expect("ESF document should serialize");

        let schema = EsfDocument::parse(&json_esf)
            .expect("JSON ESF should parse")
            .into_collection_schema()
            .expect("JSON ESF should convert to collection schema");

        assert!(schema.access_control.is_some());
    }

    #[test]
    fn collection_schema_without_access_control_defaults_to_none() {
        let doc = EsfDocument::parse(INVOICE_ESF).expect("invoice ESF fixture should parse");
        let schema = doc
            .into_collection_schema()
            .expect("invoice ESF fixture should convert to collection schema");
        assert!(schema.access_control.is_none());
    }

    #[test]
    fn invalid_access_control_returns_schema_validation_error() {
        let malformed = r#"
esf_version: "1.0"
collection: broken
entity_schema:
  type: object
access_control:
  read: definitely-not-a-policy
"#;
        let doc = EsfDocument::parse(malformed).expect("malformed policy block is structural YAML");
        let err = doc
            .into_collection_schema()
            .expect_err("malformed access-control block should fail");
        assert!(
            matches!(
                err,
                AxonError::SchemaValidation(ref msg)
                    if msg.starts_with("invalid access_control definition:")
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let result = EsfDocument::parse("{ not: valid yaml: [");
        assert!(result.is_err());
    }

    #[test]
    fn esf_parses_lifecycles_from_beads_fixture() {
        let esf = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/beads.esf.yaml"),
        )
        .expect("beads.esf.yaml fixture missing");
        let doc = EsfDocument::parse(&esf).unwrap();
        let schema = doc.into_collection_schema().unwrap();

        let lc = schema
            .lifecycles
            .get("status")
            .expect("status lifecycle missing");
        assert_eq!(lc.field, "status");
        assert_eq!(lc.initial, "draft");

        let from_draft = lc
            .transitions
            .get("draft")
            .expect("draft transitions missing");
        assert!(from_draft.contains(&"pending".to_string()));
        assert!(from_draft.contains(&"cancelled".to_string()));
    }

    #[test]
    fn collection_schema_without_lifecycles_defaults_to_empty() {
        let doc = EsfDocument::parse(INVOICE_ESF).unwrap();
        let schema = doc.into_collection_schema().unwrap();
        assert!(schema.lifecycles.is_empty());
    }

    #[test]
    fn lifecycle_def_round_trips_through_json() {
        let mut transitions = HashMap::new();
        transitions.insert("draft".to_string(), vec!["active".to_string()]);
        transitions.insert("active".to_string(), vec!["closed".to_string()]);
        let lc = LifecycleDef {
            field: "status".to_string(),
            initial: "draft".to_string(),
            transitions,
        };
        let mut lifecycles = HashMap::new();
        lifecycles.insert("status".to_string(), lc.clone());
        let schema = CollectionSchema {
            collection: CollectionId::new("items"),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: HashMap::new(),
            access_control: None,
            gates: HashMap::new(),
            validation_rules: Vec::new(),
            indexes: Vec::new(),
            compound_indexes: Vec::new(),
            queries: HashMap::new(),
            lifecycles,
        };
        let json = serde_json::to_string(&schema).unwrap();
        let restored: CollectionSchema = serde_json::from_str(&json).unwrap();
        let restored_lc = restored.lifecycles.get("status").unwrap();
        assert_eq!(restored_lc.field, lc.field);
        assert_eq!(restored_lc.initial, lc.initial);
        assert_eq!(restored_lc.transitions, lc.transitions);
    }

    #[test]
    fn collection_schema_new_has_empty_lifecycles() {
        let schema = CollectionSchema::new(CollectionId::new("tasks"));
        assert!(schema.lifecycles.is_empty());
    }
}
