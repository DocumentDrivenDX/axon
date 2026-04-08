//! GraphQL aggregation types (US-064, FEAT-018).
//!
//! Provides the types for `beadsAggregate` queries auto-generated per
//! collection: filter, groupBy, and aggregation function definitions.

use serde::{Deserialize, Serialize};

/// An aggregation function supported in GraphQL queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GqlAggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl GqlAggregateFunction {
    /// Parse from a string, returning None for unknown functions.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "count" => Some(Self::Count),
            "sum" => Some(Self::Sum),
            "avg" => Some(Self::Avg),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            _ => None,
        }
    }
}

/// An aggregation request derived from a GraphQL query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GqlAggregateRequest {
    /// The collection to aggregate.
    pub collection: String,
    /// The aggregation function.
    pub function: GqlAggregateFunction,
    /// The field to aggregate.
    pub field: String,
    /// Optional filter (as a JSON value matching the collection filter syntax).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,
    /// Optional field to group results by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
}

/// A single aggregation result group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GqlAggregateGroup {
    /// The group key (None when no GROUP BY).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_key: Option<serde_json::Value>,
    /// The aggregated value.
    pub value: serde_json::Value,
}

/// GraphQL type name generator for aggregation.
pub fn aggregate_type_name(collection: &str) -> String {
    let mut name = String::with_capacity(collection.len() + 9);
    // Capitalize first letter.
    let mut chars = collection.chars();
    if let Some(first) = chars.next() {
        name.extend(first.to_uppercase());
    }
    name.extend(chars);
    name.push_str("Aggregate");
    name
}

/// GraphQL field name for the aggregation query.
pub fn aggregate_field_name(collection: &str) -> String {
    format!("{collection}Aggregate")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_aggregate_functions() {
        assert_eq!(
            GqlAggregateFunction::parse("count"),
            Some(GqlAggregateFunction::Count)
        );
        assert_eq!(
            GqlAggregateFunction::parse("SUM"),
            Some(GqlAggregateFunction::Sum)
        );
        assert_eq!(
            GqlAggregateFunction::parse("avg"),
            Some(GqlAggregateFunction::Avg)
        );
        assert_eq!(
            GqlAggregateFunction::parse("MIN"),
            Some(GqlAggregateFunction::Min)
        );
        assert_eq!(
            GqlAggregateFunction::parse("max"),
            Some(GqlAggregateFunction::Max)
        );
        assert_eq!(GqlAggregateFunction::parse("median"), None);
    }

    #[test]
    fn aggregate_type_name_capitalizes() {
        assert_eq!(aggregate_type_name("tasks"), "TasksAggregate");
        assert_eq!(aggregate_type_name("users"), "UsersAggregate");
    }

    #[test]
    fn aggregate_field_name_format() {
        assert_eq!(aggregate_field_name("tasks"), "tasksAggregate");
    }

    #[test]
    fn aggregate_request_serialization() {
        let req = GqlAggregateRequest {
            collection: "tasks".into(),
            function: GqlAggregateFunction::Sum,
            field: "priority".into(),
            filter: Some(json!({"status": "open"})),
            group_by: Some("assignee".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: GqlAggregateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.function, GqlAggregateFunction::Sum);
        assert_eq!(parsed.field, "priority");
        assert_eq!(parsed.group_by.as_deref(), Some("assignee"));
    }

    #[test]
    fn aggregate_group_with_key() {
        let group = GqlAggregateGroup {
            group_key: Some(json!("open")),
            value: json!(42),
        };
        let json = serde_json::to_string(&group).unwrap();
        let parsed: GqlAggregateGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.value, json!(42));
    }

    #[test]
    fn aggregate_group_without_key() {
        let group = GqlAggregateGroup {
            group_key: None,
            value: json!(100.5),
        };
        let json = serde_json::to_string(&group).unwrap();
        assert!(!json.contains("group_key"));
    }
}
