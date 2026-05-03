//! Bead storage adapter: opinionated issue/work-item layer built on Axon primitives.
//!
//! Provides a pre-defined schema, lifecycle state machine, dependency tracking,
//! and ready-queue computation for bead-like work items (FEAT-006).

use serde::{Deserialize, Serialize};
use serde_json::json;

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_schema::schema::CollectionSchema;
use axon_storage::adapter::StorageAdapter;

use crate::handler::AxonHandler;
use crate::request::{
    CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest, QueryEntitiesRequest,
    TraverseDirection, TraverseRequest, UpdateEntityRequest,
};

/// The collection name for beads.
pub const BEAD_COLLECTION: &str = "__axon_beads__";

/// Link type used for bead dependencies (source depends-on target).
pub const DEPENDS_ON_LINK: &str = "depends-on";

/// Valid bead lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeadStatus {
    Draft,
    Pending,
    Ready,
    InProgress,
    Review,
    Done,
    Blocked,
    Cancelled,
}

impl BeadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Review => "review",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "pending" => Some(Self::Pending),
            "ready" => Some(Self::Ready),
            "in_progress" => Some(Self::InProgress),
            "review" => Some(Self::Review),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// Returns the set of valid transitions from this state.
    fn valid_transitions(&self) -> &[BeadStatus] {
        match self {
            Self::Draft => &[Self::Pending, Self::Cancelled],
            Self::Pending => &[
                Self::Ready,
                Self::InProgress,
                Self::Blocked,
                Self::Cancelled,
            ],
            Self::Ready => &[Self::InProgress, Self::Blocked, Self::Cancelled],
            Self::InProgress => &[Self::Review, Self::Done, Self::Blocked, Self::Cancelled],
            Self::Review => &[Self::InProgress, Self::Done, Self::Cancelled],
            Self::Done => &[],
            Self::Blocked => &[Self::Pending, Self::Cancelled],
            Self::Cancelled => &[],
        }
    }

    /// Check whether transitioning to `target` is valid from this state.
    pub fn can_transition_to(&self, target: &BeadStatus) -> bool {
        self.valid_transitions().contains(target)
    }
}

/// A bead (work item) with its full data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bead {
    /// Set from the entity ID, not stored in entity data.
    #[serde(default)]
    pub id: String,
    pub bead_type: String,
    pub status: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
}

/// Returns the bead collection ID.
pub fn bead_collection() -> CollectionId {
    CollectionId::new(BEAD_COLLECTION)
}

/// Returns the built-in bead collection schema.
pub fn bead_schema() -> CollectionSchema {
    CollectionSchema {
        collection: bead_collection(),
        description: Some("Bead work-item collection (FEAT-006)".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["bead_type", "status", "title"],
            "properties": {
                "bead_type": { "type": "string" },
                "status": {
                    "type": "string",
                    "enum": ["draft", "pending", "ready", "in_progress", "review", "done", "blocked", "cancelled"]
                },
                "title": { "type": "string", "minLength": 1 },
                "description": { "type": "string" },
                "priority": { "type": "integer", "minimum": 0 },
                "assignee": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "acceptance": { "type": "string" }
            }
        })),
        link_types: Default::default(),
        access_control: None,
        gates: Default::default(),
        validation_rules: Default::default(),
        indexes: Default::default(),
        compound_indexes: Default::default(),
        queries: Default::default(),
        lifecycles: Default::default(),
    }
}

