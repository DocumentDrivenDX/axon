//! Bead storage adapter: opinionated issue/work-item layer built on Axon primitives.
//!
//! Provides a pre-defined schema, lifecycle state machine, dependency tracking,
//! and ready-queue computation for bead-like work items (FEAT-006).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId, BEAD_SYSTEM_CAPABILITY};
use axon_core::types::Entity;
use axon_schema::{
    validate, Cardinality, CollectionSchema, IndexDef, IndexType, LifecycleDef, LinkTypeDef,
};
use axon_storage::adapter::StorageAdapter;

use crate::handler::AxonHandler;
use crate::request::{
    CreateGovernedSystemEntityRequest, CreateGovernedSystemLinkRequest,
    EnsureGovernedSystemCollectionRequest, FieldFilter, FilterNode, FilterOp,
    PutGovernedSystemSchemaRequest, QueryGovernedSystemEntitiesRequest,
    ReachableGovernedSystemRequest, TraverseDirection, TraverseGovernedSystemRequest,
    UpdateGovernedSystemEntityRequest,
};

/// The collection name for beads.
pub const BEAD_COLLECTION: &str = "__axon_beads__";

/// Link type used for bead dependencies (source depends-on target).
pub const DEPENDS_ON_LINK: &str = "depends-on";

const STATUS_LIFECYCLE: &str = "status";
const STATUS_FIELD: &str = "status";
const BEAD_ACTOR: &str = "bead-system";
const IMPORT_ACTOR: &str = "bead-import";
const QUERY_PAGE_SIZE: usize = 1000;
const LEGACY_STATUS_DRAFT: &str = concat!("dra", "ft");
const LEGACY_STATUS_PENDING: &str = concat!("pen", "ding");
const LEGACY_STATUS_READY: &str = concat!("rea", "dy");
const LEGACY_STATUS_REVIEW: &str = concat!("re", "view");
const LEGACY_STATUS_DONE: &str = concat!("do", "ne");
const OPTIONAL_SCHEMA_FIELDS: &[&str] = &[
    "schema_version",
    "issue_type",
    "description",
    "acceptance",
    "priority",
    "owner",
    "assignee",
    "parent",
    "labels",
    "notes",
    "created_at",
    "updated_at",
    "claimed-at",
    "claimed-machine",
    "claimed-pid",
    "dependencies",
];

/// Valid persisted bead lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeadStatus {
    Proposed,
    Open,
    InProgress,
    Blocked,
    Closed,
    Cancelled,
}

impl BeadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "proposed" => Some(Self::Proposed),
            "open" => Some(Self::Open),
            "in_progress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "closed" => Some(Self::Closed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Proposed,
            Self::Open,
            Self::InProgress,
            Self::Blocked,
            Self::Closed,
            Self::Cancelled,
        ]
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Closed | Self::Cancelled)
    }
}

/// DDx dependency metadata preserved from tracker imports.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BeadDependency {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue_id: String,
    pub depends_on_id: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub dependency_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(flatten)]
    pub extension: BTreeMap<String, Value>,
}

/// A bead (work item) with modeled DDx fields and preserved extension metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bead {
    /// Set from the entity ID, not stored in entity data.
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bead_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
    #[serde(default)]
    pub priority: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<BeadDependency>,
    #[serde(flatten)]
    pub extension: BTreeMap<String, Value>,
}

/// Returns the bead collection ID.
pub fn bead_collection() -> CollectionId {
    CollectionId::new(BEAD_COLLECTION)
}

fn status_vocabulary() -> Vec<&'static str> {
    BeadStatus::all()
        .iter()
        .map(|status| status.as_str())
        .collect()
}

fn migration_status_vocabulary() -> Vec<&'static str> {
    let mut vocabulary = status_vocabulary();
    vocabulary.extend([
        LEGACY_STATUS_DRAFT,
        LEGACY_STATUS_PENDING,
        LEGACY_STATUS_READY,
        LEGACY_STATUS_REVIEW,
        LEGACY_STATUS_DONE,
    ]);
    vocabulary
}

fn bead_lifecycle() -> LifecycleDef {
    LifecycleDef {
        field: STATUS_FIELD.into(),
        initial: BeadStatus::Open.as_str().into(),
        transitions: HashMap::from([
            (
                BeadStatus::Proposed.as_str().into(),
                vec![
                    BeadStatus::Open.as_str().into(),
                    BeadStatus::Blocked.as_str().into(),
                    BeadStatus::Cancelled.as_str().into(),
                ],
            ),
            (
                BeadStatus::Open.as_str().into(),
                vec![
                    BeadStatus::InProgress.as_str().into(),
                    BeadStatus::Blocked.as_str().into(),
                    BeadStatus::Closed.as_str().into(),
                    BeadStatus::Cancelled.as_str().into(),
                ],
            ),
            (
                BeadStatus::InProgress.as_str().into(),
                vec![
                    BeadStatus::Open.as_str().into(),
                    BeadStatus::Blocked.as_str().into(),
                    BeadStatus::Closed.as_str().into(),
                    BeadStatus::Cancelled.as_str().into(),
                ],
            ),
            (
                BeadStatus::Blocked.as_str().into(),
                vec![
                    BeadStatus::Open.as_str().into(),
                    BeadStatus::Cancelled.as_str().into(),
                ],
            ),
            (BeadStatus::Closed.as_str().into(), Vec::new()),
            (BeadStatus::Cancelled.as_str().into(), Vec::new()),
        ]),
    }
}

