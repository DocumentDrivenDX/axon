//! Streaming executor for planner output over [`QueryStore`].
//!
//! The executor keeps non-materializing operators as pull-based iterators:
//! scans, index lookups, filters, expands, exists checks, skip, limit, and
//! projection only advance their input as rows are requested. Sort remains a
//! materializing operator by definition.

use crate::ast::{ComparisonOp, Direction, Expression, FunctionArg, Literal, LogicalOp, SortItem};
use crate::error::CypherError;
use crate::memory_store::{EntityScan, LinkTraversal, QueryEntity, QueryLink, QueryStore};
use crate::planner::{
    ExecutionPlan, ExistsSubplan, ExpandPlan, IndexComparison, IndexLookupPlan, PlanOperator,
    ProjectPlan,
};
use serde_json::{Map, Number, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);

/// A projected result row returned by Cypher execution.
pub type Row = BTreeMap<String, Value>;

type BindingRow = BTreeMap<String, BindingValue>;
type BindingStream<'a> = Box<dyn Iterator<Item = Result<BindingRow, CypherError>> + 'a>;
type RowIter<'a> = Box<dyn Iterator<Item = Result<Row, CypherError>> + 'a>;

/// Clock abstraction used to make the execution timeout testable.
pub trait ExecutionClock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
pub struct SystemExecutionClock;

impl ExecutionClock for SystemExecutionClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Executor options. Defaults enforce the V1 30-second wall-clock budget.
#[derive(Clone)]
pub struct ExecutionOptions {
    pub timeout: Duration,
    pub clock: Arc<dyn ExecutionClock>,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_EXECUTION_TIMEOUT,
            clock: Arc::new(SystemExecutionClock),
        }
    }
}

/// Pull-based stream of projected query rows.
pub struct RowStream<'a> {
    inner: RowIter<'a>,
    deadline: Instant,
    clock: Arc<dyn ExecutionClock>,
}

impl Iterator for RowStream<'_> {
    type Item = Result<Row, CypherError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.clock.now() >= self.deadline {
            return Some(Err(CypherError::QueryTimeout(
                "execution exceeded 30-second wall-clock budget".to_string(),
            )));
        }
        self.inner.next()
    }
}

/// Execute a planned query over a query store using the default 30-second
/// execution timeout.
pub fn execute<'a, S>(plan: &'a ExecutionPlan, store: &'a S) -> RowStream<'a>
where
    S: QueryStore + ?Sized,
{
    execute_with_options(plan, store, ExecutionOptions::default())
}

