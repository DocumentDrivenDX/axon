use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::BuildHasher;

use axon_cypher::ast::{
    Expression, FunctionArg, MatchClause, NodePattern, PathPattern, Query, Subquery,
};
use axon_cypher::schema::{
    IndexedProperty, LabelDef, PropertyKind, RelationshipDef, SchemaSnapshot,
};
use axon_cypher::{parse, plan, validate, CypherError};
use serde::{Deserialize, Serialize};

use crate::schema::{CollectionSchema, IndexType};

/// Schema compile report for schema-declared named queries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileReport {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<NamedQueryDiagnostic>,
}

impl CompileReport {
    pub fn is_success(&self) -> bool {
        self.queries
            .iter()
            .all(|diagnostic| diagnostic.status == NamedQueryStatus::Ok)
    }
}

/// Per-query schema compile diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedQueryDiagnostic {
    pub name: String,
    pub status: NamedQueryStatus,
    pub code: String,
    pub message: String,
}

/// Stable named-query compile statuses surfaced by `put_schema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedQueryStatus {
    Ok,
    ParseError,
    UnknownIdentifier,
    UnsupportedQueryPlan,
    PolicyRequiredBypass,
}

/// Compile every named query declared on `candidate` against the candidate
/// schema plus active peer schemas.
pub fn compile_named_queries<S: BuildHasher>(
    candidate: &CollectionSchema,
    active_schemas: &[CollectionSchema],
    estimated_counts: &HashMap<String, u64, S>,
) -> CompileReport {
    let mut schemas: Vec<CollectionSchema> = active_schemas
        .iter()
        .filter(|schema| schema.collection != candidate.collection)
        .cloned()
        .collect();
    schemas.push(candidate.clone());

    let snapshot = schema_snapshot_from_schemas(&schemas, estimated_counts);
    let label_to_collection = label_collection_map(&schemas);
    let redacted = redacted_fields_by_label(&schemas);

    let mut names: Vec<_> = candidate.queries.keys().cloned().collect();
    names.sort();

    let queries = names
        .into_iter()
        .map(|name| {
            let query = candidate
                .queries
                .get(&name)
                .expect("query name came from map keys");
            compile_one(
                &name,
                &query.cypher,
                &snapshot,
                &label_to_collection,
                &redacted,
            )
        })
        .collect();

    CompileReport { queries }
}

/// Convert Axon collection schemas into the minimal schema snapshot consumed
/// by `axon-cypher`.
pub fn schema_snapshot_from_schemas<S: BuildHasher>(
    schemas: &[CollectionSchema],
    estimated_counts: &HashMap<String, u64, S>,
) -> SchemaSnapshot {
    let mut labels = BTreeMap::new();
    for schema in schemas {
        let label_def = label_def(schema, estimated_counts);
        for alias in label_aliases(schema.collection.as_str()) {
            labels.insert(alias, label_def.clone());
        }
    }

    let collection_labels = collection_label_map(schemas);
    let mut relationships = BTreeMap::new();
    for schema in schemas {
        let source_labels = label_aliases(schema.collection.as_str());
        for (name, link_type) in &schema.link_types {
            let target_labels = collection_labels
                .get(&link_type.target_collection)
                .cloned()
                .unwrap_or_else(|| label_aliases(&link_type.target_collection));
            let def = RelationshipDef {
                source_labels: source_labels.clone(),
                target_labels,
            };
            relationships.insert(name.clone(), def.clone());
            relationships.insert(cypher_relationship_alias(name), def);
        }
    }

    SchemaSnapshot {
        labels,
        relationships,
        planner_config: Default::default(),
    }
}

fn compile_one(
    name: &str,
    cypher: &str,
    snapshot: &SchemaSnapshot,
    label_to_collection: &HashMap<String, String>,
    redacted: &HashMap<String, HashSet<String>>,
) -> NamedQueryDiagnostic {
    match parse(cypher).and_then(|query| {
        validate(&query, snapshot)?;
        validate_policy_compatibility(&query, label_to_collection, redacted)?;
        plan(&query, snapshot).map(|_| ())
    }) {
        Ok(()) => NamedQueryDiagnostic {
            name: name.to_string(),
            status: NamedQueryStatus::Ok,
            code: "ok".to_string(),
            message: "ok".to_string(),
        },
        Err(err) => diagnostic_from_error(name, err),
    }
}

