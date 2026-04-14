//! Validation gate evaluation (ESF Layer 5).
//!
//! Gates group validation rules by purpose. The `save` gate blocks persistence;
//! custom gates (e.g. `complete`, `review`) allow saves but track readiness.
//! Advisory rules never block and are collected separately.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rules::{evaluate_rule, RuleViolation, ValidationRule};
use crate::schema::GateDef;

/// Result of evaluating a single gate for an entity.
///
/// Re-exported from `axon-core` so `Entity` can carry materialized gate
/// results as a first-class field (FEAT-019).
pub use axon_core::types::GateResult;

/// Complete gate evaluation result for an entity write.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GateEvaluation {
    /// Save-gate violations. If non-empty, the entity must NOT be persisted.
    pub save_violations: Vec<RuleViolation>,
    /// Per-gate results for all custom (non-save) gates.
    pub gate_results: HashMap<String, GateResult>,
    /// Advisory violations (never block).
    pub advisories: Vec<RuleViolation>,
}

impl GateEvaluation {
    /// Returns `true` if the save gate passes (entity can be persisted).
    pub fn save_passes(&self) -> bool {
        self.save_violations.is_empty()
    }
}

/// Evaluate all validation rules against entity data, grouped by gate.
///
/// Gate inclusion is resolved: if gate `review` includes `complete`, then
/// all `complete` gate rules are also checked for `review`.
#[allow(clippy::implicit_hasher)]
pub fn evaluate_gates(
    rules: &[ValidationRule],
    gates: &HashMap<String, GateDef>,
    data: &Value,
) -> GateEvaluation {
    let mut result = GateEvaluation::default();

    // Evaluate each rule and bucket by gate/advisory.
    let mut violations_by_gate: HashMap<String, Vec<RuleViolation>> = HashMap::new();

    for rule in rules {
        if let Some(violation) = evaluate_rule(rule, data) {
            if rule.advisory {
                result.advisories.push(violation);
            } else if let Some(gate) = &rule.gate {
                if gate == "save" {
                    result.save_violations.push(violation);
                } else {
                    violations_by_gate
                        .entry(gate.clone())
                        .or_default()
                        .push(violation);
                }
            }
        }
    }

    // Collect all gate names that have rules (from rules themselves and from gate defs).
    let mut all_gates: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rule in rules {
        if let Some(gate) = &rule.gate {
            if gate != "save" {
                all_gates.insert(gate.clone());
            }
        }
    }
    for gate_name in gates.keys() {
        if gate_name != "save" {
            all_gates.insert(gate_name.clone());
        }
    }

    // Resolve gate inclusion and build results.
    for gate_name in &all_gates {
        let mut failures = Vec::new();

        // Direct violations for this gate.
        if let Some(direct) = violations_by_gate.get(gate_name) {
            failures.extend(direct.iter().cloned());
        }

        // Inherited violations from included gates.
        let included = resolve_includes(gate_name, gates);
        for included_gate in &included {
            if let Some(inherited) = violations_by_gate.get(included_gate) {
                failures.extend(inherited.iter().cloned());
            }
        }

        let pass = failures.is_empty();
        result.gate_results.insert(
            gate_name.clone(),
            GateResult {
                gate: gate_name.clone(),
                pass,
                failures,
            },
        );
    }

    result
}