/// Execute with injected options. This is public so tests and embedding
/// surfaces can use shorter deadlines or deterministic clocks.
pub fn execute_with_options<'a, S>(
    plan: &'a ExecutionPlan,
    store: &'a S,
    options: ExecutionOptions,
) -> RowStream<'a>
where
    S: QueryStore + ?Sized,
{
    let start = options.clock.now();
    let deadline = start.checked_add(options.timeout).unwrap_or(start);
    RowStream {
        inner: row_stream(&plan.root, store),
        deadline,
        clock: options.clock,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum BindingValue {
    Entity(QueryEntity),
    Null,
}

fn row_stream<'a, S>(operator: &'a PlanOperator, store: &'a S) -> RowIter<'a>
where
    S: QueryStore + ?Sized,
{
    match operator {
        PlanOperator::Project(plan) => project_stream(plan, store),
        other => Box::new(binding_stream(other, store).map(|row| row.map(project_all_bindings))),
    }
}

fn binding_stream<'a, S>(operator: &'a PlanOperator, store: &'a S) -> BindingStream<'a>
where
    S: QueryStore + ?Sized,
{
    match operator {
        PlanOperator::Scan(plan) => {
            let scan = EntityScan {
                label: Some(plan.label.clone()),
                property_filters: Vec::new(),
            };
            let alias = plan.alias.clone();
            let predicate = plan.predicate.clone();
            Box::new(store.scan_entities(scan).filter_map(move |entity| {
                let mut row = BindingRow::new();
                bind_entity(&mut row, alias.as_deref(), entity);
                match predicate_matches(predicate.as_ref(), &row) {
                    Ok(true) => Some(Ok(row)),
                    Ok(false) => None,
                    Err(err) => Some(Err(err)),
                }
            }))
        }
        PlanOperator::IndexLookup(plan) => index_lookup_stream(plan, store),
        PlanOperator::Expand(plan) => expand_stream(plan, store),
        PlanOperator::Filter(plan) => {
            let predicate = plan.predicate.clone();
            Box::new(
                binding_stream(&plan.input, store).filter_map(move |row| match row {
                    Ok(row) => match eval_truthy(&predicate, &row) {
                        Ok(true) => Some(Ok(row)),
                        Ok(false) => None,
                        Err(err) => Some(Err(err)),
                    },
                    Err(err) => Some(Err(err)),
                }),
            )
        }
        PlanOperator::Project(plan) => binding_stream(&plan.input, store),
        PlanOperator::Sort(plan) => sort_stream(plan, store),
        PlanOperator::Skip(plan) => {
            let count = usize::try_from(plan.count).unwrap_or(usize::MAX);
            Box::new(binding_stream(&plan.input, store).skip(count))
        }
        PlanOperator::Limit(plan) => {
            let count = usize::try_from(plan.count).unwrap_or(usize::MAX);
            Box::new(binding_stream(&plan.input, store).take(count))
        }
        PlanOperator::ExistsCheck(plan) => {
            let subquery = plan.subquery.clone();
            let negated = plan.negated;
            Box::new(
                binding_stream(&plan.input, store).filter_map(move |row| match row {
                    Ok(row) => match exists_for_row(&subquery, &row, store) {
                        Ok(exists) if exists != negated => Some(Ok(row)),
                        Ok(_) => None,
                        Err(err) => Some(Err(err)),
                    },
                    Err(err) => Some(Err(err)),
                }),
            )
        }
    }
}

fn index_lookup_stream<'a, S>(plan: &'a IndexLookupPlan, store: &'a S) -> BindingStream<'a>
where
    S: QueryStore + ?Sized,
{
    let mut scan = EntityScan {
        label: Some(plan.label.clone()),
        property_filters: Vec::new(),
    };
    if matches!(plan.comparison, Some(IndexComparison::Eq)) {
        if let Some(Ok(value)) = plan.value.as_ref().map(literal_only) {
            scan = scan.with_property_eq(plan.property.clone(), value);
        }
    }

    let alias = plan.alias.clone();
    let predicate = index_predicate_expression(plan);
    let stream_alias = alias.clone();
    let iter = store.scan_entities(scan).filter_map(move |entity| {
        let mut row = BindingRow::new();
        bind_entity(&mut row, stream_alias.as_deref(), entity);
        match predicate_matches(predicate.as_ref(), &row) {
            Ok(true) => Some(Ok(row)),
            Ok(false) => None,
            Err(err) => Some(Err(err)),
        }
    });

    if plan.ordered_by.is_empty() {
        Box::new(iter)
    } else {
        let items: Vec<SortItem> = plan
            .ordered_by
            .iter()
            .map(|order| SortItem {
                expression: Expression::Property {
                    variable: alias.clone().unwrap_or_default(),
                    path: vec![order.property.clone()],
                },
                descending: order.descending,
            })
            .collect();
        Box::new(materialize_sorted(iter, items).into_iter())
    }
}

fn expand_stream<'a, S>(plan: &'a ExpandPlan, store: &'a S) -> BindingStream<'a>
where
    S: QueryStore + ?Sized,
{
    let expand = plan.clone();
    Box::new(
        binding_stream(&plan.input, store).flat_map(move |row| match row {
            Ok(row) => expand_row(&expand, &row, store),
            Err(err) => vec![Err(err)],
        }),
    )
}

fn sort_stream<'a, S>(plan: &'a crate::planner::SortPlan, store: &'a S) -> BindingStream<'a>
where
    S: QueryStore + ?Sized,
{
    let rows = materialize_sorted(binding_stream(&plan.input, store), plan.items.clone());
    Box::new(rows.into_iter())
}

fn materialize_sorted(
    iter: impl Iterator<Item = Result<BindingRow, CypherError>>,
    items: Vec<SortItem>,
) -> Vec<Result<BindingRow, CypherError>> {
    let mut rows = Vec::new();
    for row in iter {
        match row {
            Ok(row) => rows.push(row),
            Err(err) => return vec![Err(err)],
        }
    }
    rows.sort_by(|left, right| compare_rows(left, right, &items));
    rows.into_iter().map(Ok).collect()
}

fn project_stream<'a, S>(plan: &'a ProjectPlan, store: &'a S) -> RowIter<'a>
where
    S: QueryStore + ?Sized,
{
    let return_clause = plan.return_clause.clone();
    if return_clause
        .items
        .iter()
        .any(|item| is_count_star(&item.expression))
    {
        return count_star_stream(plan, store);
    }

    let projected = binding_stream(&plan.input, store).map(move |row| {
        row.and_then(|row| {
            let mut projected = Row::new();
            for item in &return_clause.items {
                let key = item
                    .alias
                    .clone()
                    .unwrap_or_else(|| expression_name(&item.expression));
                projected.insert(key, eval_json(&item.expression, &row)?);
            }
            Ok(projected)
        })
    });

    if plan.return_clause.distinct {
        Box::new(DistinctRows {
            input: Box::new(projected),
            seen: BTreeSet::new(),
        })
    } else {
        Box::new(projected)
    }
}

fn count_star_stream<'a, S>(plan: &'a ProjectPlan, store: &'a S) -> RowIter<'a>
where
    S: QueryStore + ?Sized,
{
    if !plan
        .return_clause
        .items
        .iter()
        .all(|item| is_count_star(&item.expression))
    {
        let err = CypherError::UnsupportedQueryPlan(
            "count(*) cannot be mixed with non-aggregate return items yet".to_string(),
        );
        return Box::new(std::iter::once(Err(err)));
    }

    let mut count = 0_u64;
    for row in binding_stream(&plan.input, store) {
        match row {
            Ok(_) => count = count.saturating_add(1),
            Err(err) => return Box::new(std::iter::once(Err(err))),
        }
    }

    let mut projected = Row::new();
    for item in &plan.return_clause.items {
        let key = item
            .alias
            .clone()
            .unwrap_or_else(|| expression_name(&item.expression));
        projected.insert(key, Value::Number(Number::from(count)));
    }

    Box::new(std::iter::once(Ok(projected)))
}

fn is_count_star(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::FunctionCall { name, arguments }
            if name.eq_ignore_ascii_case("count")
                && matches!(arguments.as_slice(), [FunctionArg::Star])
    )
}

struct DistinctRows<'a> {
    input: RowIter<'a>,
    seen: BTreeSet<String>,
}