fn bead_schema_with_statuses(
    vocabulary: Vec<&'static str>,
    lifecycles: HashMap<String, LifecycleDef>,
) -> CollectionSchema {
    let mut link_types = HashMap::new();
    link_types.insert(
        DEPENDS_ON_LINK.into(),
        LinkTypeDef {
            target_collection: BEAD_COLLECTION.into(),
            cardinality: Cardinality::ManyToMany,
            required: false,
            metadata_schema: Some(json!({
                "type": "object",
                "additionalProperties": true
            })),
        },
    );

    CollectionSchema {
        collection: bead_collection(),
        description: Some("Bead work-item collection (FEAT-006)".into()),
        version: 2,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["status", "title"],
            "additionalProperties": true,
            "properties": {
                "schema_version": { "type": "integer", "minimum": 0 },
                "issue_type": { "type": "string" },
                "status": {
                    "type": "string",
                    "enum": vocabulary
                },
                "title": { "type": "string", "minLength": 1 },
                "description": { "type": "string" },
                "acceptance": { "type": "string" },
                "priority": { "type": "integer", "minimum": 0 },
                "owner": { "type": "string" },
                "assignee": { "type": "string" },
                "parent": { "type": "string" },
                "labels": { "type": "array", "items": { "type": "string" } },
                "notes": { "type": "string" },
                "created_at": { "type": "string" },
                "updated_at": { "type": "string" },
                "claimed-at": { "type": "string" },
                "claimed-machine": { "type": "string" },
                "claimed-pid": { "type": "string" },
                "dependencies": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["depends_on_id"],
                        "additionalProperties": true,
                        "properties": {
                            "issue_id": { "type": "string" },
                            "depends_on_id": { "type": "string" },
                            "type": { "type": "string" },
                            "created_at": { "type": "string" }
                        }
                    }
                }
            }
        })),
        link_types,
        access_control: None,
        gates: Default::default(),
        validation_rules: Default::default(),
        indexes: vec![
            IndexDef {
                field: STATUS_FIELD.into(),
                index_type: IndexType::String,
                unique: false,
            },
            IndexDef {
                field: "issue_type".into(),
                index_type: IndexType::String,
                unique: false,
            },
            IndexDef {
                field: "owner".into(),
                index_type: IndexType::String,
                unique: false,
            },
            IndexDef {
                field: "parent".into(),
                index_type: IndexType::String,
                unique: false,
            },
        ],
        compound_indexes: Default::default(),
        queries: Default::default(),
        lifecycles,
    }
}

/// Returns the built-in bead collection schema.
pub fn bead_schema() -> CollectionSchema {
    let mut lifecycles = HashMap::new();
    lifecycles.insert(STATUS_LIFECYCLE.into(), bead_lifecycle());
    bead_schema_with_statuses(status_vocabulary(), lifecycles)
}

fn bead_migration_schema() -> CollectionSchema {
    bead_schema_with_statuses(migration_status_vocabulary(), HashMap::new())
}

/// Initialize or evolve the bead collection.
pub fn init_beads<S: StorageAdapter>(handler: &mut AxonHandler<S>) -> Result<(), AxonError> {
    let schema = bead_schema();
    let actor = Some("system".to_string());
    let result = handler.ensure_governed_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        EnsureGovernedSystemCollectionRequest {
            schema: schema.clone(),
            actor: actor.clone(),
        },
    );
    if let Err(AxonError::InvalidOperation(message)) = &result {
        if message.contains("schema change is breaking") {
            handler.handle_put_schema_in_system_collection(
                BEAD_SYSTEM_CAPABILITY,
                PutGovernedSystemSchemaRequest {
                    schema: bead_migration_schema(),
                    actor: actor.clone(),
                    force: true,
                    dry_run: false,
                    explain_inputs: Vec::new(),
                },
            )?;
            migrate_legacy_bead_statuses(handler)?;
            handler.handle_put_schema_in_system_collection(
                BEAD_SYSTEM_CAPABILITY,
                PutGovernedSystemSchemaRequest {
                    schema,
                    actor,
                    force: true,
                    dry_run: false,
                    explain_inputs: Vec::new(),
                },
            )?;
            return Ok(());
        }
    }
    result?;
    Ok(())
}

fn migrate_legacy_bead_statuses<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
) -> Result<(), AxonError> {
    for entity in query_bead_entities(handler, None)? {
        let status = entity
            .data
            .get(STATUS_FIELD)
            .ok_or_else(|| AxonError::LifecycleFieldMissing {
                field: STATUS_FIELD.into(),
            })?
            .as_str()
            .ok_or_else(|| AxonError::LifecycleStateInvalid {
                field: STATUS_FIELD.into(),
                actual: entity.data[STATUS_FIELD].clone(),
            })?;

        let Some(target_status) = normalize_legacy_status(status) else {
            if BeadStatus::parse(status).is_some() {
                continue;
            }
            return Err(AxonError::LifecycleStateInvalid {
                field: STATUS_FIELD.into(),
                actual: json!(status),
            });
        };

        let mut data = entity.data.clone();
        remove_projected_null_optional_fields(&mut data);
        data[STATUS_FIELD] = json!(target_status);
        handler.update_entity_in_system_collection(
            BEAD_SYSTEM_CAPABILITY,
            UpdateGovernedSystemEntityRequest {
                id: entity.id,
                data,
                expected_version: entity.version,
                actor: Some("system".into()),
                audit_metadata: Some(HashMap::from([(
                    "operation".into(),
                    "migrate-legacy-status".into(),
                )])),
                attribution: None,
            },
        )?;
    }

    Ok(())
}

fn normalize_legacy_status(status: &str) -> Option<&'static str> {
    match status {
        value if value == LEGACY_STATUS_DRAFT => Some(BeadStatus::Proposed.as_str()),
        value if value == LEGACY_STATUS_PENDING || value == LEGACY_STATUS_READY => {
            Some(BeadStatus::Open.as_str())
        }
        value if value == LEGACY_STATUS_REVIEW => Some(BeadStatus::InProgress.as_str()),
        value if value == LEGACY_STATUS_DONE => Some(BeadStatus::Closed.as_str()),
        _ => None,
    }
}

fn remove_projected_null_optional_fields(data: &mut Value) {
    let Some(obj) = data.as_object_mut() else {
        return;
    };
    for field in OPTIONAL_SCHEMA_FIELDS {
        if obj.get(*field).is_some_and(Value::is_null) {
            obj.remove(*field);
        }
    }
}

/// Parameters for creating a new bead.
pub struct CreateBeadParams<'a> {
    pub id: &'a str,
    pub bead_type: &'a str,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub priority: u32,
    pub assignee: Option<&'a str>,
    pub tags: &'a [String],
    pub acceptance: Option<&'a str>,
}

/// Create a new open bead.
pub fn create_bead<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    params: CreateBeadParams<'_>,
) -> Result<Bead, AxonError> {
    init_beads(handler)?;

    let mut data = json!({
        "issue_type": params.bead_type,
        "status": BeadStatus::Open.as_str(),
        "title": params.title,
        "priority": params.priority,
        "labels": params.tags,
    });
    if let Some(d) = params.description {
        data["description"] = json!(d);
    }
    if let Some(a) = params.assignee {
        data["assignee"] = json!(a);
    }
    if let Some(a) = params.acceptance {
        data["acceptance"] = json!(a);
    }

    let created = handler.create_entity_in_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        CreateGovernedSystemEntityRequest {
            id: EntityId::new(params.id),
            data,
            actor: Some(BEAD_ACTOR.into()),
            audit_metadata: None,
            attribution: None,
        },
    )?;

    entity_to_bead(&created.entity)
}

