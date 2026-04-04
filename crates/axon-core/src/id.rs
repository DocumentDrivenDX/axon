use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies a collection within an Axon instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Identifies an entity within a collection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(String);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
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
    fn entity_id_roundtrip() {
        let id = EntityId::new("ent-001");
        assert_eq!(id.as_str(), "ent-001");
    }

    #[test]
    fn link_id_roundtrip() {
        let id = LinkId::new("link-001");
        assert_eq!(id.as_str(), "link-001");
    }
}