/// Initialize the bead collection if it doesn't exist.
pub fn init_beads<S: StorageAdapter>(handler: &mut AxonHandler<S>) -> Result<(), AxonError> {
    // Check if collection already exists.
    let existing = handler
        .list_collections(crate::request::ListCollectionsRequest {})?
        .collections
        .iter()
        .any(|c| c.name == BEAD_COLLECTION);

    if existing {
        return Ok(());
    }

    handler.create_collection(CreateCollectionRequest {
        name: bead_collection(),
        schema: bead_schema(),
        actor: Some("system".into()),
    })?;
    Ok(())
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

/// Create a new bead.
pub fn create_bead<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    params: CreateBeadParams<'_>,
) -> Result<Bead, AxonError> {
    init_beads(handler)?;

    let mut data = json!({
        "bead_type": params.bead_type,
        "status": "draft",
        "title": params.title,
        "priority": params.priority,
        "tags": params.tags,
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

    handler.create_entity(CreateEntityRequest {
        collection: bead_collection(),
        id: EntityId::new(params.id),
        data: data.clone(),
        actor: Some("bead-system".into()),
        audit_metadata: None,
        attribution: None,
    })?;

    let mut bead: Bead = serde_json::from_value(data)
        .map_err(|e| AxonError::Storage(format!("bead serialization: {e}")))?;
    bead.id = params.id.to_string();
    Ok(bead)
}

/// Transition a bead's status, enforcing the lifecycle state machine.
pub fn transition_bead<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    id: &str,
    new_status: &str,
) -> Result<(), AxonError> {
    let col = bead_collection();
    let eid = EntityId::new(id);

    let entity = handler
        .storage_ref()
        .get(&col, &eid)?
        .ok_or_else(|| AxonError::NotFound(format!("bead {id}")))?;

    let current_status_str = entity.data["status"]
        .as_str()
        .ok_or_else(|| AxonError::InvalidOperation("bead has no status field".into()))?;

    let current = BeadStatus::parse(current_status_str).ok_or_else(|| {
        AxonError::InvalidOperation(format!("unknown bead status: {current_status_str}"))
    })?;

    let target = BeadStatus::parse(new_status).ok_or_else(|| {
        AxonError::InvalidOperation(format!("unknown target status: {new_status}"))
    })?;

    if !current.can_transition_to(&target) {
        return Err(AxonError::InvalidOperation(format!(
            "invalid transition: {} -> {} (allowed: {:?})",
            current_status_str,
            new_status,
            current
                .valid_transitions()
                .iter()
                .map(BeadStatus::as_str)
                .collect::<Vec<_>>()
        )));
    }

    let mut new_data = entity.data.clone();
    new_data["status"] = json!(new_status);

    handler.update_entity(UpdateEntityRequest {
        collection: col,
        id: eid,
        data: new_data,
        expected_version: entity.version,
        actor: Some("bead-system".into()),
        audit_metadata: None,
        attribution: None,
    })?;

    Ok(())
}

/// Add a dependency: `bead_id` depends-on `dep_id`.
///
/// Rejects circular dependencies by checking reachability in the reverse direction.
pub fn add_dependency<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    bead_id: &str,
    dep_id: &str,
) -> Result<(), AxonError> {
    let col = bead_collection();

    // Verify both beads exist.
    handler
        .storage_ref()
        .get(&col, &EntityId::new(bead_id))?
        .ok_or_else(|| AxonError::NotFound(format!("bead {bead_id}")))?;
    handler
        .storage_ref()
        .get(&col, &EntityId::new(dep_id))?
        .ok_or_else(|| AxonError::NotFound(format!("bead {dep_id}")))?;

    // Check for cycles: if dep_id can already reach bead_id, adding this edge creates a cycle.
    let reachable = handler.reachable(crate::request::ReachableRequest {
        source_collection: col.clone(),
        source_id: EntityId::new(dep_id),
        target_collection: col.clone(),
        target_id: EntityId::new(bead_id),
        link_type: Some(DEPENDS_ON_LINK.into()),
        max_depth: Some(10),
        direction: TraverseDirection::Forward,
    })?;

    if reachable.reachable {
        return Err(AxonError::InvalidOperation(format!(
            "circular dependency: {dep_id} already reaches {bead_id}"
        )));
    }

    handler.create_link(CreateLinkRequest {
        source_collection: col.clone(),
        source_id: EntityId::new(bead_id),
        target_collection: col,
        target_id: EntityId::new(dep_id),
        link_type: DEPENDS_ON_LINK.into(),
        metadata: json!(null),
        actor: Some("bead-system".into()),
        attribution: None,
    })?;

    Ok(())
}

