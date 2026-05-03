//! Rule-based planner for the ADR-021 Cypher subset.
//!
//! The planner compiles a validated AST plus [`SchemaSnapshot`] metadata into
//! a serializable operator tree. It does not execute the plan; storage-facing
//! execution is a later layer.

use crate::ast::{
    ComparisonOp, Direction, Expression, HopRange, Literal, MatchClause, NodePattern, PathPattern,
    Query, ReturnClause, SortItem, Subquery,
};
use crate::error::CypherError;
use crate::schema::{IndexedProperty, LabelDef, PlannerConfig, SchemaSnapshot};
use crate::validator;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Compile a parsed query into an execution plan.
pub fn plan(query: &Query, schema: &SchemaSnapshot) -> Result<ExecutionPlan, CypherError> {
    validator::validate(query, schema)?;
    Planner::new(schema).plan_query(query)
}

/// Serializable plan returned by the Cypher planner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub root: PlanOperator,
    pub estimated_rows: u64,
    pub diagnostics: Vec<PlanDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanDiagnostic {
    pub code: String,
    pub message: String,
}

/// Streaming operator tree used by the future executor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator", content = "plan")]
pub enum PlanOperator {
    Scan(Box<ScanPlan>),
    IndexLookup(Box<IndexLookupPlan>),
    Expand(Box<ExpandPlan>),
    Filter(Box<FilterPlan>),
    Project(Box<ProjectPlan>),
    Sort(Box<SortPlan>),
    Skip(Box<PagePlan>),
    Limit(Box<PagePlan>),
    ExistsCheck(Box<ExistsCheckPlan>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanPlan {
    pub alias: Option<String>,
    pub label: String,
    pub collection: String,
    pub predicate: Option<Expression>,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexLookupPlan {
    pub alias: Option<String>,
    pub label: String,
    pub collection: String,
    pub property: String,
    pub comparison: Option<IndexComparison>,
    pub value: Option<Expression>,
    pub ordered_by: Vec<IndexOrder>,
    pub unique: bool,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexComparison {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    In,
    Prefix,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexOrder {
    pub property: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpandPlan {
    pub input: PlanOperator,
    pub source_alias: Option<String>,
    pub target_alias: Option<String>,
    pub target_label: Option<String>,
    pub relationship_types: Vec<String>,
    pub direction: Direction,
    pub link_access: LinkAccess,
    pub min_depth: u32,
    pub max_depth: u32,
    pub optional: bool,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkAccess {
    OutgoingPrimaryKey,
    IncomingTargetIndex,
    BidirectionalIndexProbe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterPlan {
    pub input: PlanOperator,
    pub predicate: Expression,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectPlan {
    pub input: PlanOperator,
    pub return_clause: ReturnClause,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortPlan {
    pub input: PlanOperator,
    pub items: Vec<SortItem>,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PagePlan {
    pub input: PlanOperator,
    pub count: u64,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExistsCheckPlan {
    pub input: PlanOperator,
    pub subquery: ExistsSubplan,
    pub negated: bool,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExistsSubplan {
    pub link_probe: LinkProbePlan,
    pub target_index_probe: Option<IndexLookupPlan>,
    pub target_filter: Option<Expression>,
    pub estimated_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkProbePlan {
    pub source_alias: Option<String>,
    pub target_alias: Option<String>,
    pub target_label: Option<String>,
    pub relationship_types: Vec<String>,
    pub direction: Direction,
    pub link_access: LinkAccess,
}

impl PlanOperator {
    fn estimated_rows(&self) -> u64 {
        match self {
            Self::Scan(plan) => plan.estimated_rows,
            Self::IndexLookup(plan) => plan.estimated_rows,
            Self::Expand(plan) => plan.estimated_rows,
            Self::Filter(plan) => plan.estimated_rows,
            Self::Project(plan) => plan.estimated_rows,
            Self::Sort(plan) => plan.estimated_rows,
            Self::Skip(plan) | Self::Limit(plan) => plan.estimated_rows,
            Self::ExistsCheck(plan) => plan.estimated_rows,
        }
    }
}

#[derive(Debug, Clone)]
struct IndexPredicate {
    property: String,
    comparison: IndexComparison,
    value: Expression,
}

struct Planner<'a> {
    schema: &'a SchemaSnapshot,
    config: PlannerConfig,
    bindings: HashMap<String, Option<String>>,
    diagnostics: Vec<PlanDiagnostic>,
}

impl<'a> Planner<'a> {
    fn new(schema: &'a SchemaSnapshot) -> Self {
        Self {
            schema,
            config: schema.planner_config,
            bindings: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    fn plan_query(mut self, query: &Query) -> Result<ExecutionPlan, CypherError> {
        let mut current = None;
        for clause in &query.matches {
            current = Some(self.plan_match_clause(current, clause, query)?);
        }

        let mut root = current.ok_or_else(|| {
            CypherError::UnsupportedQueryPlan("query has no MATCH clause".to_string())
        })?;

        if let Some(where_clause) = &query.where_clause {
            root = self.apply_where(root, where_clause)?;
        }

        if !order_covered_by_existing_index(&root, &query.order_by) && !query.order_by.is_empty() {
            root = PlanOperator::Sort(Box::new(SortPlan {
                estimated_rows: root.estimated_rows(),
                input: root,
                items: query.order_by.clone(),
            }));
        }

        if let Some(skip) = query.skip {
            root = PlanOperator::Skip(Box::new(PagePlan {
                estimated_rows: root.estimated_rows().saturating_sub(skip),
                input: root,
                count: skip,
            }));
        }

        if let Some(limit) = query.limit {
            root = PlanOperator::Limit(Box::new(PagePlan {
                estimated_rows: root.estimated_rows().min(limit),
                input: root,
                count: limit,
            }));
        }

        root = PlanOperator::Project(Box::new(ProjectPlan {
            estimated_rows: root.estimated_rows(),
            input: root,
            return_clause: query.return_clause.clone(),
        }));

        self.check_cardinality(root.estimated_rows())?;

        Ok(ExecutionPlan {
            estimated_rows: root.estimated_rows(),
            root,
            diagnostics: self.diagnostics,
        })
    }

    fn plan_match_clause(
        &mut self,
        current: Option<PlanOperator>,
        clause: &MatchClause,
        query: &Query,
    ) -> Result<PlanOperator, CypherError> {
        let mut root = current;
        for pattern in &clause.patterns {
            root = Some(self.plan_path_pattern(root, clause.optional, pattern, query)?);
        }
        root.ok_or_else(|| CypherError::UnsupportedQueryPlan("empty MATCH clause".to_string()))
    }

    fn plan_path_pattern(
        &mut self,
        current: Option<PlanOperator>,
        optional: bool,
        pattern: &PathPattern,
        query: &Query,
    ) -> Result<PlanOperator, CypherError> {
        let start_var = pattern.start.variable.as_deref();
        let mut root = if let Some(var) = start_var {
            match current {
                Some(plan) if self.bindings.contains_key(var) => plan,
                _ => self.plan_node_access(
                    &pattern.start,
                    query.where_clause.as_ref(),
                    &query.order_by,
                )?,
            }
        } else {
            self.plan_node_access(&pattern.start, query.where_clause.as_ref(), &query.order_by)?
        };

        self.bind_node(&pattern.start);

        for step in &pattern.steps {
            self.check_hop_range(step.relationship.range)?;
            let max_depth = step.relationship.range.map_or(1, |range| range.max);
            let min_depth = step.relationship.range.map_or(1, |range| range.min);
            let estimated_rows = root
                .estimated_rows()
                .saturating_mul(u64::from(max_depth).max(1));
            let target_label = step.node.label.clone();
            root = PlanOperator::Expand(Box::new(ExpandPlan {
                input: root,
                source_alias: pattern.start.variable.clone(),
                target_alias: step.node.variable.clone(),
                target_label,
                relationship_types: step.relationship.types.clone(),
                direction: step.relationship.direction,
                link_access: link_access(step.relationship.direction),
                min_depth,
                max_depth,
                optional,
                estimated_rows,
            }));
            self.bind_node(&step.node);
            self.check_cardinality(estimated_rows)?;
        }

        Ok(root)
    }

    fn plan_node_access(
        &mut self,
        node: &NodePattern,
        where_clause: Option<&Expression>,
        order_by: &[SortItem],
    ) -> Result<PlanOperator, CypherError> {
        let label = node.label.as_deref().ok_or_else(|| {
            CypherError::UnsupportedQueryPlan(
                "untyped node scans are not supported by the planner".to_string(),
            )
        })?;
        let label_def = self
            .schema
            .label(label)
            .ok_or_else(|| CypherError::UnknownIdentifier {
                kind: "label",
                name: label.to_string(),
            })?;
        let alias = node.variable.as_deref();
        let indexed_predicate = Self::indexed_predicate(node, where_clause, label_def, alias);

        if let Some(predicate) = indexed_predicate {
            let index = label_def
                .index(&predicate.property)
                .expect("predicate checked index");
            let estimated_rows = estimate_index_rows(index, predicate.comparison);
            self.check_cardinality(estimated_rows)?;
            return Ok(PlanOperator::IndexLookup(Box::new(IndexLookupPlan {
                alias: node.variable.clone(),
                label: label.to_string(),
                collection: label_def.collection_name.clone(),
                property: predicate.property.clone(),
                comparison: Some(predicate.comparison),
                value: Some(predicate.value),
                ordered_by: covered_order(alias, order_by, &predicate.property),
                unique: index.unique,
                estimated_rows,
            })));
        }

        if let Some(order) = index_order_for_node(alias, order_by, label_def) {
            let estimated_rows = label_def.estimated_count;
            self.check_cardinality(estimated_rows)?;
            let unique = label_def.unique_index(&order.property);
            return Ok(PlanOperator::IndexLookup(Box::new(IndexLookupPlan {
                alias: node.variable.clone(),
                label: label.to_string(),
                collection: label_def.collection_name.clone(),
                property: order.property.clone(),
                comparison: None,
                value: None,
                ordered_by: vec![order],
                unique,
                estimated_rows,
            })));
        }

        self.ensure_scan_allowed(label, label_def)?;
        Ok(PlanOperator::Scan(Box::new(ScanPlan {
            alias: node.variable.clone(),
            label: label.to_string(),
            collection: label_def.collection_name.clone(),
            predicate: node_predicate(node, where_clause, alias),
            estimated_rows: label_def.estimated_count,
        })))
    }

    fn indexed_predicate(
        node: &NodePattern,
        where_clause: Option<&Expression>,
        label_def: &LabelDef,
        alias: Option<&str>,
    ) -> Option<IndexPredicate> {
        for property in &node.properties {
            if label_def.is_indexed(&property.key) {
                return Some(IndexPredicate {
                    property: property.key.clone(),
                    comparison: IndexComparison::Eq,
                    value: property.value.clone(),
                });
            }
        }
        alias.and_then(|var| find_indexable_predicate(where_clause?, var, label_def))
    }

    fn ensure_scan_allowed(&self, label: &str, label_def: &LabelDef) -> Result<(), CypherError> {
        if label_def.estimated_count > self.config.unindexed_scan_threshold {
            return Err(CypherError::UnsupportedQueryPlan(format!(
                "missing-index diagnostic: unindexed scan of {label} estimates {} rows, above threshold {}; declare an index on the filtered or sorted property",
                label_def.estimated_count, self.config.unindexed_scan_threshold
            )));
        }
        Ok(())
    }

    fn apply_where(
        &mut self,
        mut root: PlanOperator,
        where_clause: &Expression,
    ) -> Result<PlanOperator, CypherError> {
        for (negated, subquery) in exists_checks(where_clause) {
            let subplan = self.plan_exists_subquery(subquery)?;
            root = PlanOperator::ExistsCheck(Box::new(ExistsCheckPlan {
                estimated_rows: root.estimated_rows(),
                input: root,
                subquery: subplan,
                negated,
            }));
        }

        if let Some(predicate) = non_exists_predicate(where_clause) {
            root = PlanOperator::Filter(Box::new(FilterPlan {
                estimated_rows: root.estimated_rows(),
                input: root,
                predicate,
            }));
        }
        Ok(root)
    }

    fn plan_exists_subquery(&mut self, subquery: &Subquery) -> Result<ExistsSubplan, CypherError> {
        let Some(clause) = subquery.matches.first() else {
            return Err(CypherError::UnsupportedQueryPlan(
                "EXISTS subquery without MATCH cannot be planned".to_string(),
            ));
        };
        let Some(pattern) = clause.patterns.first() else {
            return Err(CypherError::UnsupportedQueryPlan(
                "EXISTS subquery without a path pattern cannot be planned".to_string(),
            ));
        };
        let Some(step) = pattern.steps.first() else {
            return Err(CypherError::UnsupportedQueryPlan(
                "EXISTS subquery must contain a relationship pattern".to_string(),
            ));
        };

        self.check_hop_range(step.relationship.range)?;
        let target_label = step.node.label.clone();
        let target_index_probe =
            self.exists_target_index_probe(&step.node, subquery.where_clause.as_ref())?;
        let estimated_rows = target_index_probe
            .as_ref()
            .map_or(1, |probe| probe.estimated_rows);
        self.check_cardinality(estimated_rows)?;

        Ok(ExistsSubplan {
            link_probe: LinkProbePlan {
                source_alias: pattern.start.variable.clone(),
                target_alias: step.node.variable.clone(),
                target_label,
                relationship_types: step.relationship.types.clone(),
                direction: step.relationship.direction,
                link_access: link_access(step.relationship.direction),
            },
            target_index_probe,
            target_filter: subquery.where_clause.clone(),
            estimated_rows,
        })
    }

    fn exists_target_index_probe(
        &self,
        target: &NodePattern,
        where_clause: Option<&Expression>,
    ) -> Result<Option<IndexLookupPlan>, CypherError> {
        let Some(label) = target.label.as_deref() else {
            return Ok(None);
        };
        let label_def = self
            .schema
            .label(label)
            .ok_or_else(|| CypherError::UnknownIdentifier {
                kind: "label",
                name: label.to_string(),
            })?;
        let alias = target.variable.as_deref();
        if let Some(predicate) = Self::indexed_predicate(target, where_clause, label_def, alias) {
            let index = label_def
                .index(&predicate.property)
                .expect("predicate checked index");
            return Ok(Some(IndexLookupPlan {
                alias: target.variable.clone(),
                label: label.to_string(),
                collection: label_def.collection_name.clone(),
                property: predicate.property,
                comparison: Some(predicate.comparison),
                value: Some(predicate.value),
                ordered_by: Vec::new(),
                unique: index.unique,
                estimated_rows: estimate_index_rows(index, predicate.comparison),
            }));
        }

        if contains_target_property_filter(where_clause, alias) {
            self.ensure_scan_allowed(label, label_def)?;
        }
        Ok(None)
    }

    fn bind_node(&mut self, node: &NodePattern) {
        if let Some(var) = &node.variable {
            self.bindings.insert(var.clone(), node.label.clone());
        }
    }

    fn check_hop_range(&self, range: Option<HopRange>) -> Result<(), CypherError> {
        if let Some(range) = range {
            if range.max > self.config.depth_cap {
                return Err(CypherError::UnsupportedQueryPlan(format!(
                    "variable-length path depth {} exceeds configured depth cap {}",
                    range.max, self.config.depth_cap
                )));
            }
        }
        Ok(())
    }

    fn check_cardinality(&self, estimated_rows: u64) -> Result<(), CypherError> {
        if self.config.enforce_cardinality_budget && estimated_rows > self.config.cardinality_budget
        {
            return Err(CypherError::QueryTooLarge(format!(
                "estimated intermediate rows {estimated_rows} exceed budget {}",
                self.config.cardinality_budget
            )));
        }
        Ok(())
    }
}

fn estimate_index_rows(index: &IndexedProperty, comparison: IndexComparison) -> u64 {
    match comparison {
        IndexComparison::Eq | IndexComparison::In | IndexComparison::Prefix => {
            index.estimated_equality_rows
        }
        IndexComparison::NotEq
        | IndexComparison::Lt
        | IndexComparison::LtEq
        | IndexComparison::Gt
        | IndexComparison::GtEq => index.estimated_range_rows,
    }
}

fn link_access(direction: Direction) -> LinkAccess {
    match direction {
        Direction::Outgoing => LinkAccess::OutgoingPrimaryKey,
        Direction::Incoming => LinkAccess::IncomingTargetIndex,
        Direction::Either => LinkAccess::BidirectionalIndexProbe,
    }
}

fn find_indexable_predicate(
    expr: &Expression,
    variable: &str,
    label_def: &LabelDef,
) -> Option<IndexPredicate> {
    match expr {
        Expression::BinaryLogical {
            op: crate::ast::LogicalOp::And,
            left,
            right,
        } => find_indexable_predicate(left, variable, label_def)
            .or_else(|| find_indexable_predicate(right, variable, label_def)),
        Expression::Comparison { op, left, right } => {
            comparison_predicate(*op, left, right, variable, label_def)
                .or_else(|| reversed_comparison_predicate(*op, left, right, variable, label_def))
        }
        _ => None,
    }
}

fn comparison_predicate(
    op: ComparisonOp,
    left: &Expression,
    right: &Expression,
    variable: &str,
    label_def: &LabelDef,
) -> Option<IndexPredicate> {
    let Expression::Property {
        variable: found,
        path,
    } = left
    else {
        return None;
    };
    if found != variable {
        return None;
    }
    let property = path.first()?;
    if !label_def.is_indexed(property) || !index_value_supported(right) {
        return None;
    }
    let comparison = index_comparison(op)?;
    Some(IndexPredicate {
        property: property.clone(),
        comparison,
        value: right.clone(),
    })
}

fn reversed_comparison_predicate(
    op: ComparisonOp,
    left: &Expression,
    right: &Expression,
    variable: &str,
    label_def: &LabelDef,
) -> Option<IndexPredicate> {
    let Expression::Property {
        variable: found,
        path,
    } = right
    else {
        return None;
    };
    if found != variable {
        return None;
    }
    let property = path.first()?;
    if !label_def.is_indexed(property) || !index_value_supported(left) {
        return None;
    }
    let comparison = reverse_index_comparison(op)?;
    Some(IndexPredicate {
        property: property.clone(),
        comparison,
        value: left.clone(),
    })
}

fn index_comparison(op: ComparisonOp) -> Option<IndexComparison> {
    match op {
        ComparisonOp::Eq => Some(IndexComparison::Eq),
        ComparisonOp::NotEq => Some(IndexComparison::NotEq),
        ComparisonOp::Lt => Some(IndexComparison::Lt),
        ComparisonOp::LtEq => Some(IndexComparison::LtEq),
        ComparisonOp::Gt => Some(IndexComparison::Gt),
        ComparisonOp::GtEq => Some(IndexComparison::GtEq),
        ComparisonOp::In => Some(IndexComparison::In),
        ComparisonOp::StartsWith => Some(IndexComparison::Prefix),
        ComparisonOp::Contains | ComparisonOp::EndsWith => None,
    }
}

fn reverse_index_comparison(op: ComparisonOp) -> Option<IndexComparison> {
    match op {
        ComparisonOp::Eq => Some(IndexComparison::Eq),
        ComparisonOp::NotEq => Some(IndexComparison::NotEq),
        ComparisonOp::Lt => Some(IndexComparison::Gt),
        ComparisonOp::LtEq => Some(IndexComparison::GtEq),
        ComparisonOp::Gt => Some(IndexComparison::Lt),
        ComparisonOp::GtEq => Some(IndexComparison::LtEq),
        ComparisonOp::In
        | ComparisonOp::Contains
        | ComparisonOp::StartsWith
        | ComparisonOp::EndsWith => None,
    }
}

fn index_value_supported(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Literal(
            Literal::Bool(_) | Literal::Float(_) | Literal::Integer(_) | Literal::String(_)
        ) | Expression::Parameter(_)
    )
}

fn order_covered_by_existing_index(root: &PlanOperator, order_by: &[SortItem]) -> bool {
    if order_by.is_empty() {
        return true;
    }
    let PlanOperator::IndexLookup(plan) = root else {
        return false;
    };
    plan.ordered_by.len() == order_by.len()
}

fn index_order_for_node(
    alias: Option<&str>,
    order_by: &[SortItem],
    label_def: &LabelDef,
) -> Option<IndexOrder> {
    let property = order_by_property(order_by)?;
    let variable = order_by_variable(order_by)?;
    if Some(variable.as_str()) != alias || !label_def.is_indexed(&property) {
        return None;
    }
    Some(IndexOrder {
        property,
        descending: order_by.first().is_some_and(|item| item.descending),
    })
}

fn order_by_property(order_by: &[SortItem]) -> Option<String> {
    match &order_by.first()?.expression {
        Expression::Property { path, .. } if order_by.len() == 1 => path.first().cloned(),
        _ => None,
    }
}

fn order_by_variable(order_by: &[SortItem]) -> Option<String> {
    match &order_by.first()?.expression {
        Expression::Property { variable, .. } if order_by.len() == 1 => Some(variable.clone()),
        _ => None,
    }
}

fn covered_order(alias: Option<&str>, order_by: &[SortItem], property: &str) -> Vec<IndexOrder> {
    let Some(order_property) = order_by_property(order_by) else {
        return Vec::new();
    };
    let Some(order_variable) = order_by_variable(order_by) else {
        return Vec::new();
    };
    if Some(order_variable.as_str()) != alias || order_property != property {
        return Vec::new();
    }
    vec![IndexOrder {
        property: order_property,
        descending: order_by.first().is_some_and(|item| item.descending),
    }]
}

fn node_predicate(
    node: &NodePattern,
    where_clause: Option<&Expression>,
    alias: Option<&str>,
) -> Option<Expression> {
    node.properties
        .first()
        .map(|property| Expression::Comparison {
            op: ComparisonOp::Eq,
            left: Box::new(Expression::Property {
                variable: alias.unwrap_or_default().to_string(),
                path: vec![property.key.clone()],
            }),
            right: Box::new(property.value.clone()),
        })
        .or_else(|| where_clause.cloned())
}

fn exists_checks(expr: &Expression) -> Vec<(bool, &Subquery)> {
    match expr {
        Expression::Exists(subquery) => vec![(false, subquery)],
        Expression::NotExists(subquery) => vec![(true, subquery)],
        Expression::BinaryLogical {
            op: crate::ast::LogicalOp::And,
            left,
            right,
        } => {
            let mut checks = exists_checks(left);
            checks.extend(exists_checks(right));
            checks
        }
        _ => Vec::new(),
    }
}

fn non_exists_predicate(expr: &Expression) -> Option<Expression> {
    match expr {
        Expression::Exists(_) | Expression::NotExists(_) => None,
        Expression::BinaryLogical {
            op: crate::ast::LogicalOp::And,
            left,
            right,
        } => match (non_exists_predicate(left), non_exists_predicate(right)) {
            (Some(left), Some(right)) => Some(Expression::BinaryLogical {
                op: crate::ast::LogicalOp::And,
                left: Box::new(left),
                right: Box::new(right),
            }),
            (Some(expr), None) | (None, Some(expr)) => Some(expr),
            (None, None) => None,
        },
        _ => Some(expr.clone()),
    }
}

fn contains_target_property_filter(where_clause: Option<&Expression>, alias: Option<&str>) -> bool {
    let Some(expr) = where_clause else {
        return false;
    };
    let Some(alias) = alias else {
        return false;
    };
    expression_mentions_variable_property(expr, alias)
}

fn expression_mentions_variable_property(expr: &Expression, alias: &str) -> bool {
    match expr {
        Expression::Property { variable, .. } => variable == alias,
        Expression::BinaryLogical { left, right, .. }
        | Expression::Comparison { left, right, .. } => {
            expression_mentions_variable_property(left, alias)
                || expression_mentions_variable_property(right, alias)
        }
        Expression::Not(inner) => expression_mentions_variable_property(inner, alias),
        Expression::IsNull { expression, .. } => {
            expression_mentions_variable_property(expression, alias)
        }
        Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::Parameter(_)
        | Expression::Exists(_)
        | Expression::NotExists(_)
        | Expression::FunctionCall { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::Parser;
    use crate::schema::{test_fixtures, LabelDef, PropertyKind, RelationshipDef};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    fn parse_and_plan(input: &str, schema: &SchemaSnapshot) -> Result<ExecutionPlan, CypherError> {
        let tokens = tokenize(input)?;
        let query = Parser::new(tokens).parse_query()?;
        plan(&query, schema)
    }

    fn unwrap_project(plan: &ExecutionPlan) -> &PlanOperator {
        match &plan.root {
            PlanOperator::Project(project) => &project.input,
            other => panic!("expected Project root, got {other:?}"),
        }
    }

    fn schema_with_large_unindexed_collection() -> SchemaSnapshot {
        let mut schema = test_fixtures::ddx_beads();
        let label = schema.labels.get_mut("DdxBead").expect("fixture label");
        label
            .properties
            .insert("owner".to_string(), PropertyKind::String);
        label
            .indexed_properties
            .retain(|index| index.property != "status");
        schema
    }

    #[test]
    fn index_hit_for_inline_label_property_predicate() {
        let schema = test_fixtures::ddx_beads();
        let plan = parse_and_plan("MATCH (b:DdxBead {status: 'open'}) RETURN b", &schema).unwrap();

        match unwrap_project(&plan) {
            PlanOperator::IndexLookup(index) => {
                assert_eq!(index.label, "DdxBead");
                assert_eq!(index.property, "status");
                assert_eq!(index.comparison, Some(IndexComparison::Eq));
            }
            other => panic!("expected IndexLookup, got {other:?}"),
        }
    }

    #[test]
    fn range_scan_for_indexed_comparison() {
        let schema = test_fixtures::ddx_beads();
        let plan =
            parse_and_plan("MATCH (b:DdxBead) WHERE b.priority > 3 RETURN b", &schema).unwrap();

        match unwrap_project(&plan) {
            PlanOperator::Filter(filter) => match &filter.input {
                PlanOperator::IndexLookup(index) => {
                    assert_eq!(index.property, "priority");
                    assert_eq!(index.comparison, Some(IndexComparison::Gt));
                }
                other => panic!("expected IndexLookup below Filter, got {other:?}"),
            },
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn sort_via_index_omits_sort_operator() {
        let schema = test_fixtures::ddx_beads();
        let plan = parse_and_plan(
            "MATCH (b:DdxBead) RETURN b ORDER BY b.priority DESC",
            &schema,
        )
        .unwrap();

        match unwrap_project(&plan) {
            PlanOperator::IndexLookup(index) => {
                assert_eq!(index.property, "priority");
                assert_eq!(
                    index.ordered_by,
                    vec![IndexOrder {
                        property: "priority".to_string(),
                        descending: true
                    }]
                );
            }
            other => panic!("expected ordered IndexLookup, got {other:?}"),
        }
    }

    #[test]
    fn ddx_ready_query_uses_status_index_for_outer_match_and_exists_target() {
        let schema = test_fixtures::ddx_beads();
        let plan = parse_and_plan(
            r"
            MATCH (b:DdxBead {status: 'open'})
            WHERE NOT EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
                WHERE d.status <> 'closed'
            }
            RETURN b
            ORDER BY b.priority DESC, b.updated_at DESC
            LIMIT 100
            ",
            &schema,
        )
        .unwrap();

        let PlanOperator::Limit(limit) = unwrap_project(&plan) else {
            panic!("expected Limit below Project");
        };
        let PlanOperator::Sort(sort) = &limit.input else {
            panic!("expected Sort below Limit");
        };
        let PlanOperator::ExistsCheck(exists) = &sort.input else {
            panic!("expected ExistsCheck below Sort");
        };
        assert!(exists.negated);
        let PlanOperator::IndexLookup(outer) = &exists.input else {
            panic!("expected outer IndexLookup");
        };
        assert_eq!(outer.property, "status");
        let target = exists
            .subquery
            .target_index_probe
            .as_ref()
            .expect("target index probe");
        assert_eq!(target.property, "status");
        assert_eq!(target.comparison, Some(IndexComparison::NotEq));
        assert_eq!(
            exists.subquery.link_probe.link_access,
            LinkAccess::OutgoingPrimaryKey
        );
    }

    #[test]
    fn optional_match_plans_optional_expand() {
        let schema = test_fixtures::ddx_beads();
        let plan = parse_and_plan(
            r"
            MATCH (b:DdxBead {id: 'root'})
            OPTIONAL MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
            RETURN b, d
            ",
            &schema,
        )
        .unwrap();

        match unwrap_project(&plan) {
            PlanOperator::Expand(expand) => {
                assert!(expand.optional);
                assert_eq!(expand.link_access, LinkAccess::OutgoingPrimaryKey);
            }
            other => panic!("expected optional Expand, got {other:?}"),
        }
    }

    #[test]
    fn fallback_scan_rejected_above_threshold_with_missing_index_diagnostic() {
        let schema = schema_with_large_unindexed_collection();
        let err = parse_and_plan("MATCH (b:DdxBead) WHERE b.owner = 'erik' RETURN b", &schema)
            .unwrap_err();

        assert!(matches!(err, CypherError::UnsupportedQueryPlan(message)
            if message.contains("missing-index diagnostic")
                && message.contains("DdxBead")
                && message.contains("threshold 1000")));
    }

    #[test]
    fn variable_length_depth_cap_rejected() {
        let schema = test_fixtures::ddx_beads();
        let err = parse_and_plan(
            "MATCH (b:DdxBead {id: 'root'})-[:DEPENDS_ON*1..11]->(d:DdxBead) RETURN d",
            &schema,
        )
        .unwrap_err();

        assert!(matches!(err, CypherError::UnsupportedQueryPlan(message)
            if message.contains("depth cap 10")));
    }

    #[test]
    fn cardinality_budget_rejected_from_index_stats() {
        let mut schema = test_fixtures::ddx_beads();
        schema
            .labels
            .get_mut("DdxBead")
            .expect("fixture label")
            .indexed_properties
            .iter_mut()
            .find(|index| index.property == "priority")
            .expect("priority index")
            .estimated_range_rows = 1_000_001;

        let err =
            parse_and_plan("MATCH (b:DdxBead) WHERE b.priority > 1 RETURN b", &schema).unwrap_err();

        assert!(matches!(err, CypherError::QueryTooLarge(message)
            if message.contains("1000001") || message.contains("1_000_001")));
    }

    #[test]
    fn execution_plan_serializes_to_json() {
        let schema = test_fixtures::ddx_beads();
        let plan = parse_and_plan("MATCH (b:DdxBead {status: 'open'}) RETURN b", &schema).unwrap();
        let json = serde_json::to_value(&plan).unwrap();

        assert_eq!(json["root"]["operator"], "Project");
    }

    #[test]
    fn small_unindexed_collection_can_scan_with_predicate_pushdown() {
        let mut labels = BTreeMap::new();
        let mut properties = BTreeMap::new();
        properties.insert("owner".to_string(), PropertyKind::String);
        labels.insert(
            "Small".to_string(),
            LabelDef {
                collection_name: "small".to_string(),
                estimated_count: 20,
                properties,
                indexed_properties: Vec::new(),
            },
        );
        let schema = SchemaSnapshot {
            labels,
            relationships: BTreeMap::from([(
                "LINK".to_string(),
                RelationshipDef {
                    source_labels: vec!["Small".to_string()],
                    target_labels: vec!["Small".to_string()],
                },
            )]),
            planner_config: PlannerConfig::default(),
        };

        let plan = parse_and_plan("MATCH (s:Small) WHERE s.owner = 'a' RETURN s", &schema).unwrap();
        match unwrap_project(&plan) {
            PlanOperator::Filter(filter) => match &filter.input {
                PlanOperator::Scan(scan) => assert_eq!(scan.estimated_rows, 20),
                other => panic!("expected Scan below Filter, got {other:?}"),
            },
            other => panic!("expected Filter, got {other:?}"),
        }
    }

    #[test]
    fn named_query_budget_override_can_accept_large_estimate() {
        let mut schema = test_fixtures::ddx_beads();
        schema.planner_config.enforce_cardinality_budget = false;
        schema
            .labels
            .get_mut("DdxBead")
            .expect("fixture label")
            .indexed_properties
            .iter_mut()
            .find(|index| index.property == "priority")
            .expect("priority index")
            .estimated_range_rows = 2_000_000;

        parse_and_plan("MATCH (b:DdxBead) WHERE b.priority > 1 RETURN b", &schema).unwrap();
    }
}
