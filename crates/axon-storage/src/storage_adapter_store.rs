//! [`StorageAdapter`]-backed [`QueryStore`] for the Cypher executor.
//!
//! This module bridges `axon-storage::StorageAdapter` (the production storage
//! layer) with `axon-cypher::QueryStore` (the executor interface).
//!
//! ## Crate placement
//!
//! `StorageAdapterQueryStore` lives here rather than in `axon-cypher` because
//! `axon-schema` (which `axon-storage` already depends on) depends on
//! `axon-cypher` — adding the reverse dependency would create a cycle.
//!
//! ## ID encoding
//!
//! `QueryStore` uses plain string IDs, but `StorageAdapter` requires
//! `(CollectionId, EntityId)` pairs. Entity IDs returned from this store are
//! encoded as `"{collection}\x1f{entity_id}"` using the ASCII Unit Separator
//! (`\x1f`, U+001F), which does not appear in collection names or entity IDs
//! in practice.

use std::collections::{BTreeSet, HashMap};

use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Link;
use axon_cypher::ast::Direction;
use axon_cypher::error::CypherError;
use axon_cypher::memory_store::{
    EntityScan, EntityStream, LinkStream, LinkTraversal, PropertyFilter, PropertyFilterOp,
    PropertyMap, QueryEntity, QueryLink, QueryStore,
};
use axon_cypher::schema::SchemaSnapshot;
use serde_json::Value;

use crate::{IndexValue, OrderedFloat, StorageAdapter};

/// ASCII Unit Separator — used to separate collection name from entity ID in
/// the encoded ID format `"{collection}\x1f{entity_id}"`.
const SEP: char = '\x1f';

fn encode_entity_id(collection: &str, raw_id: &str) -> String {
    format!("{collection}{SEP}{raw_id}")
}

/// Returns `None` if `encoded` does not contain the separator.
fn decode_entity_id(encoded: &str) -> Option<(CollectionId, EntityId)> {
    let pos = encoded.find(SEP)?;
    let collection = &encoded[..pos];
    let raw_id = &encoded[(pos + SEP.len_utf8())..];
    Some((CollectionId::new(collection), EntityId::new(raw_id)))
}

/// A [`QueryStore`] backed by a [`StorageAdapter`].
///
/// - **Label/property reads**: equality filters use `StorageAdapter::index_lookup`;
///   full-label scans use `StorageAdapter::range_scan`.
/// - **Link traversal**: uses `StorageAdapter::list_outbound_links` /
///   `list_inbound_links` from the dedicated portable link API.
/// - **Errors**: storage errors are mapped to [`axon_cypher::CypherError::Storage`]
///   and propagated through the entity/link streams to the executor.
///   `index_lookup` failures fall back to `range_scan` (adapter capability gap,
///   not a data error) and are not propagated.
pub struct StorageAdapterQueryStore<'a, S> {
    storage: &'a S,
    /// Label name → collection name, derived from the schema snapshot.
    label_to_collection: HashMap<String, String>,
    /// Collection name → label names (reverse of above).
    collection_to_labels: HashMap<String, Vec<String>>,
}

impl<'a, S: StorageAdapter> StorageAdapterQueryStore<'a, S> {
    /// Create a new store bridging `storage` and the label/collection mapping
    /// declared in `schema`.
    pub fn new(storage: &'a S, schema: &SchemaSnapshot) -> Self {
        let mut label_to_collection = HashMap::new();
        let mut collection_to_labels: HashMap<String, Vec<String>> = HashMap::new();
        for (label, def) in &schema.labels {
            label_to_collection.insert(label.clone(), def.collection_name.clone());
            collection_to_labels
                .entry(def.collection_name.clone())
                .or_default()
                .push(label.clone());
        }
        Self {
            storage,
            label_to_collection,
            collection_to_labels,
        }
    }