impl Iterator for DistinctRows<'_> {
    type Item = Result<Row, CypherError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let row = self.input.next()?;
            match row {
                Ok(row) => match serde_json::to_string(&row) {
                    Ok(key) => {
                        if self.seen.insert(key) {
                            return Some(Ok(row));
                        }
                    }
                    Err(err) => {
                        return Some(Err(CypherError::UnsupportedQueryPlan(format!(
                            "failed to encode DISTINCT row: {err}"
                        ))));
                    }
                },
                Err(err) => return Some(Err(err)),
            }
        }
    }
}

fn expand_row<S>(
    plan: &ExpandPlan,
    input: &BindingRow,
    store: &S,
) -> Vec<Result<BindingRow, CypherError>>
where
    S: QueryStore + ?Sized,
{
    let Some(source_alias) = plan.source_alias.as_deref() else {
        return vec![Err(CypherError::UnsupportedQueryPlan(
            "Expand requires a source alias".to_string(),
        ))];
    };
    let Some(source) = bound_entity(input, source_alias) else {
        return vec![Err(CypherError::UnsupportedQueryPlan(format!(
            "Expand source alias {source_alias} is not bound to an entity"
        )))];
    };

    let mut output = Vec::new();
    let mut frontier = VecDeque::from([(source.id.clone(), 0_u32)]);
    let mut matched = false;

    while let Some((anchor_id, depth)) = frontier.pop_front() {
        if depth >= plan.max_depth {
            continue;
        }
        let traversal = LinkTraversal {
            anchor_id,
            direction: plan.direction,
            relationship_types: plan.relationship_types.clone(),
            link_property_filters: Vec::new(),
        };
        for link in store.traverse_links(traversal) {
            let next_id = other_entity_id(&link, plan.direction);
            let Some(entity) = store.get_entity(next_id).cloned() else {
                continue;
            };
            if !target_label_matches(&entity, plan.target_label.as_deref()) {
                continue;
            }
            let next_depth = depth + 1;
            if next_depth >= plan.min_depth {
                let mut row = input.clone();
                bind_entity(&mut row, plan.target_alias.as_deref(), entity.clone());
                matched = true;
                output.push(Ok(row));
            }
            if next_depth < plan.max_depth {
                frontier.push_back((entity.id, next_depth));
            }
        }
    }

    if plan.optional && !matched {
        let mut row = input.clone();
        if let Some(alias) = &plan.target_alias {
            row.insert(alias.clone(), BindingValue::Null);
        }
        output.push(Ok(row));
    }
    output
}

