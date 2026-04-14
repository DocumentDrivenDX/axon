//! Validation rules: cross-field conditions with when/require pattern (ESF Layer 5).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A validation rule declared in the schema (ESF Layer 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRule {
    /// Unique name within the collection.
    pub name: String,

    /// Gate this rule belongs to. "save" blocks persistence.
    /// Custom gates allow save but track readiness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,

    /// If true, rule never blocks — always reports. Mutually exclusive with gate.
    #[serde(default)]
    pub advisory: bool,

    /// Condition that activates the rule. None = always active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<RuleCondition>,

    /// Constraint to enforce when active.
    pub require: RuleRequirement,

    /// Human-readable explanation of the business rule.
    pub message: String,

    /// Actionable fix suggestion. May include {field_name} placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// Condition that activates a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuleCondition {
    /// All sub-conditions must be true.
    All { all: Vec<RuleCondition> },
    /// Any sub-condition must be true.
    Any { any: Vec<RuleCondition> },
    /// Single field check.
    Field {
        field: String,
        #[serde(flatten)]
        op: ConditionOp,
    },
}

/// Operators for field-level conditions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Eq(Value),
    Ne(Value),
    In(Vec<Value>),
    NotNull(bool),
    IsNull(bool),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    #[serde(rename = "match")]
    Match(String),
}

/// Constraint to enforce when a rule is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleRequirement {
    /// The field to check.
    pub field: String,

    /// Condition the field must satisfy. Exactly one should be set.
    #[serde(flatten)]
    pub op: RequirementOp,
}

/// Operators for requirement constraints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementOp {
    NotNull(bool),
    Eq(Value),
    Ne(Value),
    In(Vec<Value>),
    GtField(String),
    GteField(String),
    LtField(String),
    LteField(String),
    #[serde(rename = "match")]
    Match(String),
    NotMatch(String),
    MinLength(usize),
    Lte(Value),
}

/// Result of evaluating a single rule against entity data.
///
/// Re-exported from `axon-core` so `Entity` can carry materialized gate
/// results as a first-class field (FEAT-019).
pub use axon_core::types::RuleViolation;

/// Evaluate a validation rule against entity data.
///
/// Returns `None` if the rule passes (condition not met, or requirement satisfied).
/// Returns `Some(RuleViolation)` if the rule fires and the requirement is not met.
pub fn evaluate_rule(rule: &ValidationRule, data: &Value) -> Option<RuleViolation> {
    // Check condition (when clause)
    if let Some(condition) = &rule.when {
        if !evaluate_condition(condition, data) {
            return None; // condition not met, rule doesn't fire
        }
    }

    // Check requirement
    if evaluate_requirement(&rule.require, data) {
        return None; // requirement satisfied
    }

    // Rule fires — build violation
    let fix = rule.fix.as_ref().map(|f| interpolate_fix(f, data));
    Some(RuleViolation {
        rule: rule.name.clone(),
        field: rule.require.field.clone(),
        message: rule.message.clone(),
        fix,
        gate: rule.gate.clone(),
        advisory: rule.advisory,
        context: rule.when.as_ref().map(|c| condition_context(c, data)),
    })
}

/// Evaluate all rules against entity data. Returns all violations.
pub fn evaluate_rules(rules: &[ValidationRule], data: &Value) -> Vec<RuleViolation> {
    rules
        .iter()
        .filter_map(|rule| evaluate_rule(rule, data))
        .collect()
}

// ── Condition evaluation ───────────────────────────────────────────────────

fn evaluate_condition(condition: &RuleCondition, data: &Value) -> bool {
    match condition {
        RuleCondition::All { all } => all.iter().all(|c| evaluate_condition(c, data)),
        RuleCondition::Any { any } => any.iter().any(|c| evaluate_condition(c, data)),
        RuleCondition::Field { field, op } => {
            let value = get_field(data, field);
            evaluate_condition_op(op, value)
        }
    }
}

