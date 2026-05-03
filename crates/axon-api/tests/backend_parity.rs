#![allow(clippy::unwrap_used, clippy::wildcard_imports)]

//! Backend parity tests: verify AxonHandler behaves identically across
//! MemoryStorageAdapter and SqliteStorageAdapter.
//!
//! Each test function is generic over StorageAdapter and run once per backend.
//! This ensures embedded (SQLite) and in-memory modes produce identical results.

use axon_api::handler::AxonHandler;
use axon_api::request::*;
use axon_audit::AuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_schema::schema::CollectionSchema;
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;
use axon_storage::SqliteStorageAdapter;
use serde_json::json;

fn col(name: &str) -> CollectionId {
    CollectionId::new(name)
}

fn eid(id: &str) -> EntityId {
    EntityId::new(id)
}

fn make_schema(name: &str) -> CollectionSchema {
    CollectionSchema {
        collection: col(name),
        description: None,
        version: 1,
        entity_schema: None,
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

// ── Test implementations (generic) ──────────────────────────────────────────

fn test_entity_crud<S: StorageAdapter>(mut h: AxonHandler<S>) {
    h.create_collection(CreateCollectionRequest {
        name: col("tasks"),
        schema: make_schema("tasks"),
        actor: None,
    })
    .unwrap();

    // Create
    let resp = h
        .create_entity(CreateEntityRequest {
            collection: col("tasks"),
            id: eid("t-001"),
            data: json!({"title": "hello"}),
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    assert_eq!(resp.entity.version, 1);

    // Get
    let resp = h
        .get_entity(GetEntityRequest {
            collection: col("tasks"),
            id: eid("t-001"),
        })
        .unwrap();
    assert_eq!(resp.entity.data["title"], "hello");

    // Update with OCC
    let resp = h
        .update_entity(UpdateEntityRequest {
            collection: col("tasks"),
            id: eid("t-001"),
            data: json!({"title": "updated"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    assert_eq!(resp.entity.version, 2);
    assert_eq!(resp.entity.data["title"], "updated");

    // Version conflict
    let err = h
        .update_entity(UpdateEntityRequest {
            collection: col("tasks"),
            id: eid("t-001"),
            data: json!({"title": "stale"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap_err();
    assert!(matches!(err, AxonError::ConflictingVersion { .. }));

    // Delete
    h.delete_entity(DeleteEntityRequest {
        collection: col("tasks"),
        id: eid("t-001"),
        actor: None,
        audit_metadata: None,
        force: false,
        attribution: None,
    })
    .unwrap();

    let err = h
        .get_entity(GetEntityRequest {
            collection: col("tasks"),
            id: eid("t-001"),
        })
        .unwrap_err();
    assert!(matches!(err, AxonError::NotFound(_)));
}

fn test_collection_lifecycle<S: StorageAdapter>(mut h: AxonHandler<S>) {
    h.create_collection(CreateCollectionRequest {
        name: col("items"),
        schema: make_schema("items"),
        actor: None,
    })
    .unwrap();

    let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
    assert_eq!(resp.collections.len(), 1);
    assert_eq!(resp.collections[0].name, "items");

    let resp = h
        .describe_collection(DescribeCollectionRequest { name: col("items") })
        .unwrap();
    assert_eq!(resp.entity_count, 0);

    h.drop_collection(DropCollectionRequest {
        name: col("items"),
        actor: None,
        confirm: true,
    })
    .unwrap();

    let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
    assert!(resp.collections.is_empty());
}

fn test_links_and_traversal<S: StorageAdapter>(mut h: AxonHandler<S>) {
    h.create_collection(CreateCollectionRequest {
        name: col("nodes"),
        schema: make_schema("nodes"),
        actor: None,
    })
    .unwrap();

    for name in ["a", "b", "c"] {
        h.create_entity(CreateEntityRequest {
            collection: col("nodes"),
            id: eid(name),
            data: json!({"name": name}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    for (src, tgt) in [("a", "b"), ("b", "c")] {
        h.create_link(CreateLinkRequest {
            source_collection: col("nodes"),
            source_id: eid(src),
            target_collection: col("nodes"),
            target_id: eid(tgt),
            link_type: "next".into(),
            metadata: json!(null),
            actor: None,
            attribution: None,
        })
        .unwrap();
    }

    // Forward traversal
    let resp = h
        .traverse(TraverseRequest {
            collection: col("nodes"),
            id: eid("a"),
            link_type: Some("next".into()),
            max_depth: Some(3),
            direction: TraverseDirection::Forward,
            hop_filter: None,
        })
        .unwrap();
    assert_eq!(resp.entities.len(), 2);

    // Reverse traversal
    let resp = h
        .traverse(TraverseRequest {
            collection: col("nodes"),
            id: eid("c"),
            link_type: Some("next".into()),
            max_depth: Some(3),
            direction: TraverseDirection::Reverse,
            hop_filter: None,
        })
        .unwrap();
    assert_eq!(resp.entities.len(), 2);

    // Reachability
    let resp = h
        .reachable(ReachableRequest {
            source_collection: col("nodes"),
            source_id: eid("a"),
            target_collection: col("nodes"),
            target_id: eid("c"),
            link_type: Some("next".into()),
            max_depth: Some(5),
            direction: TraverseDirection::Forward,
        })
        .unwrap();
    assert!(resp.reachable);
    assert_eq!(resp.depth, Some(2));
}

fn test_query_entities<S: StorageAdapter>(mut h: AxonHandler<S>) {
    h.create_collection(CreateCollectionRequest {
        name: col("tasks"),
        schema: make_schema("tasks"),
        actor: None,
    })
    .unwrap();

    for (id, status) in [("t-1", "open"), ("t-2", "done"), ("t-3", "open")] {
        h.create_entity(CreateEntityRequest {
            collection: col("tasks"),
            id: eid(id),
            data: json!({"status": status}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    let resp = h
        .query_entities(QueryEntitiesRequest {
            collection: col("tasks"),
            filter: Some(FilterNode::Field(FieldFilter {
                field: "status".into(),
                op: FilterOp::Eq,
                value: json!("open"),
            })),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(resp.total_count, 2);
    assert_eq!(resp.entities.len(), 2);
}

fn test_schema_enforcement<S: StorageAdapter>(mut h: AxonHandler<S>) {
    let schema = CollectionSchema {
        collection: col("typed"),
        description: None,
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["amount"],
            "properties": {
                "amount": {"type": "number"}
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
    };

    h.create_collection(CreateCollectionRequest {
        name: col("typed"),
        schema,
        actor: None,
    })
    .unwrap();

    // Valid entity
    h.create_entity(CreateEntityRequest {
        collection: col("typed"),
        id: eid("ok"),
        data: json!({"amount": 42}),
        actor: None,
        audit_metadata: None,
        attribution: None,
    })
    .unwrap();

    // Invalid entity — schema violation
    let err = h
        .create_entity(CreateEntityRequest {
            collection: col("typed"),
            id: eid("bad"),
            data: json!({"name": "missing amount"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap_err();
    assert!(matches!(err, AxonError::SchemaValidation(_)));
}

fn test_audit_log<S: StorageAdapter>(mut h: AxonHandler<S>) {
    h.create_collection(CreateCollectionRequest {
        name: col("tasks"),
        schema: make_schema("tasks"),
        actor: None,
    })
    .unwrap();

    h.create_entity(CreateEntityRequest {
        collection: col("tasks"),
        id: eid("t-001"),
        data: json!({"title": "v1"}),
        actor: Some("alice".into()),
        audit_metadata: None,
        attribution: None,
    })
    .unwrap();

    h.update_entity(UpdateEntityRequest {
        collection: col("tasks"),
        id: eid("t-001"),
        data: json!({"title": "v2"}),
        expected_version: 1,
        actor: Some("bob".into()),
        audit_metadata: None,
        attribution: None,
    })
    .unwrap();

    let entries = h
        .audit_log()
        .query_by_entity(&col("tasks"), &eid("t-001"))
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].actor, "alice");
    assert_eq!(entries[1].actor, "bob");
}

// ── Memory backend ──────────────────────────────────────────────────────────

mod memory {
    use super::*;

    fn handler() -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new(MemoryStorageAdapter::default())
    }

    #[test]
    fn entity_crud() {
        test_entity_crud(handler());
    }
    #[test]
    fn collection_lifecycle() {
        test_collection_lifecycle(handler());
    }
    #[test]
    fn links_and_traversal() {
        test_links_and_traversal(handler());
    }
    #[test]
    fn query_entities() {
        test_query_entities(handler());
    }
    #[test]
    fn schema_enforcement() {
        test_schema_enforcement(handler());
    }
    #[test]
    fn audit_log() {
        test_audit_log(handler());
    }
}

// ── SQLite backend ──────────────────────────────────────────────────────────

mod sqlite {
    use super::*;

    fn handler() -> AxonHandler<SqliteStorageAdapter> {
        AxonHandler::new(SqliteStorageAdapter::open(":memory:").unwrap())
    }

    #[test]
    fn entity_crud() {
        test_entity_crud(handler());
    }
    #[test]
    fn collection_lifecycle() {
        test_collection_lifecycle(handler());
    }
    #[test]
    fn links_and_traversal() {
        test_links_and_traversal(handler());
    }
    #[test]
    fn query_entities() {
        test_query_entities(handler());
    }
    #[test]
    fn schema_enforcement() {
        test_schema_enforcement(handler());
    }
    #[test]
    fn audit_log() {
        test_audit_log(handler());
    }
}