fn diagnostic_from_error(name: &str, err: CypherError) -> NamedQueryDiagnostic {
    let status = match &err {
        CypherError::Parse { .. } => NamedQueryStatus::ParseError,
        CypherError::UnknownIdentifier { .. } => NamedQueryStatus::UnknownIdentifier,
        CypherError::PolicyRequiredBypass(_) => NamedQueryStatus::PolicyRequiredBypass,
        CypherError::UnsupportedClause(_)
        | CypherError::UnsupportedQueryPlan(_)
        | CypherError::QueryTooLarge(_)
        | CypherError::QueryTimeout(_) => NamedQueryStatus::UnsupportedQueryPlan,
    };
    let code = match status {
        NamedQueryStatus::Ok => "ok",
        NamedQueryStatus::ParseError => "parse_error",
        NamedQueryStatus::UnknownIdentifier => "unknown_identifier",
        NamedQueryStatus::UnsupportedQueryPlan => "unsupported_query_plan",
        NamedQueryStatus::PolicyRequiredBypass => "policy_required_bypass",
    };
    NamedQueryDiagnostic {
        name: name.to_string(),
        status,
        code: code.to_string(),
        message: err.to_string(),
    }
}

fn label_def<S: BuildHasher>(
    schema: &CollectionSchema,
    estimated_counts: &HashMap<String, u64, S>,
) -> LabelDef {
    let collection_name = schema.collection.to_string();
    let mut properties = entity_properties(schema);
    let mut indexed_properties = Vec::new();

    for index in &schema.indexes {
        let kind = property_kind_from_index(&index.index_type);
        properties.entry(index.field.clone()).or_insert(kind);
        indexed_properties.push(IndexedProperty {
            property: index.field.clone(),
            kind,
            unique: index.unique,
            estimated_equality_rows: 1,
            estimated_range_rows: estimated_counts
                .get(&collection_name)
                .copied()
                .unwrap_or_default(),
        });
    }
    for compound in &schema.compound_indexes {
        if let Some(first) = compound.fields.first() {
            let kind = property_kind_from_index(&first.index_type);
            properties.entry(first.field.clone()).or_insert(kind);
            indexed_properties.push(IndexedProperty {
                property: first.field.clone(),
                kind,
                unique: compound.unique,
                estimated_equality_rows: 1,
                estimated_range_rows: estimated_counts
                    .get(&collection_name)
                    .copied()
                    .unwrap_or_default(),
            });
        }
    }

    LabelDef {
        collection_name: collection_name.clone(),
        estimated_count: estimated_counts
            .get(&collection_name)
            .copied()
            .unwrap_or_default(),
        properties,
        indexed_properties,
    }
}

fn entity_properties(schema: &CollectionSchema) -> BTreeMap<String, PropertyKind> {
    let mut properties = BTreeMap::from([
        ("id".to_string(), PropertyKind::String),
        ("_id".to_string(), PropertyKind::String),
    ]);
    let Some(entity_schema) = &schema.entity_schema else {
        return properties;
    };
    let Some(object) = entity_schema
        .get("properties")
        .and_then(|value| value.as_object())
    else {
        return properties;
    };
    for (name, def) in object {
        properties.insert(name.clone(), property_kind_from_json_schema(def));
    }
    properties
}

fn property_kind_from_json_schema(def: &serde_json::Value) -> PropertyKind {
    match def.get("type").and_then(|value| value.as_str()) {
        Some("string")
            if def.get("format").and_then(|value| value.as_str()) == Some("date-time") =>
        {
            PropertyKind::DateTime
        }
        Some("string") => PropertyKind::String,
        Some("integer") => PropertyKind::Integer,
        Some("number") => PropertyKind::Float,
        Some("boolean") => PropertyKind::Boolean,
        _ => PropertyKind::Other,
    }
}

fn property_kind_from_index(index_type: &IndexType) -> PropertyKind {
    match index_type {
        IndexType::String => PropertyKind::String,
        IndexType::Integer => PropertyKind::Integer,
        IndexType::Float => PropertyKind::Float,
        IndexType::Datetime => PropertyKind::DateTime,
        IndexType::Boolean => PropertyKind::Boolean,
    }
}

