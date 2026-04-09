//! Node topology types (US-039, FEAT-014, P4 — design only in V1).
//!
//! These types define the data model for multi-node Axon deployments.
//! In V1, this is design-only — no runtime behavior is implemented.

use serde::{Deserialize, Serialize};

/// A registered Axon node in the cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique node identifier.
    pub node_id: String,
    /// Host address (e.g., "10.0.1.5:50051").
    pub address: String,
    /// Node status.
    pub status: NodeStatus,
    /// Databases placed on this node.
    pub databases: Vec<String>,
}

/// Status of a node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    /// Node is online and accepting requests.
    Active,
    /// Node is draining — not accepting new placements.
    Draining,
    /// Node is offline.
    Offline,
}

/// Database placement: which node hosts a database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    /// Database name.
    pub database: String,
    /// Node that hosts this database.
    pub node_id: String,
    /// Whether this is the primary replica.
    pub primary: bool,
}

/// Node registry for the cluster.
///
/// V1: In-memory only, design placeholder. Future versions will persist
/// this via a consensus protocol.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NodeRegistry {
    pub nodes: Vec<NodeInfo>,
    pub placements: Vec<Placement>,
}

impl NodeRegistry {
    /// Register a new node.
    pub fn register(&mut self, node: NodeInfo) {
        // Remove existing entry for this node_id, then re-add.
        self.nodes.retain(|n| n.node_id != node.node_id);
        self.nodes.push(node);
    }

    /// Deregister a node.
    pub fn deregister(&mut self, node_id: &str) {
        self.nodes.retain(|n| n.node_id != node_id);
        self.placements.retain(|p| p.node_id != node_id);
    }

    /// Find the node hosting a database.
    pub fn find_database(&self, database: &str) -> Option<&NodeInfo> {
        let placement = self
            .placements
            .iter()
            .find(|p| p.database == database && p.primary)?;
        self.nodes.iter().find(|n| n.node_id == placement.node_id)
    }

    /// Place a database on a node.
    pub fn place(&mut self, database: String, node_id: String, primary: bool) {
        self.placements.push(Placement {
            database,
            node_id,
            primary,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str) -> NodeInfo {
        NodeInfo {
            node_id: id.into(),
            address: format!("10.0.0.1:{id}"),
            status: NodeStatus::Active,
            databases: vec![],
        }
    }

    #[test]
    fn register_and_find_node() {
        let mut registry = NodeRegistry::default();
        registry.register(make_node("node-1"));
        assert_eq!(registry.nodes.len(), 1);
    }

    #[test]
    fn deregister_removes_node_and_placements() {
        let mut registry = NodeRegistry::default();
        registry.register(make_node("node-1"));
        registry.place("db1".into(), "node-1".into(), true);
        registry.deregister("node-1");
        assert!(registry.nodes.is_empty());
        assert!(registry.placements.is_empty());
    }

    #[test]
    fn find_database_by_placement() {
        let mut registry = NodeRegistry::default();
        registry.register(make_node("node-1"));
        registry.place("mydb".into(), "node-1".into(), true);
        let node = registry
            .find_database("mydb")
            .expect("placed database should resolve to its node");
        assert_eq!(node.node_id, "node-1");
    }

    #[test]
    fn find_nonexistent_database_returns_none() {
        let registry = NodeRegistry::default();
        assert!(registry.find_database("nope").is_none());
    }

    #[test]
    fn node_status_serialization() {
        let node = make_node("n1");
        let json = serde_json::to_string(&node).expect("node info should serialize to JSON");
        let parsed: NodeInfo =
            serde_json::from_str(&json).expect("node info JSON should deserialize");
        assert_eq!(parsed.status, NodeStatus::Active);
    }
}