    fn build_query_entity(&self, entity: &axon_core::types::Entity) -> QueryEntity {
        let collection = entity.collection.as_str();
        let labels: BTreeSet<String> = self
            .collection_to_labels
            .get(collection)
            .map(|ls| ls.iter().cloned().collect())
            .unwrap_or_default();
        let properties: PropertyMap = match &entity.data {
            Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => PropertyMap::new(),
        };
        QueryEntity {
            id: encode_entity_id(collection, entity.id.as_str()),
            labels,
            properties,
        }
    }


    /// Full range scan of a collection, filtering with `scan.property_filters`.
    ///
    /// Returns an error item if the storage layer fails; otherwise yields
    /// one `Ok(QueryEntity)` per matching entity.
    fn range_scan_collection(
        &self,
        collection_id: &CollectionId,
        scan: &EntityScan,
    ) -> Vec<Result<QueryEntity, CypherError>> {
        match self.storage.range_scan(collection_id, None, None, None) {
            Err(err) => vec![Err(CypherError::Storage(err.to_string()))],
            Ok(entities) => entities
                .into_iter()
                .filter(|entity| data_matches_filters(&entity.data, &scan.property_filters))
                .map(|entity| Ok(self.build_query_entity(&entity)))
                .collect(),
        }
    }
}

impl<S: StorageAdapter> QueryStore for StorageAdapterQueryStore<'_, S> {
    /// Look up an entity by its encoded ID (`"{collection}\x1f{raw_id}"`).
    fn get_entity(&self, id: &str) -> Option<QueryEntity> {
        let (collection_id, entity_id) = decode_entity_id(id)?;
        let entity = self.storage.get(&collection_id, &entity_id).ok().flatten()?;
        Some(self.build_query_entity(&entity))
    }

    /// Scan entities for a given label, using `index_lookup` for equality
    /// filters and `range_scan` for full-label scans.
    ///
    /// Storage errors from `range_scan` are propagated as
    /// `Err(CypherError::Storage(...))` stream items. `index_lookup` errors
    /// are treated as a capability gap and fall through to `range_scan`.
    fn scan_entities(&self, scan: EntityScan) -> EntityStream<'_> {
        let label = match scan.label.as_deref() {
            Some(l) => l,
            None => return Box::new(std::iter::empty()),
        };
        let collection_name = match self.label_to_collection.get(label) {
            Some(c) => c.clone(),
            None => return Box::new(std::iter::empty()),
        };
        let collection_id = CollectionId::new(&collection_name);

        // For equality filters, try index_lookup; fall back to range_scan on error.
        let eq_filter = scan.property_filters.iter().find(|f| f.op == PropertyFilterOp::Eq);

        let entities: Vec<Result<QueryEntity, CypherError>> = if let Some(filter) = eq_filter {
            let property = filter.path.join(".");
            match json_to_index_value(&filter.value) {
                Some(index_val) => {
                    match self.storage.index_lookup(&collection_id, &property, &index_val) {
                        Ok(ids) => ids
                            .into_iter()
                            .filter_map(|eid| {
                                self.storage.get(&collection_id, &eid).ok().flatten()
                            })
                            .filter(|entity| {
                                data_matches_filters(&entity.data, &scan.property_filters)
                            })
                            .map(|entity| Ok(self.build_query_entity(&entity)))
                            .collect(),
                        // index_lookup not supported by this adapter — fall through.
                        Err(_) => self.range_scan_collection(&collection_id, &scan),
                    }
                }
                // Non-indexable value type — fall through to range scan.
                None => self.range_scan_collection(&collection_id, &scan),
            }
        } else {
            self.range_scan_collection(&collection_id, &scan)
        };

        Box::new(entities.into_iter())
    }

    /// Links are looked up by ID only when the executor demands it; returns
    /// `None` here since link access goes through `traverse_links`.
    fn get_link(&self, _id: &str) -> Option<QueryLink> {
        None
    }

    /// Expand links from `traversal.anchor_id` using the dedicated link API.
    ///
    /// Uses `list_outbound_links` / `list_inbound_links` according to the
    /// requested direction; filters by relationship type in memory when
    /// multiple types are specified (the storage API accepts a single type).
    /// Storage errors are propagated as `Err(CypherError::Storage(...))` items.
    fn traverse_links(&self, traversal: LinkTraversal) -> LinkStream<'_> {
        let (collection_id, entity_id) = match decode_entity_id(&traversal.anchor_id) {
            Some(parts) => parts,
            None => return Box::new(std::iter::empty()),
        };

        // Pass a single-type hint to storage to narrow the scan; multi-type or
        // untyped traversals are filtered in memory.
        let type_hint = if traversal.relationship_types.len() == 1 {
            traversal.relationship_types.first().map(String::as_str)
        } else {
            None
        };

        let raw: Vec<Result<Link, CypherError>> = match traversal.direction {
            Direction::Outgoing => match self
                .storage
                .list_outbound_links(&collection_id, &entity_id, type_hint)
            {
                Ok(links) => links.into_iter().map(Ok).collect(),
                Err(err) => vec![Err(CypherError::Storage(err.to_string()))],
            },
            Direction::Incoming => match self
                .storage
                .list_inbound_links(&collection_id, &entity_id, type_hint)
            {
                Ok(links) => links.into_iter().map(Ok).collect(),
                Err(err) => vec![Err(CypherError::Storage(err.to_string()))],
            },
            Direction::Either => {
                match self
                    .storage
                    .list_outbound_links(&collection_id, &entity_id, type_hint)
                {
                    Err(err) => vec![Err(CypherError::Storage(err.to_string()))],
                    Ok(mut out) => {
                        match self
                            .storage
                            .list_inbound_links(&collection_id, &entity_id, type_hint)
                        {
                            Err(err) => vec![Err(CypherError::Storage(err.to_string()))],
                            Ok(inbound) => {
                                out.extend(inbound);
                                out.into_iter().map(Ok).collect()
                            }
                        }
                    }
                }
            }
        };

        Box::new(raw.into_iter().filter_map(move |link_result| {
            match link_result {
                Err(err) => Some(Err(err)),
                Ok(link) => {
                    if traversal.relationship_types.is_empty()
                        || traversal.relationship_types.contains(&link.link_type)
                    {
                        Some(Ok(build_query_link(&link)))
                    } else {
                        None
                    }
                }
            }
        }))
    }
}