fn validate_policy_compatibility(
    query: &Query,
    label_to_collection: &HashMap<String, String>,
    redacted: &HashMap<String, HashSet<String>>,
) -> Result<(), CypherError> {
    let bindings = bindings_for_query(query);
    for (variable, property) in policy_sensitive_property_refs(query) {
        let Some(label) = bindings.get(&variable) else {
            continue;
        };
        let Some(collection) = label_to_collection.get(label) else {
            continue;
        };
        if redacted
            .get(collection)
            .is_some_and(|fields| fields.contains(&property))
        {
            return Err(CypherError::PolicyRequiredBypass(format!(
                "query predicates or aggregations reference redacted field {label}.{property}"
            )));
        }
    }
    Ok(())
}

fn bindings_for_query(query: &Query) -> HashMap<String, String> {
    let mut bindings = HashMap::new();
    for clause in &query.matches {
        bind_match_clause(clause, &mut bindings);
    }
    if let Some(expr) = &query.where_clause {
        bind_expression_subqueries(expr, &mut bindings);
    }
    bindings
}

fn bind_match_clause(clause: &MatchClause, bindings: &mut HashMap<String, String>) {
    for pattern in &clause.patterns {
        bind_path_pattern(pattern, bindings);
    }
}

fn bind_path_pattern(pattern: &PathPattern, bindings: &mut HashMap<String, String>) {
    bind_node(&pattern.start, bindings);
    for step in &pattern.steps {
        bind_node(&step.node, bindings);
    }
}

fn bind_node(node: &NodePattern, bindings: &mut HashMap<String, String>) {
    if let (Some(variable), Some(label)) = (&node.variable, &node.label) {
        bindings.insert(variable.clone(), label.clone());
    }
}

fn bind_expression_subqueries(expr: &Expression, bindings: &mut HashMap<String, String>) {
    match expr {
        Expression::Exists(subquery) | Expression::NotExists(subquery) => {
            for clause in &subquery.matches {
                bind_match_clause(clause, bindings);
            }
            if let Some(where_clause) = &subquery.where_clause {
                bind_expression_subqueries(where_clause, bindings);
            }
        }
        Expression::BinaryLogical { left, right, .. }
        | Expression::Comparison { left, right, .. } => {
            bind_expression_subqueries(left, bindings);
            bind_expression_subqueries(right, bindings);
        }
        Expression::Not(inner)
        | Expression::IsNull {
            expression: inner, ..
        } => {
            bind_expression_subqueries(inner, bindings);
        }
        Expression::FunctionCall { arguments, .. } => {
            for argument in arguments {
                if let FunctionArg::Expression(expr) = argument {
                    bind_expression_subqueries(expr, bindings);
                }
            }
        }
        Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::Property { .. }
        | Expression::Parameter(_) => {}
    }
}

fn policy_sensitive_property_refs(query: &Query) -> Vec<(String, String)> {
    let mut refs = Vec::new();
    for clause in &query.matches {
        collect_match_inline_properties(clause, &mut refs);
    }
    if let Some(expr) = &query.where_clause {
        collect_property_refs(expr, &mut refs);
    }
    for item in &query.order_by {
        collect_property_refs(&item.expression, &mut refs);
    }
    for item in &query.return_clause.items {
        collect_aggregation_property_refs(&item.expression, &mut refs);
    }
    refs
}

fn collect_match_inline_properties(clause: &MatchClause, refs: &mut Vec<(String, String)>) {
    for pattern in &clause.patterns {
        collect_node_inline_properties(&pattern.start, refs);
        for step in &pattern.steps {
            collect_node_inline_properties(&step.node, refs);
        }
    }
}

fn collect_node_inline_properties(node: &NodePattern, refs: &mut Vec<(String, String)>) {
    let Some(variable) = &node.variable else {
        return;
    };
    refs.extend(
        node.properties
            .iter()
            .map(|entry| (variable.clone(), entry.key.clone())),
    );
}

fn collect_property_refs(expr: &Expression, refs: &mut Vec<(String, String)>) {
    match expr {
        Expression::Property { variable, path } => {
            if let Some(first) = path.first() {
                refs.push((variable.clone(), first.clone()));
            }
        }
        Expression::BinaryLogical { left, right, .. }
        | Expression::Comparison { left, right, .. } => {
            collect_property_refs(left, refs);
            collect_property_refs(right, refs);
        }
        Expression::Not(inner)
        | Expression::IsNull {
            expression: inner, ..
        } => {
            collect_property_refs(inner, refs);
        }
        Expression::Exists(subquery) | Expression::NotExists(subquery) => {
            collect_subquery_property_refs(subquery, refs);
        }
        Expression::FunctionCall { arguments, .. } => {
            for argument in arguments {
                if let FunctionArg::Expression(expr) = argument {
                    collect_property_refs(expr, refs);
                }
            }
        }
        Expression::Literal(_) | Expression::Variable(_) | Expression::Parameter(_) => {}
    }
}