/// List beads, optionally filtered by status.
pub fn list_beads<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    status_filter: Option<&str>,
) -> Result<Vec<Bead>, AxonError> {
    let resp = handler.query_entities(QueryEntitiesRequest {
        collection: bead_collection(),
        limit: Some(1000),
        ..Default::default()
    })?;

    let mut beads: Vec<Bead> = Vec::new();
    for entity in &resp.entities {
        if let Some(filter) = status_filter {
            if entity.data["status"].as_str() != Some(filter) {
                continue;
            }
        }
        let mut bead: Bead = serde_json::from_value(entity.data.clone())
            .map_err(|e| AxonError::Storage(format!("bead deserialization: {e}")))?;
        bead.id = entity.id.to_string();
        beads.push(bead);
    }
    Ok(beads)
}

/// Compute the ready queue: beads in `pending` status where all dependencies are `done`.
pub fn ready_queue<S: StorageAdapter>(handler: &AxonHandler<S>) -> Result<Vec<Bead>, AxonError> {
    let pending = list_beads(handler, Some("pending"))?;
    let col = bead_collection();

    let mut ready = Vec::new();
    for bead in pending {
        // Find all dependencies (outbound depends-on links).
        let deps = handler.traverse(TraverseRequest {
            collection: col.clone(),
            id: EntityId::new(&bead.id),
            link_type: Some(DEPENDS_ON_LINK.into()),
            max_depth: Some(1),
            direction: TraverseDirection::Forward,
            hop_filter: None,
        })?;

        let all_deps_done = deps
            .entities
            .iter()
            .all(|dep_entity| dep_entity.data["status"].as_str() == Some("done"));

        if all_deps_done {
            ready.push(bead);
        }
    }
    Ok(ready)
}

/// Get the dependency tree for a bead (all transitive dependencies).
pub fn dependency_tree<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    bead_id: &str,
) -> Result<Vec<Bead>, AxonError> {
    let col = bead_collection();
    let resp = handler.traverse(TraverseRequest {
        collection: col,
        id: EntityId::new(bead_id),
        link_type: Some(DEPENDS_ON_LINK.into()),
        max_depth: Some(10),
        direction: TraverseDirection::Forward,
        hop_filter: None,
    })?;

    let mut beads = Vec::new();
    for entity in &resp.entities {
        let mut bead: Bead = serde_json::from_value(entity.data.clone())
            .map_err(|e| AxonError::Storage(format!("bead deserialization: {e}")))?;
        bead.id = entity.id.to_string();
        beads.push(bead);
    }
    Ok(beads)
}

/// Export all beads as a JSON array of Bead structs.
pub fn export_beads<S: StorageAdapter>(
    handler: &AxonHandler<S>,
) -> Result<serde_json::Value, AxonError> {
    let beads = list_beads(handler, None)?;
    serde_json::to_value(&beads).map_err(|e| AxonError::Storage(format!("export: {e}")))
}