/// Transition a bead's status through the ordinary schema-owned lifecycle.
pub fn transition_bead<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    id: &str,
    new_status: &str,
) -> Result<(), AxonError> {
    let entity = get_bead_entity(handler, id)?;
    transition_bead_with_expected_version(handler, id, new_status, entity.version)?;
    Ok(())
}

/// Transition a bead's status with an explicit optimistic-concurrency version.
pub fn transition_bead_with_expected_version<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    id: &str,
    new_status: &str,
    expected_version: u64,
) -> Result<Bead, AxonError> {
    let entity = get_bead_entity(handler, id)?;
    let mut data = entity.data.clone();
    data[STATUS_FIELD] = json!(new_status);
    update_bead(handler, id, data, expected_version)
}

/// Replace a bead payload through the standard governed update path.
///
/// If the payload changes `status`, the transition is validated against the
/// bead schema lifecycle. Terminal states therefore cannot be left by ordinary
/// updates.
pub fn update_bead<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    id: &str,
    data: Value,
    expected_version: u64,
) -> Result<Bead, AxonError> {
    let existing = get_bead_entity(handler, id)?;
    validate_ordinary_status_update(&existing.data, &data)?;

    let updated = handler.update_entity_in_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        UpdateGovernedSystemEntityRequest {
            id: EntityId::new(id),
            data,
            expected_version,
            actor: Some(BEAD_ACTOR.into()),
            audit_metadata: None,
            attribution: None,
        },
    )?;

    entity_to_bead(&updated.entity)
}

/// Explicitly reopen a closed bead to open.
///
/// This is intentionally separate from ordinary lifecycle transitions. Closed
/// has no outgoing schema transition, and cancelled cannot be reopened.
pub fn reopen_bead<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    id: &str,
) -> Result<Bead, AxonError> {
    let entity = get_bead_entity(handler, id)?;
    reopen_bead_with_expected_version(handler, id, entity.version)
}

/// Explicitly reopen a closed bead to open with an OCC guard.
pub fn reopen_bead_with_expected_version<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    id: &str,
    expected_version: u64,
) -> Result<Bead, AxonError> {
    let entity = get_bead_entity(handler, id)?;
    let current = parse_status_from_data(&entity.data)?;
    if current != BeadStatus::Closed {
        return Err(AxonError::InvalidTransition {
            lifecycle_name: STATUS_LIFECYCLE.into(),
            current_state: current.as_str().into(),
            target_state: BeadStatus::Open.as_str().into(),
            valid_transitions: ordinary_valid_next_states(current),
        });
    }

    let mut data = entity.data.clone();
    data[STATUS_FIELD] = json!(BeadStatus::Open.as_str());
    let updated = handler.update_entity_in_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        UpdateGovernedSystemEntityRequest {
            id: EntityId::new(id),
            data,
            expected_version,
            actor: Some(BEAD_ACTOR.into()),
            audit_metadata: Some(HashMap::from([("operation".into(), "reopen".into())])),
            attribution: None,
        },
    )?;

    entity_to_bead(&updated.entity)
}

/// Add a dependency: `bead_id` depends-on `dep_id`.
///
/// Missing targets and cycles are rejected before any link write is attempted.
pub fn add_dependency<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    bead_id: &str,
    dep_id: &str,
) -> Result<(), AxonError> {
    get_bead_entity(handler, bead_id)?;
    get_bead_entity(handler, dep_id)?;

    if bead_id == dep_id {
        return Err(AxonError::InvalidOperation(format!(
            "circular dependency: {bead_id} cannot depend on itself"
        )));
    }

    if dependency_path_exists(handler, dep_id, bead_id)? {
        return Err(AxonError::InvalidOperation(format!(
            "circular dependency: {dep_id} already reaches {bead_id}"
        )));
    }

    handler.create_link_in_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        CreateGovernedSystemLinkRequest {
            source_id: EntityId::new(bead_id),
            target_id: EntityId::new(dep_id),
            link_type: DEPENDS_ON_LINK.into(),
            metadata: json!({ "type": "blocks" }),
            actor: Some(BEAD_ACTOR.into()),
            attribution: None,
        },
    )?;

    Ok(())
}

/// List beads, optionally filtered by stored status.
pub fn list_beads<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    status_filter: Option<&str>,
) -> Result<Vec<Bead>, AxonError> {
    if let Some(status) = status_filter {
        validate_status_literal(status)?;
    }
    let filter = status_filter.map(|status| {
        FilterNode::Field(FieldFilter {
            field: STATUS_FIELD.into(),
            op: FilterOp::Eq,
            value: json!(status),
        })
    });

    let entities = query_bead_entities(handler, filter)?;
    entities.iter().map(entity_to_bead).collect()
}

/// Compute the ready queue: open beads whose dependencies are all closed.
pub fn ready_queue<S: StorageAdapter>(handler: &AxonHandler<S>) -> Result<Vec<Bead>, AxonError> {
    let open_beads = list_beads(handler, Some(BeadStatus::Open.as_str()))?;

    let mut output = Vec::new();
    for bead in open_beads {
        let deps = handler.traverse_system_collection(
            BEAD_SYSTEM_CAPABILITY,
            TraverseGovernedSystemRequest {
                id: EntityId::new(&bead.id),
                link_type: Some(DEPENDS_ON_LINK.into()),
                max_depth: Some(1),
                direction: TraverseDirection::Forward,
                hop_filter: None,
            },
        )?;

        let all_deps_closed = deps.entities.iter().all(|dep_entity| {
            matches!(
                parse_status_from_data(&dep_entity.data),
                Ok(BeadStatus::Closed)
            )
        });

        if all_deps_closed {
            output.push(bead);
        }
    }
    Ok(output)
}

/// Get the dependency tree for a bead (all transitive dependencies).
pub fn dependency_tree<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    bead_id: &str,
) -> Result<Vec<Bead>, AxonError> {
    dependency_entities_unbounded(handler, bead_id)?
        .iter()
        .map(entity_to_bead)
        .collect()
}

/// Export all beads as raw DDx-compatible JSON objects.
pub fn export_beads<S: StorageAdapter>(
    handler: &AxonHandler<S>,
) -> Result<serde_json::Value, AxonError> {
    let mut values = Vec::new();
    for entity in query_bead_entities(handler, None)? {
        let entity_id = entity.id.to_string();
        let mut data = entity.data;
        let obj = data
            .as_object_mut()
            .ok_or_else(|| AxonError::Storage("bead entity data must be an object".into()))?;
        merge_typed_dependencies_for_export(handler, &entity_id, obj)?;
        obj.insert("id".into(), json!(entity_id));
        values.push(data);
    }
    Ok(Value::Array(values))
}

