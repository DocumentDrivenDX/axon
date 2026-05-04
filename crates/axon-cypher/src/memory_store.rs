//! Executor-facing query store abstractions for Cypher.
//!
//! This module is intentionally independent of `axon-storage`. It models the
//! logical graph shape the Cypher executor needs: labeled entities with JSON
//! properties and directed, typed links with JSON properties.

use crate::ast::Direction;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub type QueryEntityId = String;
pub type QueryLinkId = String;
pub type PropertyMap = BTreeMap<String, Value>;

pub type EntityStream<'a> = Box<dyn Iterator<Item = &'a QueryEntity> + 'a>;
pub type LinkStream<'a> = Box<dyn Iterator<Item = &'a QueryLink> + 'a>;

/// Storage boundary consumed by future executor operators.
pub trait QueryStore {
    fn get_entity(&self, id: &str) -> Option<&QueryEntity>;

    fn scan_entities<'a>(&'a self, scan: &'a EntityScan) -> EntityStream<'a>;

    fn get_link(&self, id: &str) -> Option<&QueryLink>;

    fn traverse_links<'a>(&'a self, traversal: &'a LinkTraversal) -> LinkStream<'a>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryEntity {
    pub id: QueryEntityId,
    pub labels: BTreeSet<String>,
    pub properties: PropertyMap,
}

impl QueryEntity {
    pub fn new(
        id: impl Into<QueryEntityId>,
        labels: impl IntoIterator<Item = impl Into<String>>,
        properties: PropertyMap,
    ) -> Self {
        Self {
            id: id.into(),
            labels: labels.into_iter().map(Into::into).collect(),
            properties,
        }
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.labels.contains(label)
    }