/// Resolve all gates transitively included by the given gate.
///
/// Returns the set of gate names whose rules should also apply to `gate_name`.
/// Does NOT include `gate_name` itself.
fn resolve_includes(gate_name: &str, gates: &HashMap<String, GateDef>) -> Vec<String> {
    let mut resolved = Vec::new();
    let mut stack = Vec::new();
    let mut visited = std::collections::HashSet::new();

    // Seed with direct includes.
    if let Some(gate_def) = gates.get(gate_name) {
        for inc in &gate_def.includes {
            stack.push(inc.clone());
        }
    }

    while let Some(current) = stack.pop() {
        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());
        resolved.push(current.clone());

        // Transitively include.
        if let Some(gate_def) = gates.get(&current) {
            for inc in &gate_def.includes {
                if !visited.contains(inc) {
                    stack.push(inc.clone());
                }
            }
        }
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{ConditionOp, RequirementOp, RuleCondition, RuleRequirement};
    use serde_json::json;

    fn save_rule(name: &str, field: &str, msg: &str) -> ValidationRule {
        ValidationRule {
            name: name.into(),
            gate: Some("save".into()),
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: field.into(),
                op: RequirementOp::NotNull(true),
            },
            message: msg.into(),
            fix: None,
        }
    }

    fn gate_rule(name: &str, gate: &str, field: &str, msg: &str) -> ValidationRule {
        ValidationRule {
            name: name.into(),
            gate: Some(gate.into()),
            advisory: false,
            when: None,
            require: RuleRequirement {
                field: field.into(),
                op: RequirementOp::NotNull(true),
            },
            message: msg.into(),
            fix: Some(format!("Set {field}")),
        }
    }

    fn advisory_rule(name: &str, field: &str, msg: &str) -> ValidationRule {
        ValidationRule {
            name: name.into(),
            gate: None,
            advisory: true,
            when: None,
            require: RuleRequirement {
                field: field.into(),
                op: RequirementOp::NotNull(true),
            },
            message: msg.into(),
            fix: Some(format!("Add {field}")),
        }
    }

    fn conditional_gate_rule(
        name: &str,
        gate: &str,
        when_field: &str,
        when_val: &str,
        require_field: &str,
        msg: &str,
    ) -> ValidationRule {
        ValidationRule {
            name: name.into(),
            gate: Some(gate.into()),
            advisory: false,
            when: Some(RuleCondition::Field {
                field: when_field.into(),
                op: ConditionOp::Eq(json!(when_val)),
            }),
            require: RuleRequirement {
                field: require_field.into(),
                op: RequirementOp::NotNull(true),
            },
            message: msg.into(),
            fix: None,
        }
    }

    // ── Save gate blocks persistence ───────────────────────────────────

    #[test]
    fn save_gate_blocks_when_rule_fails() {
        let rules = vec![save_rule("need-title", "title", "Title required")];
        let gates = HashMap::new();
        let data = json!({});

        let eval = evaluate_gates(&rules, &gates, &data);
        assert!(!eval.save_passes());
        assert_eq!(eval.save_violations.len(), 1);
        assert_eq!(eval.save_violations[0].rule, "need-title");
    }

    #[test]
    fn save_gate_passes_when_rule_satisfied() {
        let rules = vec![save_rule("need-title", "title", "Title required")];
        let gates = HashMap::new();
        let data = json!({"title": "Hello"});

        let eval = evaluate_gates(&rules, &gates, &data);
        assert!(eval.save_passes());
        assert!(eval.save_violations.is_empty());
    }

    // ── Custom gate allows save, reports failure ───────────────────────

    #[test]
    fn custom_gate_allows_save_but_reports_failure() {
        let rules = vec![gate_rule(
            "need-desc",
            "complete",
            "description",
            "Description required for completion",
        )];
        let gates = HashMap::from([(
            "complete".into(),
            GateDef {
                description: Some("Ready for processing".into()),
                includes: vec![],
            },
        )]);
        let data = json!({"title": "Something"});

        let eval = evaluate_gates(&rules, &gates, &data);
        // Save passes (no save-gate rules).
        assert!(eval.save_passes());
        // Complete gate fails.
        let complete = eval
            .gate_results
            .get("complete")
            .expect("complete gate result should be present");
        assert!(!complete.pass);
        assert_eq!(complete.failures.len(), 1);
        assert_eq!(complete.failures[0].rule, "need-desc");
        assert_eq!(complete.failures[0].fix.as_deref(), Some("Set description"));
    }

    #[test]
    fn custom_gate_passes_when_rule_satisfied() {
        let rules = vec![gate_rule(
            "need-desc",
            "complete",
            "description",
            "Description required",
        )];
        let gates = HashMap::from([(
            "complete".into(),
            GateDef {
                description: None,
                includes: vec![],
            },
        )]);
        let data = json!({"description": "Some text"});

        let eval = evaluate_gates(&rules, &gates, &data);
        let complete = eval
            .gate_results
            .get("complete")
            .expect("complete gate result should be present");
        assert!(complete.pass);
        assert!(complete.failures.is_empty());
    }

    // ── Advisory rules never block ─────────────────────────────────────

    #[test]
    fn advisory_rules_never_block() {
        let rules = vec![advisory_rule(
            "recommend-desc",
            "description",
            "Consider adding a description",
        )];
        let gates = HashMap::new();
        let data = json!({});

        let eval = evaluate_gates(&rules, &gates, &data);
        assert!(eval.save_passes());
        assert!(eval.gate_results.is_empty());
        assert_eq!(eval.advisories.len(), 1);
        assert_eq!(eval.advisories[0].rule, "recommend-desc");
        assert!(eval.advisories[0].advisory);
    }

    #[test]
    fn advisory_rules_not_reported_when_passing() {
        let rules = vec![advisory_rule("recommend-desc", "description", "Add desc")];
        let gates = HashMap::new();
        let data = json!({"description": "Present"});

        let eval = evaluate_gates(&rules, &gates, &data);
        assert!(eval.advisories.is_empty());
    }

    // ── Gate inclusion (review includes complete) ──────────────────────

    #[test]
    fn gate_inclusion_inherits_failures() {
        let rules = vec![
            gate_rule(
                "need-desc",
                "complete",
                "description",
                "Description required",
            ),
            gate_rule(
                "need-acceptance",
                "review",
                "acceptance",
                "Acceptance criteria required",
            ),
        ];
        let gates = HashMap::from([
            (
                "complete".into(),
                GateDef {
                    description: None,
                    includes: vec![],
                },
            ),
            (
                "review".into(),
                GateDef {
                    description: None,
                    includes: vec!["complete".into()],
                },
            ),
        ]);
        let data = json!({}); // missing both description and acceptance

        let eval = evaluate_gates(&rules, &gates, &data);

        // Complete gate: 1 failure (its own rule).
        let complete = eval
            .gate_results
            .get("complete")
            .expect("complete gate result should be present");
        assert!(!complete.pass);
        assert_eq!(complete.failures.len(), 1);

        // Review gate: 2 failures (its own + inherited from complete).
        let review = eval
            .gate_results
            .get("review")
            .expect("review gate result should be present");
        assert!(!review.pass);
        assert_eq!(review.failures.len(), 2);
        let review_rules: Vec<&str> = review.failures.iter().map(|f| f.rule.as_str()).collect();
        assert!(review_rules.contains(&"need-desc"));
        assert!(review_rules.contains(&"need-acceptance"));
    }

    #[test]
    fn gate_inclusion_passes_when_all_inherited_pass() {
        let rules = vec![
            gate_rule(
                "need-desc",
                "complete",
                "description",
                "Description required",
            ),
            gate_rule(
                "need-acceptance",
                "review",
                "acceptance",
                "Acceptance criteria required",
            ),
        ];
        let gates = HashMap::from([
            (
                "complete".into(),
                GateDef {
                    description: None,
                    includes: vec![],
                },
            ),
            (
                "review".into(),
                GateDef {
                    description: None,
                    includes: vec!["complete".into()],
                },
            ),
        ]);
        let data = json!({"description": "Done", "acceptance": "Tests pass"});

        let eval = evaluate_gates(&rules, &gates, &data);
        assert!(
            eval.gate_results
                .get("complete")
                .expect("complete gate result should be present")
                .pass
        );
        assert!(
            eval.gate_results
                .get("review")
                .expect("review gate result should be present")
                .pass
        );
    }

    // ── Write response includes all gate results ───────────────────────

    #[test]
    fn evaluation_includes_all_gate_results() {
        let rules = vec![
            save_rule("need-type", "bead_type", "Type required"),
            gate_rule("need-desc", "complete", "description", "Need desc"),
            gate_rule("need-acceptance", "review", "acceptance", "Need acceptance"),
            advisory_rule("recommend-tags", "tags", "Consider adding tags"),
        ];
        let gates = HashMap::from([
            (
                "complete".into(),
                GateDef {
                    description: None,
                    includes: vec![],
                },
            ),
            (
                "review".into(),
                GateDef {
                    description: None,
                    includes: vec!["complete".into()],
                },
            ),
        ]);
        let data = json!({"bead_type": "task"});

        let eval = evaluate_gates(&rules, &gates, &data);
        // Save passes.
        assert!(eval.save_passes());
        // Both custom gates reported.
        assert!(eval.gate_results.contains_key("complete"));
        assert!(eval.gate_results.contains_key("review"));
        // Advisory reported.
        assert_eq!(eval.advisories.len(), 1);
    }

    // ── Conditional rules with gates ───────────────────────────────────

    #[test]
    fn conditional_gate_rule_only_fires_when_condition_met() {
        let rules = vec![conditional_gate_rule(
            "bugs-need-priority",
            "complete",
            "bead_type",
            "bug",
            "priority",
            "Bugs must have priority",
        )];
        let gates = HashMap::from([(
            "complete".into(),
            GateDef {
                description: None,
                includes: vec![],
            },
        )]);

        // Condition met, requirement fails.
        let data = json!({"bead_type": "bug"});
        let eval = evaluate_gates(&rules, &gates, &data);
        assert!(
            !eval
                .gate_results
                .get("complete")
                .expect("complete gate result should be present")
                .pass
        );

        // Condition not met (different type).
        let data = json!({"bead_type": "task"});
        let eval = evaluate_gates(&rules, &gates, &data);
        assert!(
            eval.gate_results
                .get("complete")
                .expect("complete gate result should be present")
                .pass
        );
    }

    // ── Transitive gate inclusion ──────────────────────────────────────

    #[test]
    fn transitive_gate_inclusion() {
        let rules = vec![
            gate_rule("need-a", "basic", "field_a", "Need A"),
            gate_rule("need-b", "complete", "field_b", "Need B"),
            gate_rule("need-c", "review", "field_c", "Need C"),
        ];
        let gates = HashMap::from([
            (
                "basic".into(),
                GateDef {
                    description: None,
                    includes: vec![],
                },
            ),
            (
                "complete".into(),
                GateDef {
                    description: None,
                    includes: vec!["basic".into()],
                },
            ),
            (
                "review".into(),
                GateDef {
                    description: None,
                    includes: vec!["complete".into()],
                },
            ),
        ]);
        let data = json!({});

        let eval = evaluate_gates(&rules, &gates, &data);

        // basic: 1 failure.
        assert_eq!(
            eval.gate_results
                .get("basic")
                .expect("basic gate result should be present")
                .failures
                .len(),
            1
        );
        // complete: 2 failures (own + basic).
        assert_eq!(
            eval.gate_results
                .get("complete")
                .expect("complete gate result should be present")
                .failures
                .len(),
            2
        );
        // review: 3 failures (own + complete + basic).
        assert_eq!(
            eval.gate_results
                .get("review")
                .expect("review gate result should be present")
                .failures
                .len(),
            3
        );
    }

    // ── Empty rules produce empty results ──────────────────────────────

    #[test]
    fn empty_rules_all_pass() {
        let eval = evaluate_gates(&[], &HashMap::new(), &json!({}));
        assert!(eval.save_passes());
        assert!(eval.gate_results.is_empty());
        assert!(eval.advisories.is_empty());
    }
}