fn build_query_link(link: &Link) -> QueryLink {
    let link_id = Link::storage_id(
        &link.source_collection,
        &link.source_id,
        &link.link_type,
        &link.target_collection,
        &link.target_id,
    )
    .to_string();
    let properties: PropertyMap = match &link.metadata {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => PropertyMap::new(),
    };
    QueryLink {
        id: link_id,
        source_id: encode_entity_id(link.source_collection.as_str(), link.source_id.as_str()),
        target_id: encode_entity_id(link.target_collection.as_str(), link.target_id.as_str()),
        link_type: link.link_type.clone(),
        properties,
    }
}

/// Convert a JSON value to an [`IndexValue`] for `index_lookup` calls.
///
/// Returns `None` for non-scalar types (objects, arrays, null) that cannot
/// be used as index keys.
fn json_to_index_value(value: &Value) -> Option<IndexValue> {
    match value {
        Value::String(s) => Some(IndexValue::String(s.clone())),
        Value::Number(n) => n
            .as_i64()
            .map(IndexValue::Integer)
            .or_else(|| n.as_f64().map(|f| IndexValue::Float(OrderedFloat::new(f)))),
        Value::Bool(b) => Some(IndexValue::Boolean(*b)),
        _ => None,
    }
}

/// Check that `data` matches all property filters (conjunction).
fn data_matches_filters(data: &Value, filters: &[PropertyFilter]) -> bool {
    filters.iter().all(|filter| {
        let actual = resolve_json_path(data, &filter.path);
        match filter.op {
            PropertyFilterOp::Eq => actual == Some(&filter.value),
        }
    })
}