    pub fn property(&self, path: &[String]) -> Option<&Value> {
        property_at(&self.properties, path)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryLink {
    pub id: QueryLinkId,
    pub source_id: QueryEntityId,
    pub target_id: QueryEntityId,
    pub link_type: String,
    pub properties: PropertyMap,
}

impl QueryLink {
    pub fn new(
        source_id: impl Into<QueryEntityId>,
        link_type: impl Into<String>,
        target_id: impl Into<QueryEntityId>,
        properties: PropertyMap,
    ) -> Self {
        let source_id = source_id.into();
        let link_type = link_type.into();
        let target_id = target_id.into();
        Self {
            id: format!("{source_id}/{link_type}/{target_id}"),
            source_id,
            target_id,
            link_type,
            properties,
        }
    }

    pub fn with_id(
        id: impl Into<QueryLinkId>,
        source_id: impl Into<QueryEntityId>,
        link_type: impl Into<String>,
        target_id: impl Into<QueryEntityId>,
        properties: PropertyMap,
    ) -> Self {
        Self {
            id: id.into(),
            source_id: source_id.into(),
            target_id: target_id.into(),
            link_type: link_type.into(),
            properties,
        }
    }

    pub fn property(&self, path: &[String]) -> Option<&Value> {
        property_at(&self.properties, path)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityScan {
    pub label: Option<String>,
    pub property_filters: Vec<PropertyFilter>,
}

impl EntityScan {
    pub fn label(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            property_filters: Vec::new(),
        }
    }

    pub fn with_property_eq(mut self, path: impl IntoPropertyPath, value: Value) -> Self {
        self.property_filters.push(PropertyFilter::eq(path, value));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkTraversal {
    pub anchor_id: QueryEntityId,
    pub direction: Direction,
    pub relationship_types: Vec<String>,
    pub link_property_filters: Vec<PropertyFilter>,
}

impl LinkTraversal {
    pub fn new(anchor_id: impl Into<QueryEntityId>, direction: Direction) -> Self {
        Self {
            anchor_id: anchor_id.into(),
            direction,
            relationship_types: Vec::new(),
            link_property_filters: Vec::new(),
        }
    }

    pub fn with_type(mut self, relationship_type: impl Into<String>) -> Self {
        self.relationship_types.push(relationship_type.into());
        self
    }

    pub fn with_property_eq(mut self, path: impl IntoPropertyPath, value: Value) -> Self {
        self.link_property_filters
            .push(PropertyFilter::eq(path, value));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyFilter {
    pub path: Vec<String>,
    pub op: PropertyFilterOp,
    pub value: Value,
}

impl PropertyFilter {
    pub fn eq(path: impl IntoPropertyPath, value: Value) -> Self {
        Self {
            path: path.into_property_path(),
            op: PropertyFilterOp::Eq,
            value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyFilterOp {
    Eq,
}

pub trait IntoPropertyPath {
    fn into_property_path(self) -> Vec<String>;
}

impl IntoPropertyPath for &str {
    fn into_property_path(self) -> Vec<String> {
        self.split('.')
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
            .collect()
    }
}

impl IntoPropertyPath for String {
    fn into_property_path(self) -> Vec<String> {
        self.as_str().into_property_path()
    }
}

impl IntoPropertyPath for Vec<String> {
    fn into_property_path(self) -> Vec<String> {
        self
    }
}

/// In-memory graph store for executor and planner integration tests.
#[derive(Debug, Clone, Default)]
pub struct MemoryQueryStore {
    entities: BTreeMap<QueryEntityId, QueryEntity>,
    links: BTreeMap<QueryLinkId, QueryLink>,
    outgoing_links: BTreeMap<QueryEntityId, BTreeSet<QueryLinkId>>,
    incoming_links: BTreeMap<QueryEntityId, BTreeSet<QueryLinkId>>,
}

impl MemoryQueryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_entity(&mut self, entity: QueryEntity) -> Option<QueryEntity> {
        self.entities.insert(entity.id.clone(), entity)
    }

    pub fn insert_link(&mut self, link: QueryLink) -> Option<QueryLink> {
        let replaced = self.links.insert(link.id.clone(), link.clone());
        if let Some(old) = &replaced {
            self.remove_link_indexes(old);
        }
        self.outgoing_links
            .entry(link.source_id.clone())
            .or_default()
            .insert(link.id.clone());
        self.incoming_links
            .entry(link.target_id.clone())
            .or_default()
            .insert(link.id.clone());
        replaced
    }

    pub fn entities_len(&self) -> usize {
        self.entities.len()
    }

    pub fn links_len(&self) -> usize {
        self.links.len()
    }

    fn remove_link_indexes(&mut self, link: &QueryLink) {
        if let Some(outgoing) = self.outgoing_links.get_mut(&link.source_id) {
            outgoing.remove(&link.id);
        }
        if let Some(incoming) = self.incoming_links.get_mut(&link.target_id) {
            incoming.remove(&link.id);
        }
    }

    fn link_ids_for_direction(&self, traversal: &LinkTraversal) -> BTreeSet<QueryLinkId> {
        match traversal.direction {
            Direction::Outgoing => self
                .outgoing_links
                .get(&traversal.anchor_id)
                .cloned()
                .unwrap_or_default(),
            Direction::Incoming => self
                .incoming_links
                .get(&traversal.anchor_id)
                .cloned()
                .unwrap_or_default(),
            Direction::Either => {
                let mut ids = self
                    .outgoing_links
                    .get(&traversal.anchor_id)
                    .cloned()
                    .unwrap_or_default();
                if let Some(incoming) = self.incoming_links.get(&traversal.anchor_id) {
                    ids.extend(incoming.iter().cloned());
                }
                ids
            }
        }
    }
}

impl QueryStore for MemoryQueryStore {
    fn get_entity(&self, id: &str) -> Option<&QueryEntity> {
        self.entities.get(id)
    }

    fn scan_entities<'a>(&'a self, scan: &'a EntityScan) -> EntityStream<'a> {
        Box::new(
            self.entities
                .values()
                .filter(move |entity| entity_matches_scan(entity, scan)),
        )
    }

    fn get_link(&self, id: &str) -> Option<&QueryLink> {
        self.links.get(id)
    }

    fn traverse_links<'a>(&'a self, traversal: &'a LinkTraversal) -> LinkStream<'a> {
        let link_ids = self.link_ids_for_direction(traversal);
        Box::new(link_ids.into_iter().filter_map(move |id| {
            self.links
                .get(&id)
                .filter(|link| link_matches_traversal(link, traversal))
        }))
    }
}

fn entity_matches_scan(entity: &QueryEntity, scan: &EntityScan) -> bool {
    scan.label
        .as_deref()
        .map_or(true, |label| entity.has_label(label))
        && scan
            .property_filters
            .iter()
            .all(|filter| matches_property(&entity.properties, filter))
}

fn link_matches_traversal(link: &QueryLink, traversal: &LinkTraversal) -> bool {
    (traversal.relationship_types.is_empty()
        || traversal
            .relationship_types
            .iter()
            .any(|relationship_type| relationship_type == &link.link_type))
        && traversal
            .link_property_filters
            .iter()
            .all(|filter| matches_property(&link.properties, filter))
}

fn matches_property(properties: &PropertyMap, filter: &PropertyFilter) -> bool {
    match filter.op {
        PropertyFilterOp::Eq => property_at(properties, &filter.path) == Some(&filter.value),
    }
}

fn property_at<'a>(properties: &'a PropertyMap, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = properties.get(first)?;
    for segment in rest {
        current = current.get(segment)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn properties(entries: impl IntoIterator<Item = (&'static str, Value)>) -> PropertyMap {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    #[test]
    fn entity_scan_filters_by_label_and_property() {
        let mut store = MemoryQueryStore::new();
        store.insert_entity(QueryEntity::new(
            "bead-a",
            ["DdxBead"],
            properties([("status", json!("open")), ("priority", json!(10))]),
        ));
        store.insert_entity(QueryEntity::new(
            "note-a",
            ["Note"],
            properties([("status", json!("open"))]),
        ));
        store.insert_entity(QueryEntity::new(
            "bead-b",
            ["DdxBead"],
            properties([("status", json!("closed"))]),
        ));

        let scan = EntityScan::label("DdxBead").with_property_eq("status", json!("open"));
        let ids: Vec<&str> = store
            .scan_entities(&scan)
            .map(|entity| entity.id.as_str())
            .collect();

        assert_eq!(ids, vec!["bead-a"]);
    }

    #[test]
    fn entity_lookup_by_id_returns_hand_built_properties() {
        let mut store = MemoryQueryStore::new();
        store.insert_entity(QueryEntity::new(
            "task-1",
            ["Task", "WorkItem"],
            properties([("owner", json!({"name": "Erik"}))]),
        ));

        let owner_path = vec!["owner".to_string(), "name".to_string()];
        let entity = store.get_entity("task-1").expect("entity should exist");

        assert!(entity.has_label("WorkItem"));
        assert_eq!(entity.property(&owner_path), Some(&json!("Erik")));
    }

    #[test]
    fn outgoing_link_traversal_filters_by_type_and_properties() {
        let mut store = MemoryQueryStore::new();
        store.insert_link(QueryLink::new(
            "bead-a",
            "DEPENDS_ON",
            "bead-b",
            properties([("active", json!(true)), ("reason", json!("blocked"))]),
        ));
        store.insert_link(QueryLink::new(
            "bead-a",
            "RELATES_TO",
            "bead-c",
            properties([("active", json!(true))]),
        ));
        store.insert_link(QueryLink::new(
            "bead-d",
            "DEPENDS_ON",
            "bead-a",
            properties([("active", json!(true))]),
        ));

        let traversal = LinkTraversal::new("bead-a", Direction::Outgoing)
            .with_type("DEPENDS_ON")
            .with_property_eq("active", json!(true));
        let targets: Vec<&str> = store
            .traverse_links(&traversal)
            .map(|link| link.target_id.as_str())
            .collect();

        assert_eq!(targets, vec!["bead-b"]);
    }

    #[test]
    fn incoming_and_bidirectional_traversals_use_directed_indexes() {
        let mut store = MemoryQueryStore::new();
        store.insert_link(QueryLink::new(
            "bead-a",
            "DEPENDS_ON",
            "bead-b",
            PropertyMap::new(),
        ));
        store.insert_link(QueryLink::new(
            "bead-c",
            "DEPENDS_ON",
            "bead-a",
            PropertyMap::new(),
        ));

        let incoming = LinkTraversal::new("bead-a", Direction::Incoming);
        let incoming_sources: Vec<&str> = store
            .traverse_links(&incoming)
            .map(|link| link.source_id.as_str())
            .collect();
        assert_eq!(incoming_sources, vec!["bead-c"]);

        let either = LinkTraversal::new("bead-a", Direction::Either);
        let link_ids: Vec<&str> = store
            .traverse_links(&either)
            .map(|link| link.id.as_str())
            .collect();
        assert_eq!(
            link_ids,
            vec!["bead-a/DEPENDS_ON/bead-b", "bead-c/DEPENDS_ON/bead-a"]
        );
    }
}