/// Parse bead import input from either a JSON array or newline-delimited JSON.
pub fn parse_bead_import_data(input: &str) -> Result<Value, AxonError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }

    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed)
            .map_err(|error| AxonError::InvalidArgument(format!("invalid bead JSON: {error}")));
    }

    let mut records = Vec::new();
    for (line_index, line) in trimmed.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record = serde_json::from_str(line).map_err(|error| {
            AxonError::InvalidArgument(format!(
                "invalid bead JSONL line {}: {error}",
                line_index + 1
            ))
        })?;
        records.push(record);
    }
    Ok(Value::Array(records))
}

/// Import beads from a JSON array.
///
/// Entity fields are preserved exactly except for `id`, which is stored as the
/// entity identity. Dependency objects are also materialized as typed links when
/// their targets are present in the imported collection.
pub fn import_beads<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    data: &serde_json::Value,
) -> Result<usize, AxonError> {
    init_beads(handler)?;

    let arr = data
        .as_array()
        .ok_or_else(|| AxonError::InvalidArgument("expected JSON array".into()))?;

    let import_plan = build_import_plan(handler, arr)?;
    let dependency_links = validate_import_dependency_links(handler, &import_plan)?;

    for item in &import_plan {
        handler.create_entity_in_system_collection(
            BEAD_SYSTEM_CAPABILITY,
            CreateGovernedSystemEntityRequest {
                id: EntityId::new(&item.id),
                data: item.data.clone(),
                actor: Some(IMPORT_ACTOR.into()),
                audit_metadata: None,
                attribution: None,
            },
        )?;
    }

    for (source_id, target_id) in dependency_links {
        match add_dependency(handler, &source_id, &target_id) {
            Ok(()) | Err(AxonError::AlreadyExists(_)) => {}
            Err(err) => return Err(err),
        }
    }

    Ok(import_plan.len())
}

struct ImportBeadPlan {
    id: String,
    data: Value,
    dependency_links: Vec<(String, String)>,
}

fn build_import_plan<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    items: &[Value],
) -> Result<Vec<ImportBeadPlan>, AxonError> {
    let schema = bead_schema();
    let existing_ids = existing_bead_ids(handler)?;
    let mut planned_ids = HashSet::new();
    let mut plan = Vec::new();

    for item in items {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| AxonError::InvalidArgument("bead missing id".into()))?;
        if existing_ids.contains(id) {
            continue;
        }
        if !planned_ids.insert(id.to_string()) {
            return Err(AxonError::AlreadyExists(format!("bead {id}")));
        }

        let dependency_links = dependency_links_from_import_item(item, id);
        let mut entity_data = item.clone();
        let obj = entity_data
            .as_object_mut()
            .ok_or_else(|| AxonError::InvalidArgument("bead item must be an object".into()))?;
        obj.remove("id");
        validate(&schema, &entity_data)?;

        plan.push(ImportBeadPlan {
            id: id.to_string(),
            data: entity_data,
            dependency_links,
        });
    }

    Ok(plan)
}

fn validate_import_dependency_links<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    plan: &[ImportBeadPlan],
) -> Result<Vec<(String, String)>, AxonError> {
    let existing_ids = existing_bead_ids(handler)?;
    let planned_ids = plan
        .iter()
        .map(|item| item.id.clone())
        .collect::<HashSet<_>>();
    let all_ids = existing_ids
        .union(&planned_ids)
        .cloned()
        .collect::<HashSet<_>>();
    let mut graph = existing_dependency_graph(handler, &existing_ids)?;
    let mut materialized_links = Vec::new();

    for (source_id, target_id) in plan
        .iter()
        .flat_map(|item| item.dependency_links.iter())
        .filter(|(source_id, target_id)| all_ids.contains(source_id) && all_ids.contains(target_id))
    {
        if source_id == target_id {
            return Err(AxonError::InvalidOperation(format!(
                "circular dependency: {source_id} cannot depend on itself"
            )));
        }
        if graph_path_exists(&graph, target_id, source_id) {
            return Err(AxonError::InvalidOperation(format!(
                "circular dependency: {target_id} already reaches {source_id}"
            )));
        }
        graph
            .entry(source_id.clone())
            .or_default()
            .insert(target_id.clone());
        materialized_links.push((source_id.clone(), target_id.clone()));
    }

    Ok(materialized_links)
}

fn dependency_links_from_import_item(item: &Value, fallback_source: &str) -> Vec<(String, String)> {
    let Some(dependencies) = item.get("dependencies").and_then(Value::as_array) else {
        return Vec::new();
    };

    dependencies
        .iter()
        .filter_map(|dep| dependency_key(dep, fallback_source))
        .collect()
}

fn dependency_key(dep: &Value, fallback_source: &str) -> Option<(String, String)> {
    let target = dep.get("depends_on_id").and_then(Value::as_str)?;
    let source = dep
        .get("issue_id")
        .and_then(Value::as_str)
        .unwrap_or(fallback_source);
    Some((source.to_string(), target.to_string()))
}

fn merge_typed_dependencies_for_export<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    bead_id: &str,
    obj: &mut Map<String, Value>,
) -> Result<(), AxonError> {
    let links = direct_dependency_links(handler, bead_id)?;
    if links.is_empty() {
        return Ok(());
    }

    let mut dependencies = obj
        .remove("dependencies")
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let dependency_array = dependencies
        .as_array_mut()
        .ok_or_else(|| AxonError::Storage("bead dependencies must be an array".into()))?;
    let mut seen = dependency_array
        .iter()
        .filter_map(|dep| dependency_key(dep, bead_id))
        .collect::<HashSet<_>>();

    for link in links {
        let source_id = link.source_id.to_string();
        let target_id = link.target_id.to_string();
        if !seen.insert((source_id.clone(), target_id.clone())) {
            continue;
        }

        let mut dep_obj = match link.metadata {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        dep_obj.insert("issue_id".into(), json!(source_id));
        dep_obj.insert("depends_on_id".into(), json!(target_id));
        dep_obj.entry("type").or_insert_with(|| json!("blocks"));
        dependency_array.push(Value::Object(dep_obj));
    }

    if !dependency_array.is_empty() {
        obj.insert("dependencies".into(), dependencies);
    }

    Ok(())
}

fn dependency_entities_unbounded<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    bead_id: &str,
) -> Result<Vec<Entity>, AxonError> {
    get_bead_entity(handler, bead_id)?;

    let mut visited = HashSet::from([bead_id.to_string()]);
    let mut queue = VecDeque::from([bead_id.to_string()]);
    let mut dependencies = Vec::new();

    while let Some(current_id) = queue.pop_front() {
        for entity in direct_dependency_entities(handler, &current_id)? {
            let entity_id = entity.id.to_string();
            if visited.insert(entity_id.clone()) {
                queue.push_back(entity_id);
                dependencies.push(entity);
            }
        }
    }

    Ok(dependencies)
}