/// Import beads from a JSON array. Each element must have at least `id`, `bead_type`,
/// `status`, and `title`. Existing beads with the same ID are skipped.
pub fn import_beads<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    data: &serde_json::Value,
) -> Result<usize, AxonError> {
    init_beads(handler)?;

    let arr = data
        .as_array()
        .ok_or_else(|| AxonError::InvalidArgument("expected JSON array".into()))?;

    let col = bead_collection();
    let mut imported = 0;

    for item in arr {
        let bead: Bead = serde_json::from_value(item.clone())
            .map_err(|e| AxonError::InvalidArgument(format!("invalid bead: {e}")))?;

        if bead.id.is_empty() {
            return Err(AxonError::InvalidArgument("bead missing id".into()));
        }

        // Skip if already exists.
        if handler
            .storage_ref()
            .get(&col, &EntityId::new(&bead.id))?
            .is_some()
        {
            continue;
        }

        // Build entity data without the id field.
        let mut entity_data = serde_json::to_value(&bead)
            .map_err(|e| AxonError::Storage(format!("bead serialization: {e}")))?;
        if let Some(obj) = entity_data.as_object_mut() {
            obj.remove("id");
        }

        handler.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new(&bead.id),
            data: entity_data,
            actor: Some("bead-import".into()),
            audit_metadata: None,
            attribution: None,
        })?;
        imported += 1;
    }

    Ok(imported)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axon_storage::memory::MemoryStorageAdapter;

    fn handler() -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new(MemoryStorageAdapter::default())
    }

    fn make_bead(h: &mut AxonHandler<MemoryStorageAdapter>, id: &str, title: &str) {
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
        .unwrap();
    }

    #[test]
    fn init_creates_bead_collection() {
        let mut h = handler();
        init_beads(&mut h).unwrap();

        let cols = h
            .list_collections(crate::request::ListCollectionsRequest {})
            .unwrap();
        assert!(cols.collections.iter().any(|c| c.name == BEAD_COLLECTION));
    }

    #[test]
    fn init_is_idempotent() {
        let mut h = handler();
        init_beads(&mut h).unwrap();
        init_beads(&mut h).unwrap(); // second call should not fail
    }

    #[test]
    fn create_and_list_beads() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "First task");
        make_bead(&mut h, "b-2", "Second task");

        let beads = list_beads(&h, None).unwrap();
        assert_eq!(beads.len(), 2);
    }

    #[test]
    fn lifecycle_valid_transition() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "Test");

        // draft -> pending
        transition_bead(&mut h, "b-1", "pending").unwrap();
        // pending -> in_progress
        transition_bead(&mut h, "b-1", "in_progress").unwrap();
        // in_progress -> done
        transition_bead(&mut h, "b-1", "done").unwrap();
    }

    #[test]
    fn lifecycle_invalid_transition_rejected() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "Test");

        // draft -> done should fail
        let err = transition_bead(&mut h, "b-1", "done").unwrap_err();
        assert!(err.to_string().contains("invalid transition"));
    }

    #[test]
    fn dependency_tracking() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "Base");
        make_bead(&mut h, "b-2", "Depends on b-1");

        add_dependency(&mut h, "b-2", "b-1").unwrap();

        let deps = dependency_tree(&h, "b-2").unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id, "b-1");
    }

    #[test]
    fn circular_dependency_rejected() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "A");
        make_bead(&mut h, "b-2", "B");

        add_dependency(&mut h, "b-2", "b-1").unwrap();
        let err = add_dependency(&mut h, "b-1", "b-2").unwrap_err();
        assert!(err.to_string().contains("circular dependency"));
    }

    #[test]
    fn ready_queue_requires_deps_done() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "Base");
        make_bead(&mut h, "b-2", "Depends on b-1");

        // b-2 depends on b-1.
        add_dependency(&mut h, "b-2", "b-1").unwrap();

        // Move both to pending.
        transition_bead(&mut h, "b-1", "pending").unwrap();
        transition_bead(&mut h, "b-2", "pending").unwrap();

        // b-1 has no deps, so it should be ready. b-2 has unfinished dep.
        let ready = ready_queue(&h).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "b-1");

        // Complete b-1: pending -> in_progress -> done.
        transition_bead(&mut h, "b-1", "in_progress").unwrap();
        transition_bead(&mut h, "b-1", "done").unwrap();

        // Now b-2 should be ready.
        let ready = ready_queue(&h).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "b-2");
    }

    #[test]
    fn list_beads_with_status_filter() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "Draft");
        make_bead(&mut h, "b-2", "Pending");
        transition_bead(&mut h, "b-2", "pending").unwrap();

        let drafts = list_beads(&h, Some("draft")).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, "b-1");

        let pendings = list_beads(&h, Some("pending")).unwrap();
        assert_eq!(pendings.len(), 1);
        assert_eq!(pendings[0].id, "b-2");
    }

    #[test]
    fn export_import_round_trip() {
        let mut h = handler();
        make_bead(&mut h, "b-1", "First");
        make_bead(&mut h, "b-2", "Second");

        let exported = export_beads(&h).unwrap();
        let arr = exported.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        // Import into a fresh handler.
        let mut h2 = handler();
        let count = import_beads(&mut h2, &exported).unwrap();
        assert_eq!(count, 2);

        let beads = list_beads(&h2, None).unwrap();
        assert_eq!(beads.len(), 2);

        // Import again — should skip existing.
        let count = import_beads(&mut h2, &exported).unwrap();
        assert_eq!(count, 0);
    }
}