fn collect_subquery_property_refs(subquery: &Subquery, refs: &mut Vec<(String, String)>) {
    for clause in &subquery.matches {
        collect_match_inline_properties(clause, refs);
    }
    if let Some(where_clause) = &subquery.where_clause {
        collect_property_refs(where_clause, refs);
    }
}

fn collect_aggregation_property_refs(expr: &Expression, refs: &mut Vec<(String, String)>) {
    if let Expression::FunctionCall { arguments, .. } = expr {
        for argument in arguments {
            if let FunctionArg::Expression(expr) = argument {
                collect_property_refs(expr, refs);
            }
        }
    }
}

fn redacted_fields_by_label(schemas: &[CollectionSchema]) -> HashMap<String, HashSet<String>> {
    let mut redacted = HashMap::new();
    for schema in schemas {
        let fields: HashSet<String> = schema
            .access_control
            .as_ref()
            .into_iter()
            .flat_map(|policy| &policy.fields)
            .filter(|(_, field_policy)| {
                field_policy
                    .read
                    .as_ref()
                    .is_some_and(|read| !read.deny.is_empty())
            })
            .map(|(field, _)| field_root(field).to_string())
            .collect();
        if !fields.is_empty() {
            redacted.insert(schema.collection.to_string(), fields);
        }
    }
    redacted
}

fn field_root(field: &str) -> &str {
    field
        .split(['.', '['])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(field)
}

fn label_collection_map(schemas: &[CollectionSchema]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for schema in schemas {
        for alias in label_aliases(schema.collection.as_str()) {
            map.insert(alias, schema.collection.to_string());
        }
    }
    map
}

fn collection_label_map(schemas: &[CollectionSchema]) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for schema in schemas {
        map.insert(
            schema.collection.to_string(),
            label_aliases(schema.collection.as_str()),
        );
    }
    map
}

fn label_aliases(collection: &str) -> Vec<String> {
    let bare = collection.split('.').next_back().unwrap_or(collection);
    let mut aliases = vec![bare.to_string(), pascal_singular(bare)];
    aliases.sort();
    aliases.dedup();
    aliases
}