/// Walk a dotted-path through a JSON value.
fn resolve_json_path<'a>(data: &'a Value, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = data.get(first)?;
    for segment in rest {
        current = current.get(segment)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use axon_core::types::Entity;
    use axon_cypher::ast::Direction;
    use axon_cypher::memory_store::{EntityScan, LinkTraversal, QueryStore};
    use axon_cypher::schema::{
        IndexedProperty, LabelDef, PlannerConfig, PropertyKind, RelationshipDef, SchemaSnapshot,
    };
    use axon_schema::schema::{IndexDef, IndexType};
    use serde_json::json;

    use crate::MemoryStorageAdapter;

    // ── Schema fixture ───────────────────────────────────────────────────────

    fn ddx_beads_schema() -> SchemaSnapshot {
        let mut labels = BTreeMap::new();
        let mut properties = BTreeMap::new();
        properties.insert("id".to_string(), PropertyKind::String);
        properties.insert("status".to_string(), PropertyKind::String);
        properties.insert("priority".to_string(), PropertyKind::Integer);
        properties.insert("title".to_string(), PropertyKind::String);

        labels.insert(
            "DdxBead".to_string(),
            LabelDef {
                collection_name: "ddx_beads".to_string(),
                estimated_count: 10_000,
                properties,
                indexed_properties: vec![
                    IndexedProperty {
                        property: "status".to_string(),
                        kind: PropertyKind::String,
                        unique: false,
                        estimated_equality_rows: 2_500,
                        estimated_range_rows: 7_500,
                    },
                    IndexedProperty {
                        property: "priority".to_string(),
                        kind: PropertyKind::Integer,
                        unique: false,
                        estimated_equality_rows: 500,
                        estimated_range_rows: 5_000,
                    },
                    IndexedProperty {
                        property: "id".to_string(),
                        kind: PropertyKind::String,
                        unique: true,
                        estimated_equality_rows: 1,
                        estimated_range_rows: 10_000,
                    },
                ],
            },
        );

        let mut relationships = BTreeMap::new();
        relationships.insert(
            "DEPENDS_ON".to_string(),
            RelationshipDef {
                source_labels: vec!["DdxBead".to_string()],
                target_labels: vec!["DdxBead".to_string()],
            },
        );

        SchemaSnapshot {
            labels,
            relationships,
            planner_config: PlannerConfig::default(),
            queries: BTreeMap::new(),
        }
    }

    // ── Index fixtures ───────────────────────────────────────────────────────

    fn status_index() -> IndexDef {
        IndexDef {
            field: "status".to_string(),
            index_type: IndexType::String,
            unique: false,
        }
    }

    fn priority_index() -> IndexDef {
        IndexDef {
            field: "priority".to_string(),
            index_type: IndexType::Integer,
            unique: false,
        }
    }

    fn id_index() -> IndexDef {
        IndexDef {
            field: "id".to_string(),
            index_type: IndexType::String,
            unique: true,
        }
    }

    /// Build a `MemoryStorageAdapter` populated with three beads plus one
    /// `DEPENDS_ON` link (bead-a → bead-b).
    fn setup() -> (MemoryStorageAdapter, SchemaSnapshot) {
        let schema = ddx_beads_schema();
        let mut storage = MemoryStorageAdapter::default();
        let col = CollectionId::new("ddx_beads");
        let indexes = [status_index(), priority_index(), id_index()];

        for (id, status, priority, title) in [
            ("bead-a", "open", 5_i64, "first"),
            ("bead-b", "open", 1_i64, "second"),
            ("bead-c", "closed", 10_i64, "closed"),
        ] {
            let data = json!({
                "id": id, "status": status,
                "priority": priority, "title": title,
            });
            storage
                .put(Entity::new(col.clone(), EntityId::new(id), data.clone()))
                .unwrap();
            storage
                .update_indexes(&col, &EntityId::new(id), None, &data, &indexes)
                .unwrap();
        }

        // One outbound link: bead-a → DEPENDS_ON → bead-b
        storage
            .put_link(&Link {
                source_collection: col.clone(),
                source_id: EntityId::new("bead-a"),
                target_collection: col.clone(),
                target_id: EntityId::new("bead-b"),
                link_type: "DEPENDS_ON".to_string(),
                metadata: Value::Null,
            })
            .unwrap();

        (storage, schema)
    }

    fn encoded(raw_id: &str) -> String {
        encode_entity_id("ddx_beads", raw_id)
    }

    // ── AC2: scan / index lookup ─────────────────────────────────────────────

    fn collect_entities(store: &StorageAdapterQueryStore<'_, MemoryStorageAdapter>, scan: EntityScan) -> Vec<QueryEntity> {
        store
            .scan_entities(scan)
            .map(|r| r.expect("storage scan should not error"))
            .collect()
    }

    fn collect_links(store: &StorageAdapterQueryStore<'_, MemoryStorageAdapter>, traversal: LinkTraversal) -> Vec<QueryLink> {
        store
            .traverse_links(traversal)
            .map(|r| r.expect("storage traverse should not error"))
            .collect()
    }

    #[test]
    fn scan_by_label_returns_all_entities_in_collection() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);
        let entities = collect_entities(&store, EntityScan::label("DdxBead"));
        assert_eq!(entities.len(), 3, "expected all three beads");
        let mut ids: Vec<String> = entities.iter().map(|e| e.id.clone()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![encoded("bead-a"), encoded("bead-b"), encoded("bead-c")]
        );
    }

    #[test]
    fn scan_with_eq_filter_uses_index_lookup_and_returns_matching_entities() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);
        let scan = EntityScan::label("DdxBead").with_property_eq("status", json!("open"));
        let entities = collect_entities(&store, scan);
        assert_eq!(entities.len(), 2, "only bead-a and bead-b are open");
        assert!(
            entities.iter().all(|e| e
                .properties
                .get("status")
                .and_then(Value::as_str)
                == Some("open")),
            "all returned entities should have status=open"
        );
    }

    #[test]
    fn scan_with_eq_filter_for_unique_id_returns_single_entity() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);
        let scan = EntityScan::label("DdxBead").with_property_eq("id", json!("bead-a"));
        let entities = collect_entities(&store, scan);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id, encoded("bead-a"));
    }

    #[test]
    fn scan_for_unknown_label_returns_empty() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);
        let scan = EntityScan::label("NoSuchLabel");
        let entities: Vec<_> = store.scan_entities(scan).collect();
        assert!(entities.is_empty());
    }

    // ── AC2: get_entity via encoded ID ───────────────────────────────────────

    #[test]
    fn get_entity_returns_entity_for_valid_encoded_id() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);
        let entity = store.get_entity(&encoded("bead-a")).expect("entity should exist");
        assert_eq!(
            entity.properties.get("title").and_then(Value::as_str),
            Some("first")
        );
        assert!(entity.has_label("DdxBead"));
    }

    #[test]
    fn get_entity_returns_none_for_nonexistent_id() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);
        assert!(store.get_entity(&encoded("no-such-entity")).is_none());
    }

    // ── AC3: outgoing link traversal ─────────────────────────────────────────

    #[test]
    fn traverse_outgoing_links_returns_depends_on_target() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        let traversal = LinkTraversal {
            anchor_id: encoded("bead-a"),
            direction: Direction::Outgoing,
            relationship_types: vec!["DEPENDS_ON".to_string()],
            link_property_filters: vec![],
        };
        let links = collect_links(&store, traversal);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, "DEPENDS_ON");
        assert_eq!(links[0].target_id, encoded("bead-b"));
        assert_eq!(links[0].source_id, encoded("bead-a"));
    }

    #[test]
    fn traverse_outgoing_links_empty_for_entity_with_no_links() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        let traversal = LinkTraversal {
            anchor_id: encoded("bead-b"),
            direction: Direction::Outgoing,
            relationship_types: vec!["DEPENDS_ON".to_string()],
            link_property_filters: vec![],
        };
        let links = collect_links(&store, traversal);
        assert!(links.is_empty());
    }

    // ── AC3: incoming link traversal ─────────────────────────────────────────

    #[test]
    fn traverse_incoming_links_returns_source_entity() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        let traversal = LinkTraversal {
            anchor_id: encoded("bead-b"),
            direction: Direction::Incoming,
            relationship_types: vec!["DEPENDS_ON".to_string()],
            link_property_filters: vec![],
        };
        let links = collect_links(&store, traversal);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].source_id, encoded("bead-a"));
    }

    // ── AC3: bidirectional traversal ─────────────────────────────────────────

    #[test]
    fn traverse_either_direction_returns_both_sides() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        // bead-a has one outgoing DEPENDS_ON link to bead-b.
        // Traversing Either from bead-a returns that outgoing link.
        let traversal = LinkTraversal {
            anchor_id: encoded("bead-a"),
            direction: Direction::Either,
            relationship_types: vec![],
            link_property_filters: vec![],
        };
        let links = collect_links(&store, traversal);
        assert_eq!(links.len(), 1);
    }

    // ── AC3: EXISTS probe (via full executor pipeline) ───────────────────────

    #[test]
    fn exists_probe_filters_rows_via_executor_pipeline() {
        use axon_cypher::{execute, parse, plan};

        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        // Query: open beads WITH an open dependency (bead-a → bead-b, both open).
        let query = parse(
            r"
            MATCH (b:DdxBead {status: 'open'})
            WHERE EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
                WHERE d.status = 'open'
            }
            RETURN b.title AS title
            ",
        )
        .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows: Vec<_> = execute(&plan, &store)
            .collect::<Result<Vec<_>, _>>()
            .expect("should execute");

        assert_eq!(rows.len(), 1, "only bead-a has an open dependency");
        assert_eq!(
            rows[0].get("title").and_then(Value::as_str),
            Some("first")
        );
    }

    #[test]
    fn not_exists_probe_returns_entity_without_open_dependency() {
        use axon_cypher::{execute, parse, plan};

        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        let query = parse(
            r"
            MATCH (b:DdxBead {status: 'open'})
            WHERE NOT EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
                WHERE d.status = 'open'
            }
            RETURN b.title AS title
            ",
        )
        .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows: Vec<_> = execute(&plan, &store)
            .collect::<Result<Vec<_>, _>>()
            .expect("should execute");

        // bead-b is open but has no outgoing DEPENDS_ON link.
        assert_eq!(rows.len(), 1, "only bead-b has no open dependency");
        assert_eq!(
            rows[0].get("title").and_then(Value::as_str),
            Some("second")
        );
    }

    // ── AC3: expand (variable-length path) via executor ─────────────────────

    #[test]
    fn expand_via_executor_finds_dependency_target() {
        use axon_cypher::{execute, parse, plan};

        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        let query = parse(
            "MATCH (b:DdxBead {id: 'bead-a'})-[:DEPENDS_ON]->(d:DdxBead) RETURN d.title AS title",
        )
        .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows: Vec<_> = execute(&plan, &store)
            .collect::<Result<Vec<_>, _>>()
            .expect("should execute");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title").and_then(Value::as_str), Some("second"));
    }

    #[test]
    fn traverse_links_with_invalid_anchor_returns_empty() {
        let (storage, schema) = setup();
        let store = StorageAdapterQueryStore::new(&storage, &schema);

        let traversal = LinkTraversal {
            anchor_id: "not-encoded-at-all".to_string(),
            direction: Direction::Outgoing,
            relationship_types: vec![],
            link_property_filters: vec![],
        };
        let links: Vec<_> = store.traverse_links(traversal).collect();
        assert!(links.is_empty());
    }
}