fn exists_for_row<S>(
    subquery: &ExistsSubplan,
    input: &BindingRow,
    store: &S,
) -> Result<bool, CypherError>
where
    S: QueryStore + ?Sized,
{
    let Some(source_alias) = subquery.link_probe.source_alias.as_deref() else {
        return Err(CypherError::UnsupportedQueryPlan(
            "EXISTS link probe requires a source alias".to_string(),
        ));
    };
    let Some(source) = bound_entity(input, source_alias) else {
        return Ok(false);
    };

    let traversal = LinkTraversal {
        anchor_id: source.id.clone(),
        direction: subquery.link_probe.direction,
        relationship_types: subquery.link_probe.relationship_types.clone(),
        link_property_filters: Vec::new(),
    };
    for link in store.traverse_links(traversal) {
        let target_id = other_entity_id(&link, subquery.link_probe.direction);
        let Some(target) = store.get_entity(target_id).cloned() else {
            continue;
        };
        if !target_label_matches(&target, subquery.link_probe.target_label.as_deref()) {
            continue;
        }
        let mut row = input.clone();
        bind_entity(
            &mut row,
            subquery.link_probe.target_alias.as_deref(),
            target,
        );
        if predicate_matches(subquery.target_filter.as_ref(), &row)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn bind_entity(row: &mut BindingRow, alias: Option<&str>, entity: QueryEntity) {
    if let Some(alias) = alias {
        row.insert(alias.to_string(), BindingValue::Entity(entity));
    }
}

fn bound_entity<'a>(row: &'a BindingRow, alias: &str) -> Option<&'a QueryEntity> {
    match row.get(alias) {
        Some(BindingValue::Entity(entity)) => Some(entity),
        _ => None,
    }
}

fn target_label_matches(entity: &QueryEntity, label: Option<&str>) -> bool {
    label.map_or(true, |label| entity.has_label(label))
}

fn other_entity_id(link: &QueryLink, direction: Direction) -> &str {
    match direction {
        Direction::Outgoing => &link.target_id,
        Direction::Incoming => &link.source_id,
        Direction::Either => &link.target_id,
    }
}

fn predicate_matches(
    predicate: Option<&Expression>,
    row: &BindingRow,
) -> Result<bool, CypherError> {
    predicate.map_or(Ok(true), |predicate| eval_truthy(predicate, row))
}

fn index_predicate_expression(plan: &IndexLookupPlan) -> Option<Expression> {
    let (Some(comparison), Some(value), Some(alias)) =
        (plan.comparison, plan.value.clone(), plan.alias.clone())
    else {
        return None;
    };
    let op = match comparison {
        IndexComparison::Eq => ComparisonOp::Eq,
        IndexComparison::NotEq => ComparisonOp::NotEq,
        IndexComparison::Lt => ComparisonOp::Lt,
        IndexComparison::LtEq => ComparisonOp::LtEq,
        IndexComparison::Gt => ComparisonOp::Gt,
        IndexComparison::GtEq => ComparisonOp::GtEq,
        IndexComparison::In => ComparisonOp::In,
        IndexComparison::Prefix => ComparisonOp::StartsWith,
    };
    Some(Expression::Comparison {
        op,
        left: Box::new(Expression::Property {
            variable: alias,
            path: vec![plan.property.clone()],
        }),
        right: Box::new(value),
    })
}

fn eval_truthy(expr: &Expression, row: &BindingRow) -> Result<bool, CypherError> {
    Ok(eval_json(expr, row)?.as_bool().unwrap_or(false))
}

fn eval_json(expr: &Expression, row: &BindingRow) -> Result<Value, CypherError> {
    match expr {
        Expression::Literal(literal) => eval_literal(literal, row),
        Expression::Variable(name) => Ok(row.get(name).map_or(Value::Null, binding_to_json)),
        Expression::Property { variable, path } => Ok(row
            .get(variable)
            .and_then(|binding| binding_property(binding, path))
            .cloned()
            .unwrap_or(Value::Null)),
        Expression::Parameter(name) => Err(CypherError::UnsupportedQueryPlan(format!(
            "execution parameters are not available for ${name}"
        ))),
        Expression::Exists(_) | Expression::NotExists(_) => Err(CypherError::UnsupportedQueryPlan(
            "EXISTS expressions must be planned as ExistsCheck operators".to_string(),
        )),
        Expression::BinaryLogical { op, left, right } => {
            let left = eval_truthy(left, row)?;
            match op {
                LogicalOp::And if !left => Ok(Value::Bool(false)),
                LogicalOp::Or if left => Ok(Value::Bool(true)),
                LogicalOp::And => Ok(Value::Bool(eval_truthy(right, row)?)),
                LogicalOp::Or => Ok(Value::Bool(eval_truthy(right, row)?)),
            }
        }
        Expression::Not(inner) => Ok(Value::Bool(!eval_truthy(inner, row)?)),
        Expression::Comparison { op, left, right } => {
            let left = eval_json(left, row)?;
            let right = eval_json(right, row)?;
            Ok(Value::Bool(compare_values(*op, &left, &right)))
        }
        Expression::IsNull {
            expression,
            negated,
        } => {
            let is_null = eval_json(expression, row)?.is_null();
            Ok(Value::Bool(if *negated { !is_null } else { is_null }))
        }
        Expression::FunctionCall { name, arguments } => eval_function(name, arguments, row),
    }
}