fn pascal_singular(name: &str) -> String {
    let mut out = String::new();
    for part in name.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let singular = part.strip_suffix('s').unwrap_or(part);
        let mut chars = singular.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

fn cypher_relationship_alias(name: &str) -> String {
    name.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_uppercase)
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axon_core::id::CollectionId;
    use serde_json::json;

    use super::*;
    use crate::schema::{
        Cardinality, IndexDef, IndexType, LinkTypeDef, NamedQueryDef, NamedQueryParameter,
    };

    fn bead_schema() -> CollectionSchema {
        let mut schema = CollectionSchema::new(CollectionId::new("ddx_beads"));
        schema.entity_schema = Some(json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "priority": { "type": "integer" },
                "updated_at": { "type": "string", "format": "date-time" },
                "secret": { "type": "string" }
            }
        }));
        schema.indexes = vec![
            IndexDef {
                field: "status".into(),
                index_type: IndexType::String,
                unique: false,
            },
            IndexDef {
                field: "id".into(),
                index_type: IndexType::String,
                unique: true,
            },
        ];
        schema.link_types.insert(
            "depends_on".into(),
            LinkTypeDef {
                target_collection: "ddx_beads".into(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );
        schema
    }

    fn named(cypher: &str) -> NamedQueryDef {
        NamedQueryDef {
            description: "test query".into(),
            cypher: cypher.into(),
            parameters: Vec::new(),
        }
    }

    fn compile(schema: &CollectionSchema, count: u64) -> CompileReport {
        compile_named_queries(
            schema,
            &[],
            &HashMap::from([(schema.collection.to_string(), count)]),
        )
    }

    #[test]
    fn valid_query_reports_ok() {
        let mut schema = bead_schema();
        schema.queries.insert(
            "ready_beads".into(),
            named("MATCH (b:DdxBead {status: 'open'}) RETURN b"),
        );

        let report = compile(&schema, 10_000);

        assert!(report.is_success(), "{report:?}");
        assert_eq!(report.queries[0].status, NamedQueryStatus::Ok);
    }

    #[test]
    fn parse_error_reports_parse_error() {
        let mut schema = bead_schema();
        schema
            .queries
            .insert("broken".into(), named("MATCH (b:DdxBead RETURN b"));

        let report = compile(&schema, 0);

        assert_eq!(report.queries[0].status, NamedQueryStatus::ParseError);
        assert_eq!(report.queries[0].code, "parse_error");
    }

    #[test]
    fn unknown_label_reports_unknown_identifier() {
        let mut schema = bead_schema();
        schema
            .queries
            .insert("bad_label".into(), named("MATCH (b:Missing) RETURN b"));

        let report = compile(&schema, 0);

        assert_eq!(
            report.queries[0].status,
            NamedQueryStatus::UnknownIdentifier
        );
    }

    #[test]
    fn unknown_property_reports_unknown_identifier() {
        let mut schema = bead_schema();
        schema.queries.insert(
            "bad_property".into(),
            named("MATCH (b:DdxBead) WHERE b.missing = 'x' RETURN b"),
        );

        let report = compile(&schema, 0);

        assert_eq!(
            report.queries[0].status,
            NamedQueryStatus::UnknownIdentifier
        );
    }

    #[test]
    fn unknown_relationship_reports_unknown_identifier() {
        let mut schema = bead_schema();
        schema.queries.insert(
            "bad_rel".into(),
            named("MATCH (b:DdxBead)-[:MISSING]->(d:DdxBead) RETURN b"),
        );

        let report = compile(&schema, 0);

        assert_eq!(
            report.queries[0].status,
            NamedQueryStatus::UnknownIdentifier
        );
    }

    #[test]
    fn unindexed_plan_on_large_collection_reports_unsupported_query_plan() {
        let mut schema = bead_schema();
        schema.queries.insert(
            "scan".into(),
            named("MATCH (b:DdxBead) WHERE b.secret = 'x' RETURN b"),
        );

        let report = compile(&schema, 1_001);

        assert_eq!(
            report.queries[0].status,
            NamedQueryStatus::UnsupportedQueryPlan
        );
        assert!(report.queries[0].message.contains("missing-index"));
    }

    #[test]
    fn policy_bypass_reports_policy_required_bypass() {
        let mut schema = bead_schema();
        schema.access_control = Some(
            serde_json::from_value(json!({
                "fields": {
                    "secret": {
                        "read": {
                            "deny": [{ "name": "hide-secret", "redact_as": null }]
                        }
                    }
                }
            }))
            .unwrap(),
        );
        schema.indexes.push(IndexDef {
            field: "secret".into(),
            index_type: IndexType::String,
            unique: false,
        });
        schema.queries.insert(
            "secret_filter".into(),
            named("MATCH (b:DdxBead) WHERE b.secret = 'x' RETURN b"),
        );

        let report = compile(&schema, 10);

        assert_eq!(
            report.queries[0].status,
            NamedQueryStatus::PolicyRequiredBypass
        );
        assert_eq!(report.queries[0].code, "policy_required_bypass");
    }

    #[test]
    fn parameterized_query_reports_ok() {
        let mut schema = bead_schema();
        schema.queries.insert(
            "by_status".into(),
            NamedQueryDef {
                description: "by status".into(),
                cypher: "MATCH (b:DdxBead {status: $status}) RETURN b".into(),
                parameters: vec![NamedQueryParameter {
                    name: "status".into(),
                    param_type: "String".into(),
                    required: true,
                }],
            },
        );

        let report = compile(&schema, 10_000);

        assert_eq!(report.queries[0].status, NamedQueryStatus::Ok);
    }

    #[test]
    fn multiple_named_queries_report_individually_in_name_order() {
        let mut schema = bead_schema();
        schema.queries.insert(
            "z_valid".into(),
            named("MATCH (b:DdxBead {status: 'open'}) RETURN b"),
        );
        schema
            .queries
            .insert("a_bad".into(), named("MATCH (b:Missing) RETURN b"));

        let report = compile(&schema, 10_000);

        assert_eq!(report.queries.len(), 2);
        assert_eq!(report.queries[0].name, "a_bad");
        assert_eq!(
            report.queries[0].status,
            NamedQueryStatus::UnknownIdentifier
        );
        assert_eq!(report.queries[1].name, "z_valid");
        assert_eq!(report.queries[1].status, NamedQueryStatus::Ok);
    }
}