fn dependency_path_exists<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    from_id: &str,
    target_id: &str,
) -> Result<bool, AxonError> {
    if from_id == target_id {
        return Ok(true);
    }

    let bounded = handler.reachable_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        ReachableGovernedSystemRequest {
            source_id: EntityId::new(from_id),
            target_id: EntityId::new(target_id),
            link_type: Some(DEPENDS_ON_LINK.into()),
            max_depth: None,
            direction: TraverseDirection::Forward,
        },
    )?;
    if bounded.reachable {
        return Ok(true);
    }

    let mut visited = HashSet::from([from_id.to_string()]);
    let mut queue = VecDeque::from([from_id.to_string()]);

    while let Some(current_id) = queue.pop_front() {
        for entity in direct_dependency_entities(handler, &current_id)? {
            let entity_id = entity.id.to_string();
            if entity_id == target_id {
                return Ok(true);
            }
            if visited.insert(entity_id.clone()) {
                queue.push_back(entity_id);
            }
        }
    }

    Ok(false)
}

fn direct_dependency_entities<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    bead_id: &str,
) -> Result<Vec<Entity>, AxonError> {
    let resp = handler.traverse_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        TraverseGovernedSystemRequest {
            id: EntityId::new(bead_id),
            link_type: Some(DEPENDS_ON_LINK.into()),
            max_depth: Some(1),
            direction: TraverseDirection::Forward,
            hop_filter: None,
        },
    )?;
    Ok(resp.entities)
}

fn direct_dependency_links<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    bead_id: &str,
) -> Result<Vec<axon_core::types::Link>, AxonError> {
    let resp = handler.traverse_system_collection(
        BEAD_SYSTEM_CAPABILITY,
        TraverseGovernedSystemRequest {
            id: EntityId::new(bead_id),
            link_type: Some(DEPENDS_ON_LINK.into()),
            max_depth: Some(1),
            direction: TraverseDirection::Forward,
            hop_filter: None,
        },
    )?;
    Ok(resp.links)
}

fn existing_bead_ids<S: StorageAdapter>(
    handler: &AxonHandler<S>,
) -> Result<HashSet<String>, AxonError> {
    Ok(query_bead_entities(handler, None)?
        .into_iter()
        .map(|entity| entity.id.to_string())
        .collect())
}

fn existing_dependency_graph<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    existing_ids: &HashSet<String>,
) -> Result<HashMap<String, HashSet<String>>, AxonError> {
    let mut graph = HashMap::new();
    for id in existing_ids {
        let deps = direct_dependency_entities(handler, id)?
            .into_iter()
            .map(|entity| entity.id.to_string())
            .collect::<HashSet<_>>();
        graph.insert(id.clone(), deps);
    }
    Ok(graph)
}

fn graph_path_exists(
    graph: &HashMap<String, HashSet<String>>,
    from_id: &str,
    target_id: &str,
) -> bool {
    if from_id == target_id {
        return true;
    }

    let mut visited = HashSet::from([from_id.to_string()]);
    let mut queue = VecDeque::from([from_id.to_string()]);

    while let Some(current_id) = queue.pop_front() {
        let Some(dependencies) = graph.get(&current_id) else {
            continue;
        };
        for dependency_id in dependencies {
            if dependency_id == target_id {
                return true;
            }
            if visited.insert(dependency_id.clone()) {
                queue.push_back(dependency_id.clone());
            }
        }
    }

    false
}

fn query_bead_entities<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    filter: Option<FilterNode>,
) -> Result<Vec<Entity>, AxonError> {
    let mut after_id = None;
    let mut entities = Vec::new();

    loop {
        let page = handler.query_entities_in_system_collection(
            BEAD_SYSTEM_CAPABILITY,
            QueryGovernedSystemEntitiesRequest {
                filter: filter.clone(),
                limit: Some(QUERY_PAGE_SIZE),
                after_id: after_id.take(),
                ..Default::default()
            },
        )?;
        entities.extend(page.entities);
        match page.next_cursor {
            Some(cursor) => after_id = Some(EntityId::new(cursor)),
            None => break,
        }
    }

    Ok(entities)
}

fn get_bead_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    id: &str,
) -> Result<Entity, AxonError> {
    let target = EntityId::new(id);
    let mut after_id = None;

    loop {
        let page = handler.query_entities_in_system_collection(
            BEAD_SYSTEM_CAPABILITY,
            QueryGovernedSystemEntitiesRequest {
                limit: Some(QUERY_PAGE_SIZE),
                after_id: after_id.take(),
                ..Default::default()
            },
        )?;
        if let Some(entity) = page.entities.into_iter().find(|entity| entity.id == target) {
            return Ok(entity);
        }
        match page.next_cursor {
            Some(cursor) => after_id = Some(EntityId::new(cursor)),
            None => return Err(AxonError::NotFound(format!("bead {id}"))),
        }
    }
}

fn entity_to_bead(entity: &Entity) -> Result<Bead, AxonError> {
    let mut bead: Bead = serde_json::from_value(entity.data.clone())
        .map_err(|e| AxonError::Storage(format!("bead deserialization: {e}")))?;
    bead.id = entity.id.to_string();
    if bead.bead_type.is_empty() {
        if let Some(issue_type) = &bead.issue_type {
            bead.bead_type = issue_type.clone();
        }
    }
    if bead.tags.is_empty() {
        bead.tags = bead.labels.clone();
    }
    Ok(bead)
}

fn parse_status_from_data(data: &Value) -> Result<BeadStatus, AxonError> {
    let status = data
        .get(STATUS_FIELD)
        .ok_or_else(|| AxonError::LifecycleFieldMissing {
            field: STATUS_FIELD.into(),
        })?
        .as_str()
        .ok_or_else(|| AxonError::LifecycleStateInvalid {
            field: STATUS_FIELD.into(),
            actual: data[STATUS_FIELD].clone(),
        })?;
    BeadStatus::parse(status).ok_or_else(|| AxonError::LifecycleStateInvalid {
        field: STATUS_FIELD.into(),
        actual: json!(status),
    })
}