fn evaluate_condition_op(op: &ConditionOp, value: Option<&Value>) -> bool {
    match op {
        ConditionOp::Eq(expected) => value == Some(expected),
        ConditionOp::Ne(expected) => value != Some(expected),
        ConditionOp::In(values) => value.is_some_and(|v| values.contains(v)),
        ConditionOp::NotNull(flag) => {
            let is_present = value.is_some_and(|v| !v.is_null());
            is_present == *flag
        }
        ConditionOp::IsNull(flag) => {
            let is_null = value.is_none() || value.is_some_and(|v| v.is_null());
            is_null == *flag
        }
        ConditionOp::Gt(expected) => {
            compare_values(value, Some(expected)) == Some(std::cmp::Ordering::Greater)
        }
        ConditionOp::Gte(expected) => matches!(
            compare_values(value, Some(expected)),
            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
        ),
        ConditionOp::Lt(expected) => {
            compare_values(value, Some(expected)) == Some(std::cmp::Ordering::Less)
        }
        ConditionOp::Lte(expected) => matches!(
            compare_values(value, Some(expected)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ),
        ConditionOp::Match(pattern) => value
            .and_then(|v| v.as_str())
            .and_then(|s| regex::Regex::new(pattern).ok().map(|r| r.is_match(s)))
            .unwrap_or(false),
    }
}

// ── Requirement evaluation ─────────────────────────────────────────────────

fn evaluate_requirement(req: &RuleRequirement, data: &Value) -> bool {
    let value = get_field(data, &req.field);
    match &req.op {
        RequirementOp::NotNull(flag) => {
            let is_present = value.is_some_and(|v| !v.is_null());
            is_present == *flag
        }
        RequirementOp::Eq(expected) => value == Some(expected),
        RequirementOp::Ne(expected) => value != Some(expected),
        RequirementOp::In(values) => value.is_some_and(|v| values.contains(v)),
        RequirementOp::GtField(other_field) => {
            let other = get_field(data, other_field);
            compare_values(value, other) == Some(std::cmp::Ordering::Greater)
        }
        RequirementOp::GteField(other_field) => {
            let other = get_field(data, other_field);
            matches!(
                compare_values(value, other),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            )
        }
        RequirementOp::LtField(other_field) => {
            let other = get_field(data, other_field);
            compare_values(value, other) == Some(std::cmp::Ordering::Less)
        }
        RequirementOp::LteField(other_field) => {
            let other = get_field(data, other_field);
            matches!(
                compare_values(value, other),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
        }
        RequirementOp::Match(pattern) => value
            .and_then(|v| v.as_str())
            .and_then(|s| regex::Regex::new(pattern).ok().map(|r| r.is_match(s)))
            .unwrap_or(false),
        RequirementOp::NotMatch(pattern) => value
            .and_then(|v| v.as_str())
            .map(|s| {
                regex::Regex::new(pattern)
                    .ok()
                    .map_or(true, |r| !r.is_match(s))
            })
            .unwrap_or(true),
        RequirementOp::MinLength(min) => value
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.len() >= *min),
        RequirementOp::Lte(expected) => {
            matches!(
                compare_values(value, Some(expected)),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Get a field value by dot-path (e.g., "address.city").
fn get_field<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = data;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    if current.is_null() {
        None
    } else {
        Some(current)
    }
}

/// Compare two JSON values numerically or lexicographically.
fn compare_values(a: Option<&Value>, b: Option<&Value>) -> Option<std::cmp::Ordering> {
    match (a?, b?) {
        (Value::Number(a), Value::Number(b)) => a.as_f64()?.partial_cmp(&b.as_f64()?),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// Interpolate {field_name} placeholders in fix suggestions.
fn interpolate_fix(template: &str, data: &Value) -> String {
    let mut result = template.to_string();
    // Simple brace interpolation: {field_name} → field value
    while let Some(start) = result.find('{') {
        if let Some(end) = result[start..].find('}') {
            let field = &result[start + 1..start + end];
            let value = get_field(data, field)
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_else(|| "<missing>".to_string());
            result = format!(
                "{}{}{}",
                &result[..start],
                value,
                &result[start + end + 1..]
            );
        } else {
            break;
        }
    }
    result
}

/// Build context JSON from the condition that triggered the rule.
fn condition_context(condition: &RuleCondition, data: &Value) -> Value {
    match condition {
        RuleCondition::Field { field, op } => {
            let value = get_field(data, field);
            serde_json::json!({
                "trigger_field": field,
                "trigger_value": value,
                "operator": format!("{op:?}"),
            })
        }
        RuleCondition::All { .. } => serde_json::json!({"compound": "all"}),
        RuleCondition::Any { .. } => serde_json::json!({"compound": "any"}),
    }
}

// ── Rule validation on schema save (US-069) ─────────────────────────────────

/// Error from validating rule definitions against the entity schema.
#[derive(Debug, Clone)]
pub struct RuleDefinitionError {
    /// Rule name that has the error (if applicable).
    pub rule: Option<String>,
    /// Description of the problem.
    pub message: String,
}

impl std::fmt::Display for RuleDefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.rule {
            Some(name) => write!(f, "rule '{}': {}", name, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// Validate rule definitions against the entity schema.
///
/// Checks that:
/// - Rule names are unique
/// - Referenced fields exist in the entity schema (when schema is provided)
/// - Regex patterns compile
/// - Messages are non-empty
/// - Cross-field references are valid
///
/// Returns all errors found (not just the first).
pub fn validate_rule_definitions(
    rules: &[ValidationRule],
    entity_schema: Option<&Value>,
) -> Vec<RuleDefinitionError> {
    let mut errors = Vec::new();

    // Collect known fields from the entity schema.
    let known_fields = entity_schema
        .and_then(|s| s.get("properties"))
        .and_then(|p| p.as_object())
        .map(|obj| {
            obj.keys()
                .cloned()
                .collect::<std::collections::HashSet<_>>()
        });

    // Check for duplicate rule names.
    let mut seen_names = std::collections::HashSet::new();
    for rule in rules {
        if !seen_names.insert(&rule.name) {
            errors.push(RuleDefinitionError {
                rule: Some(rule.name.clone()),
                message: "duplicate rule name".into(),
            });
        }
    }

    for rule in rules {
        // Empty message.
        if rule.message.trim().is_empty() {
            errors.push(RuleDefinitionError {
                rule: Some(rule.name.clone()),
                message: "message must not be empty".into(),
            });
        }

        // Each rule must have either gate or advisory: true.
        if rule.gate.is_none() && !rule.advisory {
            errors.push(RuleDefinitionError {
                rule: Some(rule.name.clone()),
                message: "rule must specify either 'gate' or 'advisory: true'".into(),
            });
        }

        // Validate field references in the requirement.
        if let Some(ref fields) = known_fields {
            let req_field = top_level_field(&rule.require.field);
            if !fields.contains(req_field) && req_field != "*" {
                errors.push(RuleDefinitionError {
                    rule: Some(rule.name.clone()),
                    message: format!(
                        "require references non-existent field '{}'",
                        rule.require.field
                    ),
                });
            }

            // Check cross-field references (gt_field, etc.).
            if let Some(other) = cross_field_ref(&rule.require.op) {
                let other_top = top_level_field(other);
                if !fields.contains(other_top) {
                    errors.push(RuleDefinitionError {
                        rule: Some(rule.name.clone()),
                        message: format!("cross-field reference to non-existent field '{other}'"),
                    });
                }
            }

            // Check condition field references.
            if let Some(cond) = &rule.when {
                validate_condition_fields(cond, fields, &rule.name, &mut errors);
            }
        }

        // Validate regex patterns.
        validate_regex_in_requirement(&rule.require.op, &rule.name, &mut errors);
        if let Some(cond) = &rule.when {
            validate_regex_in_condition(cond, &rule.name, &mut errors);
        }
    }

    errors
}

/// Extract the top-level field name from a dot-path.
fn top_level_field(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}

/// Extract cross-field reference from a requirement operator, if any.
fn cross_field_ref(op: &RequirementOp) -> Option<&str> {
    match op {
        RequirementOp::GtField(f)
        | RequirementOp::GteField(f)
        | RequirementOp::LtField(f)
        | RequirementOp::LteField(f) => Some(f),
        _ => None,
    }
}

/// Validate field references in a condition tree.
fn validate_condition_fields(
    cond: &RuleCondition,
    known_fields: &std::collections::HashSet<String>,
    rule_name: &str,
    errors: &mut Vec<RuleDefinitionError>,
) {
    match cond {
        RuleCondition::Field { field, .. } => {
            let top = top_level_field(field);
            if !known_fields.contains(top) && top != "*" {
                errors.push(RuleDefinitionError {
                    rule: Some(rule_name.into()),
                    message: format!("when condition references non-existent field '{field}'"),
                });
            }
        }
        RuleCondition::All { all } => {
            for c in all {
                validate_condition_fields(c, known_fields, rule_name, errors);
            }
        }
        RuleCondition::Any { any } => {
            for c in any {
                validate_condition_fields(c, known_fields, rule_name, errors);
            }
        }
    }
}

/// Validate regex patterns in requirement operators.
fn validate_regex_in_requirement(
    op: &RequirementOp,
    rule_name: &str,
    errors: &mut Vec<RuleDefinitionError>,
) {
    let pattern = match op {
        RequirementOp::Match(p) | RequirementOp::NotMatch(p) => Some(p),
        _ => None,
    };
    if let Some(pattern) = pattern {
        if let Err(e) = regex::Regex::new(pattern) {
            errors.push(RuleDefinitionError {
                rule: Some(rule_name.into()),
                message: format!("invalid regex pattern '{pattern}': {e}"),
            });
        }
    }
}

/// Validate regex patterns in condition operators.
fn validate_regex_in_condition(
    cond: &RuleCondition,
    rule_name: &str,
    errors: &mut Vec<RuleDefinitionError>,
) {
    match cond {
        RuleCondition::Field { op, .. } => {
            if let ConditionOp::Match(pattern) = op {
                if let Err(e) = regex::Regex::new(pattern) {
                    errors.push(RuleDefinitionError {
                        rule: Some(rule_name.into()),
                        message: format!("invalid regex in condition '{pattern}': {e}"),
                    });
                }
            }
        }
        RuleCondition::All { all } => {
            for c in all {
                validate_regex_in_condition(c, rule_name, errors);
            }
        }
        RuleCondition::Any { any } => {
            for c in any {
                validate_regex_in_condition(c, rule_name, errors);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule_approved_needs_approver() -> ValidationRule {
        ValidationRule {
            name: "approved-needs-approver".into(),
            gate: Some("save".into()),
            advisory: false,
            when: Some(RuleCondition::Field {
                field: "status".into(),
                op: ConditionOp::Eq(json!("approved")),
            }),
            require: RuleRequirement {
                field: "approver_id".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "Approved items must have an approver_id".into(),
            fix: Some("Set approver_id to the user who approved this item".into()),
        }
    }

    fn rule_bugs_need_priority() -> ValidationRule {
        ValidationRule {
            name: "bugs-need-priority".into(),
            gate: Some("complete".into()),
            advisory: false,
            when: Some(RuleCondition::Field {
                field: "bead_type".into(),
                op: ConditionOp::Eq(json!("bug")),
            }),
            require: RuleRequirement {
                field: "priority".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "Bugs must have a priority".into(),
            fix: Some("Set priority to 0-4".into()),
        }
    }

    fn rule_unconditional_title() -> ValidationRule {
        ValidationRule {
            name: "title-not-placeholder".into(),
            gate: None,
            advisory: true,
            when: None,
            require: RuleRequirement {
                field: "title".into(),
                op: RequirementOp::NotMatch("^(TODO|FIXME|untitled)$".into()),
            },
            message: "Title appears to be a placeholder".into(),
            fix: Some("Replace with a descriptive name".into()),
        }
    }

    // ── Condition met, requirement fails → violation ────────────────────

    #[test]
    fn approved_without_approver_fires() {
        let rule = rule_approved_needs_approver();
        let data = json!({"status": "approved"});
        let violation = evaluate_rule(&rule, &data);
        assert!(violation.is_some());
        let v = violation.expect("approved status without approver should violate the rule");
        assert_eq!(v.rule, "approved-needs-approver");
        assert_eq!(v.field, "approver_id");
    }

    #[test]
    fn approved_with_approver_passes() {
        let rule = rule_approved_needs_approver();
        let data = json!({"status": "approved", "approver_id": "alice"});
        assert!(evaluate_rule(&rule, &data).is_none());
    }

    // ── Condition not met → rule doesn't fire ──────────────────────────

    #[test]
    fn draft_without_approver_passes() {
        let rule = rule_approved_needs_approver();
        let data = json!({"status": "draft"});
        assert!(evaluate_rule(&rule, &data).is_none());
    }

    // ── Unconditional rule (no when) ───────────────────────────────────

    #[test]
    fn unconditional_rule_fires_on_placeholder() {
        let rule = rule_unconditional_title();
        let data = json!({"title": "TODO"});
        let v = evaluate_rule(&rule, &data);
        assert!(v.is_some());
        assert!(
            v.expect("placeholder title should produce an advisory violation")
                .advisory
        );
    }

    #[test]
    fn unconditional_rule_passes_on_real_title() {
        let rule = rule_unconditional_title();
        let data = json!({"title": "Implement auth middleware"});
        assert!(evaluate_rule(&rule, &data).is_none());
    }

    // ── All (AND) compound condition ───────────────────────────────────

    #[test]
    fn all_condition_fires_when_both_match() {
        let rule = ValidationRule {
            name: "high-priority-bugs-need-assignee".into(),
            gate: Some("review".into()),
            advisory: false,
            when: Some(RuleCondition::All {
                all: vec![
                    RuleCondition::Field {
                        field: "bead_type".into(),
                        op: ConditionOp::Eq(json!("bug")),
                    },
                    RuleCondition::Field {
                        field: "priority".into(),
                        op: ConditionOp::Lte(json!(1)),
                    },
                ],
            }),
            require: RuleRequirement {
                field: "assignee".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "High-priority bugs need assignee".into(),
            fix: None,
        };

        // Both conditions met, requirement fails
        let data = json!({"bead_type": "bug", "priority": 0});
        assert!(evaluate_rule(&rule, &data).is_some());

        // One condition not met
        let data = json!({"bead_type": "task", "priority": 0});
        assert!(evaluate_rule(&rule, &data).is_none());

        // Both met, requirement satisfied
        let data = json!({"bead_type": "bug", "priority": 1, "assignee": "alice"});
        assert!(evaluate_rule(&rule, &data).is_none());
    }

    // ── Any (OR) compound condition ────────────────────────────────────

    #[test]
    fn any_condition_fires_when_one_matches() {
        let rule = ValidationRule {
            name: "needs-owner".into(),
            gate: Some("complete".into()),
            advisory: false,
            when: Some(RuleCondition::Any {
                any: vec![
                    RuleCondition::Field {
                        field: "bead_type".into(),
                        op: ConditionOp::Eq(json!("epic")),
                    },
                    RuleCondition::Field {
                        field: "bead_type".into(),
                        op: ConditionOp::Eq(json!("feature")),
                    },
                ],
            }),
            require: RuleRequirement {
                field: "owner".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "Epics and features need an owner".into(),
            fix: None,
        };

        // One condition met, requirement fails
        let data = json!({"bead_type": "epic"});
        assert!(evaluate_rule(&rule, &data).is_some());

        // Neither condition met
        let data = json!({"bead_type": "task"});
        assert!(evaluate_rule(&rule, &data).is_none());
    }

    // ── Cross-field comparison (gt_field) ──────────────────────────────

    #[test]
    fn gt_field_comparison() {
        let rule = ValidationRule {
            name: "due-after-created".into(),
            gate: Some("save".into()),
            advisory: false,
            when: Some(RuleCondition::Field {
                field: "due_date".into(),
                op: ConditionOp::NotNull(true),
            }),
            require: RuleRequirement {
                field: "due_date".into(),
                op: RequirementOp::GtField("created_date".into()),
            },
            message: "Due date must be after creation date".into(),
            fix: Some("Set due_date after {created_date}".into()),
        };

        // due_date > created_date → passes
        let data = json!({"due_date": "2026-04-10", "created_date": "2026-04-01"});
        assert!(evaluate_rule(&rule, &data).is_none());

        // due_date < created_date → violation
        let data = json!({"due_date": "2026-03-01", "created_date": "2026-04-01"});
        let v = evaluate_rule(&rule, &data);
        assert!(v.is_some());

        // due_date absent → condition not met, rule doesn't fire
        let data = json!({"created_date": "2026-04-01"});
        assert!(evaluate_rule(&rule, &data).is_none());
    }

    // ── evaluate_rules: all rules, all violations ──────────────────────

    #[test]
    fn evaluate_rules_collects_all_violations() {
        let rules = vec![
            rule_approved_needs_approver(),
            rule_bugs_need_priority(),
            rule_unconditional_title(),
        ];

        let data = json!({
            "status": "approved",
            "bead_type": "bug",
            "title": "TODO"
        });

        let violations = evaluate_rules(&rules, &data);
        assert_eq!(violations.len(), 3);
        assert!(violations
            .iter()
            .any(|v| v.rule == "approved-needs-approver"));
        assert!(violations.iter().any(|v| v.rule == "bugs-need-priority"));
        assert!(violations.iter().any(|v| v.rule == "title-not-placeholder"));
    }

    #[test]
    fn evaluate_rules_empty_when_all_pass() {
        let rules = vec![rule_approved_needs_approver()];
        let data = json!({"status": "draft"});
        assert!(evaluate_rules(&rules, &data).is_empty());
    }

    // ── Fix interpolation ──────────────────────────────────────────────

    #[test]
    fn fix_interpolation() {
        let rule = ValidationRule {
            name: "test".into(),
            gate: None,
            advisory: true,
            when: None,
            require: RuleRequirement {
                field: "end_date".into(),
                op: RequirementOp::GtField("start_date".into()),
            },
            message: "end must be after start".into(),
            fix: Some("Set end_date after {start_date}".into()),
        };

        let data = json!({"start_date": "2026-01-01", "end_date": "2025-12-01"});
        let v = evaluate_rule(&rule, &data)
            .expect("violating gt_field rule should produce a fixable violation");
        assert_eq!(
            v.fix
                .expect("gt_field violation should include the interpolated fix"),
            "Set end_date after 2026-01-01"
        );
    }

    // ── Dot-path field access ──────────────────────────────────────────

    #[test]
    fn dot_path_field_access() {
        let rule = ValidationRule {
            name: "city-required".into(),
            gate: Some("complete".into()),
            advisory: false,
            when: Some(RuleCondition::Field {
                field: "address.country".into(),
                op: ConditionOp::Eq(json!("US")),
            }),
            require: RuleRequirement {
                field: "address.zip".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "US addresses need a zip code".into(),
            fix: None,
        };

        let data = json!({"address": {"country": "US"}});
        assert!(evaluate_rule(&rule, &data).is_some());

        let data = json!({"address": {"country": "US", "zip": "98101"}});
        assert!(evaluate_rule(&rule, &data).is_none());

        let data = json!({"address": {"country": "UK"}});
        assert!(evaluate_rule(&rule, &data).is_none());
    }

    // ── Rule definition validation (US-069) ──────────────────────────────

    fn entity_schema_with_fields(fields: &[&str]) -> Value {
        let mut props = serde_json::Map::new();
        for f in fields {
            props.insert((*f).to_string(), json!({"type": "string"}));
        }
        json!({
            "type": "object",
            "properties": props,
        })
    }

    fn simple_rule(name: &str, field: &str) -> ValidationRule {
        ValidationRule {
            name: name.into(),
            gate: Some("save".into()),
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: field.into(),
                op: RequirementOp::NotNull(true),
            },
            message: "field required".into(),
            fix: None,
        }
    }

    #[test]
    fn duplicate_rule_names_rejected() {
        let rules = vec![
            simple_rule("my-rule", "title"),
            simple_rule("my-rule", "description"),
        ];
        let errors = validate_rule_definitions(&rules, None);
        assert!(errors.iter().any(|e| e.message.contains("duplicate")));
    }

    #[test]
    fn non_existent_field_in_require_rejected() {
        let schema = entity_schema_with_fields(&["title", "status"]);
        let rules = vec![simple_rule("check-ghost", "ghost_field")];
        let errors = validate_rule_definitions(&rules, Some(&schema));
        assert!(errors
            .iter()
            .any(|e| e.message.contains("non-existent field 'ghost_field'")));
    }

    #[test]
    fn valid_field_references_pass() {
        let schema = entity_schema_with_fields(&["title", "status"]);
        let rules = vec![simple_rule("check-title", "title")];
        let errors = validate_rule_definitions(&rules, Some(&schema));
        assert!(
            errors.is_empty(),
            "should pass with valid fields: {errors:?}"
        );
    }

    #[test]
    fn gt_field_referencing_non_existent_field_rejected() {
        let schema = entity_schema_with_fields(&["due_date", "created_date"]);
        let rules = vec![ValidationRule {
            name: "due-after-start".into(),
            gate: Some("save".into()),
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: "due_date".into(),
                op: RequirementOp::GtField("start_date".into()), // doesn't exist
            },
            message: "due must be after start".into(),
            fix: None,
        }];
        let errors = validate_rule_definitions(&rules, Some(&schema));
        assert!(errors
            .iter()
            .any(|e| e.message.contains("non-existent field 'start_date'")));
    }

    #[test]
    fn invalid_regex_pattern_rejected() {
        let rules = vec![ValidationRule {
            name: "bad-regex".into(),
            gate: Some("save".into()),
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: "email".into(),
                op: RequirementOp::Match("[invalid".into()),
            },
            message: "must match email".into(),
            fix: None,
        }];
        let errors = validate_rule_definitions(&rules, None);
        assert!(errors.iter().any(|e| e.message.contains("invalid regex")));
    }

    #[test]
    fn valid_regex_pattern_passes() {
        let rules = vec![ValidationRule {
            name: "email-check".into(),
            gate: Some("save".into()),
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: "email".into(),
                op: RequirementOp::Match("^.+@.+$".into()),
            },
            message: "must match email".into(),
            fix: None,
        }];
        let errors = validate_rule_definitions(&rules, None);
        assert!(errors.is_empty(), "valid regex should pass: {errors:?}");
    }

    #[test]
    fn empty_message_rejected() {
        let rules = vec![ValidationRule {
            name: "no-msg".into(),
            gate: Some("save".into()),
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: "title".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "  ".into(), // whitespace only
            fix: None,
        }];
        let errors = validate_rule_definitions(&rules, None);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("message must not be empty")));
    }

    #[test]
    fn condition_field_reference_validated() {
        let schema = entity_schema_with_fields(&["title", "status"]);
        let rules = vec![ValidationRule {
            name: "check-when".into(),
            gate: Some("complete".into()),
            advisory: false,
            when: Some(RuleCondition::Field {
                field: "nonexistent".into(),
                op: ConditionOp::Eq(json!("x")),
            }),
            require: RuleRequirement {
                field: "title".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "title needed".into(),
            fix: None,
        }];
        let errors = validate_rule_definitions(&rules, Some(&schema));
        assert!(errors
            .iter()
            .any(|e| e.message.contains("non-existent field 'nonexistent'")));
    }

    #[test]
    fn rule_without_gate_or_advisory_rejected() {
        let rules = vec![ValidationRule {
            name: "orphan-rule".into(),
            gate: None,
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: "title".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "title needed".into(),
            fix: None,
        }];
        let errors = validate_rule_definitions(&rules, None);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("must specify either")));
    }

    #[test]
    fn invalid_regex_in_condition_rejected() {
        let rules = vec![ValidationRule {
            name: "cond-regex".into(),
            gate: Some("save".into()),
            advisory: false,
            when: Some(RuleCondition::Field {
                field: "email".into(),
                op: ConditionOp::Match("[bad".into()),
            }),
            require: RuleRequirement {
                field: "title".into(),
                op: RequirementOp::NotNull(true),
            },
            message: "title needed".into(),
            fix: None,
        }];
        let errors = validate_rule_definitions(&rules, None);
        assert!(errors.iter().any(|e| e.message.contains("invalid regex")));
    }

    #[test]
    fn no_errors_without_entity_schema() {
        // When no entity schema is provided, field references are not checked.
        let rules = vec![simple_rule("check-anything", "any_field")];
        let errors = validate_rule_definitions(&rules, None);
        assert!(errors.is_empty());
    }
}