fn eval_literal(literal: &Literal, row: &BindingRow) -> Result<Value, CypherError> {
    match literal {
        Literal::Null => Ok(Value::Null),
        Literal::Bool(value) => Ok(Value::Bool(*value)),
        Literal::Integer(value) => Ok(Value::Number(Number::from(*value))),
        Literal::Float(value) => Number::from_f64(*value).map_or_else(
            || {
                Err(CypherError::UnsupportedQueryPlan(format!(
                    "non-finite float literal {value}"
                )))
            },
            |number| Ok(Value::Number(number)),
        ),
        Literal::String(value) => Ok(Value::String(value.clone())),
        Literal::List(values) => values
            .iter()
            .map(|value| eval_json(value, row))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
    }
}

fn literal_only(expr: &Expression) -> Result<Value, CypherError> {
    eval_json(expr, &BindingRow::new())
}

fn eval_function(
    name: &str,
    arguments: &[FunctionArg],
    row: &BindingRow,
) -> Result<Value, CypherError> {
    match name.to_ascii_lowercase().as_str() {
        "id" if arguments.len() == 1 => match &arguments[0] {
            FunctionArg::Expression(Expression::Variable(variable)) => match row.get(variable) {
                Some(BindingValue::Entity(entity)) => Ok(Value::String(entity.id.clone())),
                _ => Ok(Value::Null),
            },
            _ => Err(CypherError::UnsupportedQueryPlan(
                "id() requires a variable argument".to_string(),
            )),
        },
        "type" if arguments.len() == 1 => match &arguments[0] {
            FunctionArg::Expression(Expression::Variable(_)) => Ok(Value::Null),
            _ => Err(CypherError::UnsupportedQueryPlan(
                "type() requires a relationship variable argument".to_string(),
            )),
        },
        "count" | "sum" | "avg" | "min" | "max" | "collect" => Err(
            CypherError::UnsupportedQueryPlan(format!(
                "materializing aggregate function {name}() is out of scope for the streaming executor core"
            )),
        ),
        _ => Err(CypherError::UnsupportedQueryPlan(format!(
            "unsupported function {name}()"
        ))),
    }
}

fn binding_property<'a>(binding: &'a BindingValue, path: &[String]) -> Option<&'a Value> {
    match binding {
        BindingValue::Entity(entity) => entity.property(path),
        BindingValue::Null => None,
    }
}

fn binding_to_json(binding: &BindingValue) -> Value {
    match binding {
        BindingValue::Entity(entity) => entity_to_json(entity),
        BindingValue::Null => Value::Null,
    }
}

fn entity_to_json(entity: &QueryEntity) -> Value {
    let properties = entity
        .properties
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<String, Value>>();
    let labels = entity
        .labels
        .iter()
        .cloned()
        .map(Value::String)
        .collect::<Vec<_>>();
    Value::Object(Map::from_iter([
        ("id".to_string(), Value::String(entity.id.clone())),
        ("labels".to_string(), Value::Array(labels)),
        ("properties".to_string(), Value::Object(properties)),
    ]))
}

fn project_all_bindings(row: BindingRow) -> Row {
    row.into_iter()
        .map(|(key, value)| (key, binding_to_json(&value)))
        .collect()
}

fn expression_name(expr: &Expression) -> String {
    match expr {
        Expression::Variable(name) => name.clone(),
        Expression::Property { variable, path } => format!("{}.{}", variable, path.join(".")),
        Expression::FunctionCall { name, .. } => name.clone(),
        Expression::Literal(_) => "literal".to_string(),
        Expression::Parameter(name) => format!("${name}"),
        Expression::Exists(_) => "exists".to_string(),
        Expression::NotExists(_) => "not_exists".to_string(),
        Expression::BinaryLogical { .. }
        | Expression::Not(_)
        | Expression::Comparison { .. }
        | Expression::IsNull { .. } => "expression".to_string(),
    }
}