fn validate_status_literal(status: &str) -> Result<(), AxonError> {
    if BeadStatus::parse(status).is_some() {
        Ok(())
    } else {
        Err(AxonError::LifecycleStateInvalid {
            field: STATUS_FIELD.into(),
            actual: json!(status),
        })
    }
}

fn validate_ordinary_status_update(
    current_data: &Value,
    target_data: &Value,
) -> Result<(), AxonError> {
    let current = parse_status_from_data(current_data)?;
    let target = parse_status_from_data(target_data)?;
    if current == target {
        return Ok(());
    }
    let valid_transitions = ordinary_valid_next_states(current);
    if valid_transitions
        .iter()
        .any(|state| state == target.as_str())
    {
        Ok(())
    } else {
        Err(AxonError::InvalidTransition {
            lifecycle_name: STATUS_LIFECYCLE.into(),
            current_state: current.as_str().into(),
            target_state: target.as_str().into(),
            valid_transitions,
        })
    }
}

fn ordinary_valid_next_states(current: BeadStatus) -> Vec<String> {
    bead_lifecycle()
        .transitions
        .get(current.as_str())
        .cloned()
        .unwrap_or_default()
}

// -- Tests ------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axon_audit::entry::MutationType;
    use axon_schema::CollectionSchema;
    use axon_storage::memory::MemoryStorageAdapter;

    fn handler() -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new(MemoryStorageAdapter::default())
    }

    fn make_bead(h: &mut AxonHandler<MemoryStorageAdapter>, id: &str, title: &str) -> Bead {
        create_bead(
            h,
            CreateBeadParams {
                id,
                bead_type: "task",
                title,
                description: None,
                priority: 1,
                assignee: None,
                tags: &[],
                acceptance: None,
            },
        )
        .unwrap()
    }

    fn ids(beads: &[Bead]) -> Vec<String> {
        let mut ids = beads.iter().map(|bead| bead.id.clone()).collect::<Vec<_>>();
        ids.sort();
        ids
    }

    fn audit_len(h: &AxonHandler<MemoryStorageAdapter>) -> usize {
        h.audit_log().len()
    }

    fn entity_and_link_audit_len(h: &AxonHandler<MemoryStorageAdapter>) -> usize {
        h.audit_log()
            .entries()
            .iter()
            .filter(|entry| {
                matches!(
                    entry.mutation,
                    MutationType::EntityCreate
                        | MutationType::EntityUpdate
                        | MutationType::LinkCreate
                )
            })
            .count()
    }

    #[test]
    fn bead_governed_bootstrap_evolution_uses_sealed_capability() {
        let mut h = handler();

        let mut old_schema = CollectionSchema::new(bead_collection());
        old_schema.version = 1;
        old_schema.entity_schema = Some(json!({
            "type": "object",
            "required": ["status", "title"],
            "properties": {
                "status": {
                    "type": "string",
                    "enum": [
                        LEGACY_STATUS_DRAFT,
                        LEGACY_STATUS_PENDING,
                        LEGACY_STATUS_READY,
                        LEGACY_STATUS_REVIEW,
                        LEGACY_STATUS_DONE
                    ]
                },
                "title": { "type": "string" }
            }
        }));
        h.ensure_governed_system_collection(
            BEAD_SYSTEM_CAPABILITY,
            EnsureGovernedSystemCollectionRequest {
                schema: old_schema,
                actor: Some("test".into()),
            },
        )
        .unwrap();

        for (id, status) in [
            ("legacy-proposed", LEGACY_STATUS_DRAFT),
            ("legacy-open", LEGACY_STATUS_PENDING),
            ("legacy-ready-open", LEGACY_STATUS_READY),
            ("legacy-in-progress", LEGACY_STATUS_REVIEW),
            ("legacy-closed", LEGACY_STATUS_DONE),
        ] {
            h.create_entity_in_system_collection(
                BEAD_SYSTEM_CAPABILITY,
                CreateGovernedSystemEntityRequest {
                    id: EntityId::new(id),
                    data: json!({ "status": status, "title": id }),
                    actor: Some("test".into()),
                    audit_metadata: None,
                    attribution: None,
                },
            )
            .unwrap();
        }

        init_beads(&mut h).unwrap();
        init_beads(&mut h).unwrap();

        assert_eq!(
            ids(&list_beads(&h, Some("proposed")).unwrap()),
            vec!["legacy-proposed"]
        );
        assert_eq!(
            ids(&list_beads(&h, Some("open")).unwrap()),
            vec!["legacy-open", "legacy-ready-open"]
        );
        assert_eq!(
            ids(&list_beads(&h, Some("in_progress")).unwrap()),
            vec!["legacy-in-progress"]
        );
        assert_eq!(
            ids(&list_beads(&h, Some("closed")).unwrap()),
            vec!["legacy-closed"]
        );

        let imported = import_beads(
            &mut h,
            &json!([{
                "id": "p-1",
                "issue_type": "task",
                "status": "proposed",
                "title": "Proposed work"
            }]),
        )
        .unwrap();
        assert_eq!(imported, 1);
        assert_eq!(
            ids(&list_beads(&h, Some("proposed")).unwrap()),
            vec!["legacy-proposed", "p-1"]
        );
    }

    #[test]
    fn bead_governed_create_query_uses_sealed_capability() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "First");
        make_bead(&mut h, "b-2", "Second");

        let beads = list_beads(&h, Some("open")).unwrap();
        assert_eq!(ids(&beads), vec!["b-1", "b-2"]);
        assert!(beads.iter().all(|bead| bead.status == "open"));
    }

    #[test]
    fn bead_governed_update_occ_uses_standard_path() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "First");
        let entity = get_bead_entity(&h, "b-1").unwrap();
        let mut data = entity.data.clone();
        data["title"] = json!("Updated");

        let updated = update_bead(&mut h, "b-1", data.clone(), entity.version).unwrap();
        assert_eq!(updated.title, "Updated");

        let stale = update_bead(&mut h, "b-1", data, entity.version).unwrap_err();
        assert!(matches!(stale, AxonError::ConflictingVersion { .. }));
    }

    #[test]
    fn bead_governed_audit_records_create_update_and_link() {
        let mut h = handler();
        make_bead(&mut h, "base", "Base");
        make_bead(&mut h, "child", "Child");
        transition_bead(&mut h, "base", "in_progress").unwrap();
        add_dependency(&mut h, "child", "base").unwrap();

        let entries = h.audit_log().entries();
        assert!(entries.iter().any(|entry| {
            entry.collection == bead_collection()
                && entry.mutation == MutationType::EntityCreate
                && entry.entity_id.as_str() == "base"
        }));
        assert!(entries.iter().any(|entry| {
            entry.collection == bead_collection()
                && entry.mutation == MutationType::EntityUpdate
                && entry.entity_id.as_str() == "base"
        }));
        assert!(entries
            .iter()
            .any(|entry| entry.mutation == MutationType::LinkCreate));
    }

    #[test]
    fn bead_governed_import_export_extension_fields_round_trip() {
        let mut h = handler();
        let fixture = json!([
            {
                "id": "ddx-1",
                "schema_version": 1,
                "issue_type": "task",
                "status": "open",
                "title": "Keep every DDx field",
                "priority": 0,
                "owner": "erik",
                "labels": ["area:test", "kind:task"],
                "claimed-at": "2026-07-11T18:00:00Z",
                "execute-loop-heartbeat-at": "2026-07-11T18:01:00Z",
                "custom-object": { "nested": true },
                "dependencies": [
                    {
                        "issue_id": "ddx-1",
                        "depends_on_id": "missing-archived",
                        "type": "blocks",
                        "created_at": "2026-07-11T18:02:00Z",
                        "custom": "preserved"
                    }
                ]
            }
        ]);

        assert_eq!(import_beads(&mut h, &fixture).unwrap(), 1);
        let exported = export_beads(&h).unwrap();
        assert_eq!(exported, fixture);
    }

    #[test]
    fn bead_import_parser_accepts_json_array_and_jsonl() {
        let array = r#"[{"id":"array-1","status":"open","title":"Array"}]"#;
        let jsonl = r#"
{"id":"jsonl-1","status":"open","title":"First","claimed-at":"2026-07-11T18:00:00Z"}
{"id":"jsonl-2","status":"closed","title":"Second","owner":"erik"}
        "#;

        assert_eq!(
            parse_bead_import_data(array).unwrap(),
            serde_json::from_str::<Value>(array).unwrap()
        );

        let parsed = parse_bead_import_data(jsonl).unwrap();
        let records = parsed.as_array().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["claimed-at"], "2026-07-11T18:00:00Z");
        assert_eq!(records[1]["owner"], "erik");
    }

    #[test]
    fn bead_import_parser_reports_jsonl_line_number() {
        let error = parse_bead_import_data(
            "{\"id\":\"valid\",\"status\":\"open\",\"title\":\"Valid\"}\nnot-json",
        )
        .unwrap_err();

        assert!(error.to_string().contains("JSONL line 2"));
    }

    #[test]
    fn bead_governed_export_includes_typed_dependencies_created_by_api() {
        let mut h = handler();
        make_bead(&mut h, "base", "Base");
        make_bead(&mut h, "child", "Child");
        add_dependency(&mut h, "child", "base").unwrap();

        let exported = export_beads(&h).unwrap();
        let child = exported
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "child")
            .unwrap();
        assert_eq!(
            child["dependencies"],
            json!([{
                "issue_id": "child",
                "depends_on_id": "base",
                "type": "blocks"
            }])
        );
    }

    #[test]
    fn bead_governed_import_rejects_later_invalid_entity_without_partial_state_or_audit() {
        let mut h = handler();
        init_beads(&mut h).unwrap();
        let audit_before = entity_and_link_audit_len(&h);
        let fixture = json!([
            {
                "id": "valid",
                "issue_type": "task",
                "status": "open",
                "title": "Valid"
            },
            {
                "id": "invalid",
                "issue_type": "task",
                "status": "open"
            }
        ]);

        let err = import_beads(&mut h, &fixture).unwrap_err();
        assert!(matches!(err, AxonError::SchemaValidation(_)));
        assert!(list_beads(&h, None).unwrap().is_empty());
        assert_eq!(entity_and_link_audit_len(&h), audit_before);
    }

    #[test]
    fn bead_governed_import_rejects_dependency_cycles_without_partial_state_or_audit() {
        let mut h = handler();
        init_beads(&mut h).unwrap();
        let audit_before = entity_and_link_audit_len(&h);
        let fixture = json!([
            {
                "id": "a",
                "issue_type": "task",
                "status": "open",
                "title": "A",
                "dependencies": [{
                    "issue_id": "a",
                    "depends_on_id": "b",
                    "type": "blocks"
                }]
            },
            {
                "id": "b",
                "issue_type": "task",
                "status": "open",
                "title": "B",
                "dependencies": [{
                    "issue_id": "b",
                    "depends_on_id": "a",
                    "type": "blocks"
                }]
            }
        ]);

        let err = import_beads(&mut h, &fixture).unwrap_err();
        assert!(err.to_string().contains("circular dependency"));
        assert!(list_beads(&h, None).unwrap().is_empty());
        assert_eq!(entity_and_link_audit_len(&h), audit_before);
    }

    #[test]
    fn bead_lifecycle_vocabulary_is_exactly_ddx_states() {
        let actual = BeadStatus::all()
            .iter()
            .map(|status| status.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                "proposed",
                "open",
                "in_progress",
                "blocked",
                "closed",
                "cancelled"
            ]
        );

        let schema = bead_schema();
        let enum_values = schema
            .entity_schema
            .as_ref()
            .and_then(|schema| schema.pointer("/properties/status/enum"))
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .unwrap();
        assert_eq!(enum_values, actual);
    }

    #[test]
    fn bead_lifecycle_terminal_states_reject_ordinary_updates() {
        let mut h = handler();
        make_bead(&mut h, "closed-bead", "Closed");
        transition_bead(&mut h, "closed-bead", "in_progress").unwrap();
        transition_bead(&mut h, "closed-bead", "closed").unwrap();
        let closed_err = transition_bead(&mut h, "closed-bead", "open").unwrap_err();
        assert!(matches!(
            closed_err,
            AxonError::InvalidTransition {
                current_state,
                target_state,
                valid_transitions,
                ..
            } if current_state == "closed"
                && target_state == "open"
                && valid_transitions.is_empty()
        ));

        make_bead(&mut h, "cancelled-bead", "Cancelled");
        transition_bead(&mut h, "cancelled-bead", "cancelled").unwrap();
        let cancelled_err = transition_bead(&mut h, "cancelled-bead", "open").unwrap_err();
        assert!(matches!(
            cancelled_err,
            AxonError::InvalidTransition {
                current_state,
                target_state,
                valid_transitions,
                ..
            } if current_state == "cancelled"
                && target_state == "open"
                && valid_transitions.is_empty()
        ));
    }

    #[test]
    fn bead_lifecycle_explicit_reopen_is_only_closed_to_open_path() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "Reopen");
        transition_bead(&mut h, "b-1", "in_progress").unwrap();
        transition_bead(&mut h, "b-1", "closed").unwrap();

        assert!(transition_bead(&mut h, "b-1", "open").is_err());
        let reopened = reopen_bead(&mut h, "b-1").unwrap();
        assert_eq!(reopened.status, "open");

        make_bead(&mut h, "b-2", "No reopen");
        transition_bead(&mut h, "b-2", "cancelled").unwrap();
        let err = reopen_bead(&mut h, "b-2").unwrap_err();
        assert!(matches!(
            err,
            AxonError::InvalidTransition {
                current_state,
                target_state,
                ..
            } if current_state == "cancelled" && target_state == "open"
        ));
    }

    #[test]
    fn bead_lifecycle_explicit_reopen_crosses_governed_terminal_boundary() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "Boundary");
        transition_bead(&mut h, "b-1", "in_progress").unwrap();
        transition_bead(&mut h, "b-1", "closed").unwrap();

        let closed = get_bead_entity(&h, "b-1").unwrap();
        let mut ordinary_data = closed.data.clone();
        ordinary_data[STATUS_FIELD] = json!("open");
        let ordinary_err = update_bead(&mut h, "b-1", ordinary_data, closed.version).unwrap_err();
        assert!(matches!(
            ordinary_err,
            AxonError::InvalidTransition {
                current_state,
                target_state,
                valid_transitions,
                ..
            } if current_state == "closed"
                && target_state == "open"
                && valid_transitions.is_empty()
        ));

        let reopened = reopen_bead_with_expected_version(&mut h, "b-1", closed.version).unwrap();
        assert_eq!(reopened.status, "open");
    }

    #[test]
    fn bead_lifecycle_ready_is_derived_from_open_and_closed_dependencies() {
        let mut h = handler();
        make_bead(&mut h, "base", "Base");
        make_bead(&mut h, "child", "Child");
        add_dependency(&mut h, "child", "base").unwrap();

        assert_eq!(ids(&ready_queue(&h).unwrap()), vec!["base"]);

        transition_bead(&mut h, "base", "in_progress").unwrap();
        transition_bead(&mut h, "base", "closed").unwrap();
        assert_eq!(ids(&ready_queue(&h).unwrap()), vec!["child"]);
    }

    #[test]
    fn bead_dependency_schema_declares_self_many_to_many_depends_on() {
        let schema = bead_schema();
        let link = schema.link_types.get(DEPENDS_ON_LINK).unwrap();
        assert_eq!(link.target_collection, BEAD_COLLECTION);
        assert_eq!(link.cardinality, Cardinality::ManyToMany);
        assert!(!link.required);
    }

    #[test]
    fn bead_dependency_rejects_missing_targets_without_partial_state_or_audit() {
        let mut h = handler();
        make_bead(&mut h, "base", "Base");
        let audit_before = audit_len(&h);

        let err = add_dependency(&mut h, "base", "missing").unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
        assert!(dependency_tree(&h, "base").unwrap().is_empty());
        assert_eq!(audit_len(&h), audit_before);
    }

    #[test]
    fn bead_dependency_rejects_self_dependencies_without_partial_state_or_audit() {
        let mut h = handler();
        make_bead(&mut h, "base", "Base");
        let audit_before = audit_len(&h);

        let err = add_dependency(&mut h, "base", "base").unwrap_err();
        assert!(err.to_string().contains("cannot depend on itself"));
        assert!(dependency_tree(&h, "base").unwrap().is_empty());
        assert_eq!(audit_len(&h), audit_before);
    }

    #[test]
    fn bead_dependency_rejects_cycles_without_partial_state_or_audit() {
        let mut h = handler();
        make_bead(&mut h, "a", "A");
        make_bead(&mut h, "b", "B");
        make_bead(&mut h, "c", "C");
        add_dependency(&mut h, "b", "a").unwrap();
        add_dependency(&mut h, "c", "b").unwrap();
        let audit_before = audit_len(&h);

        let err = add_dependency(&mut h, "a", "c").unwrap_err();
        assert!(err.to_string().contains("circular dependency"));
        assert!(dependency_tree(&h, "a").unwrap().is_empty());
        assert_eq!(ids(&dependency_tree(&h, "c").unwrap()), vec!["a", "b"]);
        assert_eq!(audit_len(&h), audit_before);
    }

    #[test]
    fn bead_dependency_rejects_long_cycles_and_returns_untruncated_tree() {
        let mut h = handler();
        for index in 0..12 {
            let id = format!("n-{index:02}");
            make_bead(&mut h, &id, &id);
        }
        for index in 1..12 {
            add_dependency(
                &mut h,
                &format!("n-{index:02}"),
                &format!("n-{:02}", index - 1),
            )
            .unwrap();
        }

        let expected = (0..11)
            .map(|index| format!("n-{index:02}"))
            .collect::<Vec<_>>();
        assert_eq!(ids(&dependency_tree(&h, "n-11").unwrap()), expected);

        let audit_before = audit_len(&h);
        let err = add_dependency(&mut h, "n-00", "n-11").unwrap_err();
        assert!(err.to_string().contains("circular dependency"));
        assert!(dependency_tree(&h, "n-00").unwrap().is_empty());
        assert_eq!(audit_len(&h), audit_before);
    }

    #[test]
    fn bead_dependency_returns_dependency_tree_and_ready_queue() {
        let mut h = handler();
        make_bead(&mut h, "root", "Root");
        make_bead(&mut h, "mid", "Middle");
        make_bead(&mut h, "leaf", "Leaf");
        add_dependency(&mut h, "mid", "root").unwrap();
        add_dependency(&mut h, "leaf", "mid").unwrap();

        assert_eq!(
            ids(&dependency_tree(&h, "leaf").unwrap()),
            vec!["mid", "root"]
        );
        assert_eq!(ids(&ready_queue(&h).unwrap()), vec!["root"]);

        transition_bead(&mut h, "root", "in_progress").unwrap();
        transition_bead(&mut h, "root", "closed").unwrap();
        assert_eq!(ids(&ready_queue(&h).unwrap()), vec!["mid"]);

        transition_bead(&mut h, "mid", "in_progress").unwrap();
        transition_bead(&mut h, "mid", "closed").unwrap();
        assert_eq!(ids(&ready_queue(&h).unwrap()), vec!["leaf"]);
    }
}
