//! Graph exploration types for GraphQL (US-072, FEAT-020).
//!
//! Provides types and configuration for resolving linked entities
//! in GraphQL queries, with N+1 prevention via batching and depth limits.

use serde::{Deserialize, Serialize};

/// Configuration for graph exploration depth limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    /// Maximum depth for nested relationship resolution.
    /// Default: 3. Maximum: 10.
    pub max_depth: usize,
    /// Maximum number of linked entities to return per relationship field.
    /// Default: 100.
    pub max_links_per_field: usize,
    /// Whether to enable DataLoader batching for N+1 prevention.
    /// Default: true.
    pub enable_batching: bool,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_links_per_field: 100,
            enable_batching: true,
        }
    }
}

impl GraphConfig {
    /// Validate and clamp depth to allowed range.
    pub fn effective_depth(&self, requested: Option<usize>) -> usize {
        let depth = requested.unwrap_or(self.max_depth);
        depth.clamp(1, 10)
    }
}

/// A resolved relationship in a GraphQL response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRelationship {
    /// Link type name.
    pub link_type: String,
    /// Direction: "outbound" or "inbound".
    pub direction: String,
    /// Resolved linked entities (as JSON values).
    pub entities: Vec<serde_json::Value>,
    /// Total count (may exceed entities.len() if truncated by limit).
    pub total_count: usize,
}

/// Batch key for DataLoader N+1 prevention.
///
/// Groups entity lookups by (collection, id_list) to batch into
/// a single storage call instead of N individual lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BatchKey {
    pub collection: String,
    pub entity_ids: Vec<String>,
}

/// Track current resolution depth to enforce limits.
#[derive(Debug, Clone, Default)]
pub struct DepthTracker {
    current: usize,
    max: usize,
}

impl DepthTracker {
    /// Create a new depth tracker with the given limit.
    pub fn new(max_depth: usize) -> Self {
        Self {
            current: 0,
            max: max_depth,
        }
    }

    /// Try to descend one level. Returns false if limit would be exceeded.
    pub fn descend(&mut self) -> bool {
        if self.current >= self.max {
            return false;
        }
        self.current += 1;
        true
    }

    /// Ascend one level (after resolving a nested relationship).
    pub fn ascend(&mut self) {
        if self.current > 0 {
            self.current -= 1;
        }
    }

    /// Current depth level.
    pub fn depth(&self) -> usize {
        self.current
    }

    /// Whether we can go deeper.
    pub fn can_descend(&self) -> bool {
        self.current < self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = GraphConfig::default();
        assert_eq!(config.max_depth, 3);
        assert_eq!(config.max_links_per_field, 100);
        assert!(config.enable_batching);
    }

    #[test]
    fn effective_depth_clamps() {
        let config = GraphConfig::default();
        assert_eq!(config.effective_depth(Some(20)), 10); // clamped to max
        assert_eq!(config.effective_depth(Some(0)), 1); // clamped to min
        assert_eq!(config.effective_depth(None), 3); // default
    }

    #[test]
    fn depth_tracker_enforces_limit() {
        let mut tracker = DepthTracker::new(2);
        assert!(tracker.can_descend());
        assert!(tracker.descend());
        assert_eq!(tracker.depth(), 1);
        assert!(tracker.descend());
        assert_eq!(tracker.depth(), 2);
        assert!(!tracker.descend()); // at limit
        assert!(!tracker.can_descend());
    }

    #[test]
    fn depth_tracker_ascend() {
        let mut tracker = DepthTracker::new(3);
        tracker.descend();
        tracker.descend();
        assert_eq!(tracker.depth(), 2);
        tracker.ascend();
        assert_eq!(tracker.depth(), 1);
        assert!(tracker.can_descend());
    }

    #[test]
    fn depth_tracker_ascend_at_zero_is_safe() {
        let mut tracker = DepthTracker::new(3);
        tracker.ascend(); // should not underflow
        assert_eq!(tracker.depth(), 0);
    }

    #[test]
    fn resolved_relationship_serialization() {
        let rel = ResolvedRelationship {
            link_type: "depends-on".into(),
            direction: "outbound".into(),
            entities: vec![serde_json::json!({"id": "t-002", "title": "Task 2"})],
            total_count: 1,
        };
        let json = serde_json::to_string(&rel).expect("relationship should serialize");
        let parsed: ResolvedRelationship =
            serde_json::from_str(&json).expect("relationship JSON should deserialize");
        assert_eq!(parsed.link_type, "depends-on");
        assert_eq!(parsed.entities.len(), 1);
    }

    #[test]
    fn batch_key_equality() {
        let key1 = BatchKey {
            collection: "tasks".into(),
            entity_ids: vec!["t-001".into(), "t-002".into()],
        };
        let key2 = key1.clone();
        assert_eq!(key1, key2);
    }
}