fn compare_values(op: ComparisonOp, left: &Value, right: &Value) -> bool {
    match op {
        ComparisonOp::Eq => left == right,
        ComparisonOp::NotEq => left != right,
        ComparisonOp::Lt => value_ordering(left, right).is_some_and(|ordering| ordering.is_lt()),
        ComparisonOp::LtEq => value_ordering(left, right).is_some_and(|ordering| !ordering.is_gt()),
        ComparisonOp::Gt => value_ordering(left, right).is_some_and(|ordering| ordering.is_gt()),
        ComparisonOp::GtEq => value_ordering(left, right).is_some_and(|ordering| !ordering.is_lt()),
        ComparisonOp::In => match right {
            Value::Array(values) => values.iter().any(|value| value == left),
            _ => false,
        },
        ComparisonOp::Contains => {
            string_pair(left, right).is_some_and(|(left, right)| left.contains(right))
        }
        ComparisonOp::StartsWith => {
            string_pair(left, right).is_some_and(|(left, right)| left.starts_with(right))
        }
        ComparisonOp::EndsWith => {
            string_pair(left, right).is_some_and(|(left, right)| left.ends_with(right))
        }
    }
}

fn string_pair<'a>(left: &'a Value, right: &'a Value) -> Option<(&'a str, &'a str)> {
    Some((left.as_str()?, right.as_str()?))
}

fn value_ordering(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.as_f64()?.partial_cmp(&right.as_f64()?),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn compare_rows(left: &BindingRow, right: &BindingRow, items: &[SortItem]) -> Ordering {
    for item in items {
        let ordering = match (
            eval_json(&item.expression, left),
            eval_json(&item.expression, right),
        ) {
            (Ok(left), Ok(right)) => total_value_ordering(&left, &right),
            (Err(_), Ok(_)) => Ordering::Greater,
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Err(_)) => Ordering::Equal,
        };
        let ordering = if item.descending {
            ordering.reverse()
        } else {
            ordering
        };
        if !ordering.is_eq() {
            return ordering;
        }
    }
    Ordering::Equal
}

fn total_value_ordering(left: &Value, right: &Value) -> Ordering {
    value_rank(left)
        .cmp(&value_rank(right))
        .then_with(|| match (left, right) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
            (Value::Number(left), Value::Number(right)) => left
                .as_f64()
                .and_then(|left| right.as_f64().and_then(|right| left.partial_cmp(&right)))
                .unwrap_or(Ordering::Equal),
            (Value::String(left), Value::String(right)) => left.cmp(right),
            _ => left.to_string().cmp(&right.to_string()),
        })
}

fn value_rank(value: &Value) -> u8 {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_store::{PropertyMap, QueryEntity, QueryLink};
    use crate::schema::test_fixtures;
    use crate::{parse, plan, validate};
    use serde_json::json;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Mutex;

    fn properties(entries: impl IntoIterator<Item = (&'static str, Value)>) -> PropertyMap {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect()
    }

    fn store() -> crate::MemoryQueryStore {
        let mut store = crate::MemoryQueryStore::new();
        store.insert_entity(QueryEntity::new(
            "bead-a",
            ["DdxBead"],
            properties([
                ("status", json!("open")),
                ("priority", json!(5)),
                ("title", json!("first")),
                ("id", json!("bead-a")),
            ]),
        ));
        store.insert_entity(QueryEntity::new(
            "bead-b",
            ["DdxBead"],
            properties([
                ("status", json!("open")),
                ("priority", json!(1)),
                ("title", json!("second")),
                ("id", json!("bead-b")),
            ]),
        ));
        store.insert_entity(QueryEntity::new(
            "bead-c",
            ["DdxBead"],
            properties([
                ("status", json!("closed")),
                ("priority", json!(10)),
                ("title", json!("closed")),
                ("id", json!("bead-c")),
            ]),
        ));
        store.insert_link(QueryLink::new(
            "bead-a",
            "DEPENDS_ON",
            "bead-b",
            PropertyMap::new(),
        ));
        store
    }

    #[test]
    fn parse_validate_plan_execute_simple_match_where_return() {
        let schema = test_fixtures::ddx_beads();
        let query = parse(
            "MATCH (b:DdxBead {status: 'open'}) WHERE b.priority > 3 RETURN b.title AS title, b.priority AS priority",
        )
        .expect("query should parse");
        validate(&query, &schema).expect("query should validate");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows = execute(&plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("query should execute");

        assert_eq!(
            rows,
            vec![Row::from([
                ("priority".to_string(), json!(5)),
                ("title".to_string(), json!("first")),
            ])]
        );
    }

    #[test]
    fn expand_streams_relationship_targets() {
        let schema = test_fixtures::ddx_beads();
        let query = parse(
            "MATCH (b:DdxBead {id: 'bead-a'})-[:DEPENDS_ON]->(d:DdxBead) RETURN d.title AS title",
        )
        .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows = execute(&plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("query should execute");

        assert_eq!(
            rows,
            vec![Row::from([("title".to_string(), json!("second"))])]
        );
    }

    #[test]
    fn exists_check_filters_rows() {
        let schema = test_fixtures::ddx_beads();
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

        let rows = execute(&plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("query should execute");

        assert_eq!(
            rows,
            vec![Row::from([("title".to_string(), json!("first"))])]
        );
    }

    #[test]
    fn optional_match_preserves_unmatched_input_with_null_binding() {
        let schema = test_fixtures::ddx_beads();
        let query = parse(
            r"
            MATCH (b:DdxBead {id: 'bead-b'})
            OPTIONAL MATCH (b:DdxBead)-[:DEPENDS_ON]->(d:DdxBead)
            RETURN b.title AS source, d AS dependency
            ",
        )
        .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows = execute(&plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("query should execute");

        assert_eq!(
            rows,
            vec![Row::from([
                ("dependency".to_string(), Value::Null),
                ("source".to_string(), json!("second")),
            ])]
        );
    }

    #[test]
    fn exists_check_filters_true_and_false_predicate_cases() {
        let schema = test_fixtures::ddx_beads();
        let true_case = parse(
            r"
            MATCH (b:DdxBead {status: 'open'})
            WHERE EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
                WHERE d.status = 'open'
            }
            RETURN b.id AS id
            ",
        )
        .expect("true case should parse");
        let false_case = parse(
            r"
            MATCH (b:DdxBead {status: 'open'})
            WHERE NOT EXISTS {
                MATCH (b)-[:DEPENDS_ON]->(d:DdxBead)
                WHERE d.status = 'open'
            }
            RETURN b.id AS id
            ",
        )
        .expect("false case should parse");

        let true_plan = plan(&true_case, &schema).expect("true case should plan");
        let false_plan = plan(&false_case, &schema).expect("false case should plan");

        let true_rows = execute(&true_plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("true case should execute");
        let false_rows = execute(&false_plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("false case should execute");

        assert_eq!(
            true_rows,
            vec![Row::from([("id".to_string(), json!("bead-a"))])]
        );
        assert_eq!(
            false_rows,
            vec![Row::from([("id".to_string(), json!("bead-b"))])]
        );
    }

    #[test]
    fn count_star_counts_filtered_rows_without_materializing_result_rows() {
        let schema = test_fixtures::ddx_beads();
        let query = parse("MATCH (b:DdxBead {status: 'open'}) RETURN count(*) AS n")
            .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows = execute(&plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("query should execute");

        assert_eq!(rows, vec![Row::from([("n".to_string(), json!(2))])]);
    }

    #[test]
    fn distinct_returns_unique_projected_rows() {
        let schema = test_fixtures::ddx_beads();
        let query = parse("MATCH (b:DdxBead {status: 'open'}) RETURN DISTINCT b.status AS status")
            .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        let rows = execute(&plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("query should execute");

        assert_eq!(
            rows,
            vec![Row::from([("status".to_string(), json!("open"))])]
        );
    }

    #[test]
    fn order_by_asc_and_desc_execute_with_materialized_sort_when_uncovered() {
        let schema = test_fixtures::ddx_beads();
        let asc = parse(
            "MATCH (b:DdxBead {status: 'open'}) RETURN b.title AS title ORDER BY b.title ASC",
        )
        .expect("ASC query should parse");
        let desc = parse(
            "MATCH (b:DdxBead {status: 'open'}) RETURN b.title AS title ORDER BY b.title DESC",
        )
        .expect("DESC query should parse");
        let asc_plan = plan(&asc, &schema).expect("ASC query should plan");
        let desc_plan = plan(&desc, &schema).expect("DESC query should plan");

        match &asc_plan.root {
            PlanOperator::Project(project) => match &project.input {
                PlanOperator::Sort(_) => {}
                other => panic!("uncovered ORDER BY should materialize with Sort, got {other:?}"),
            },
            other => panic!("expected Project root, got {other:?}"),
        }

        let asc_rows = execute(&asc_plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("ASC query should execute");
        let desc_rows = execute(&desc_plan, &store())
            .collect::<Result<Vec<_>, _>>()
            .expect("DESC query should execute");

        assert_eq!(
            asc_rows,
            vec![
                Row::from([("title".to_string(), json!("first"))]),
                Row::from([("title".to_string(), json!("second"))]),
            ]
        );
        assert_eq!(
            desc_rows,
            vec![
                Row::from([("title".to_string(), json!("second"))]),
                Row::from([("title".to_string(), json!("first"))]),
            ]
        );
    }

    #[test]
    fn covered_order_by_uses_index_lookup_without_sort_operator() {
        let schema = test_fixtures::ddx_beads();
        let query = parse("MATCH (b:DdxBead) RETURN b.id AS id ORDER BY b.priority DESC")
            .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");

        match &plan.root {
            PlanOperator::Project(project) => match &project.input {
                PlanOperator::IndexLookup(index) => {
                    assert_eq!(index.property, "priority");
                    assert_eq!(index.ordered_by.len(), 1);
                    assert!(index.ordered_by[0].descending);
                }
                other => panic!("covered ORDER BY should stay as IndexLookup, got {other:?}"),
            },
            other => panic!("expected Project root, got {other:?}"),
        }
    }

    #[test]
    fn limit_does_not_pull_full_scan() {
        let plan = ExecutionPlan {
            root: PlanOperator::Project(Box::new(crate::planner::ProjectPlan {
                input: PlanOperator::Limit(Box::new(crate::planner::PagePlan {
                    input: PlanOperator::Scan(Box::new(crate::planner::ScanPlan {
                        alias: Some("b".to_string()),
                        label: "DdxBead".to_string(),
                        collection: "ddx_beads".to_string(),
                        predicate: None,
                        estimated_rows: 100,
                    })),
                    count: 2,
                    estimated_rows: 2,
                })),
                return_clause: crate::ast::ReturnClause {
                    distinct: false,
                    items: vec![crate::ast::ReturnItem {
                        expression: Expression::Variable("b".to_string()),
                        alias: None,
                    }],
                },
                estimated_rows: 2,
            })),
            estimated_rows: 2,
            diagnostics: Vec::new(),
        };
        let scanned = Rc::new(Cell::new(0));
        let store = CountingStore {
            scanned: Rc::clone(&scanned),
            total: 100,
        };

        let rows = execute(&plan, &store)
            .collect::<Result<Vec<_>, _>>()
            .expect("query should execute");

        assert_eq!(rows.len(), 2);
        assert_eq!(scanned.get(), 2);
    }

    #[test]
    fn timeout_is_checked_while_rows_are_pulled() {
        let schema = test_fixtures::ddx_beads();
        let query = parse("MATCH (b:DdxBead {status: 'open'}) RETURN b.title AS title")
            .expect("query should parse");
        let plan = plan(&query, &schema).expect("query should plan");
        let clock = Arc::new(ManualClock::new());
        let options = ExecutionOptions {
            timeout: Duration::from_secs(30),
            clock: clock.clone(),
        };
        let store = store();
        let mut rows = execute_with_options(&plan, &store, options);

        let first = rows.next().expect("first pull").expect("first row");
        assert_eq!(first.get("title"), Some(&json!("first")));
        clock.advance(Duration::from_secs(31));
        let second = rows
            .next()
            .expect("timeout result")
            .expect_err("second pull should time out");

        assert!(
            matches!(second, CypherError::QueryTimeout(message) if message.contains("30-second"))
        );
    }

    struct CountingStore {
        scanned: Rc<Cell<usize>>,
        total: usize,
    }

    impl QueryStore for CountingStore {
        fn get_entity(&self, id: &str) -> Option<&QueryEntity> {
            let _ = id;
            None
        }

        fn scan_entities(&self, scan: EntityScan) -> crate::memory_store::EntityStream<'_> {
            let scanned = Rc::clone(&self.scanned);
            Box::new((0..self.total).map(move |i| {
                scanned.set(scanned.get() + 1);
                QueryEntity::new(
                    format!("bead-{i}"),
                    [scan.label.clone().unwrap_or_else(|| "DdxBead".to_string())],
                    PropertyMap::new(),
                )
            }))
        }

        fn get_link(&self, id: &str) -> Option<&QueryLink> {
            let _ = id;
            None
        }

        fn traverse_links(&self, traversal: LinkTraversal) -> crate::memory_store::LinkStream<'_> {
            let _ = traversal;
            Box::new(std::iter::empty())
        }
    }

    struct ManualClock {
        current: Mutex<Instant>,
    }

    impl ManualClock {
        fn new() -> Self {
            Self {
                current: Mutex::new(Instant::now()),
            }
        }

        fn advance(&self, duration: Duration) {
            if let Ok(mut current) = self.current.lock() {
                if let Some(next) = current.checked_add(duration) {
                    *current = next;
                }
            }
        }
    }

    impl ExecutionClock for ManualClock {
        fn now(&self) -> Instant {
            let Ok(current) = self.current.lock() else {
                return Instant::now();
            };
            *current
        }
    }
}
