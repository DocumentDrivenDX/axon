//! Access-control policy compilation.
//!
//! The parser in `access_control` preserves the authoring AST. This module
//! normalizes predicates into a plan shape that downstream policy evaluation,
//! GraphQL generation, and MCP metadata can consume.

use std::collections::{HashMap, HashSet};
use std::fmt;

use axon_core::error::AxonError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::access_control::{
    AccessControlPolicy, ApprovalRoute, FieldAccessPolicy, FieldPolicy, FieldPolicyRule,
    LinkDirection, OperationPolicy, PolicyCompareOp, PolicyDecision, PolicyEnvelope,
    PolicyOperation, PolicyPredicate, PolicyRule, RelationshipPredicate, SharesRelationPredicate,
};
use crate::schema::CollectionSchema;

/// Compile the collection's optional access-control policy into a normalized plan.
pub fn compile_policy_plan(schema: &CollectionSchema) -> Result<Option<PolicyPlan>, AxonError> {
    match &schema.access_control {
        Some(policy) => PolicyCompiler::new(schema, policy).compile().map(Some),
        None => Ok(None),
    }
}

/// Stable policy compilation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCompileError {
    message: String,
}

impl PolicyCompileError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Stable error detail without the `SchemaValidation` wrapper.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PolicyCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "policy_expression_invalid: {}", self.message)
    }
}

impl From<PolicyCompileError> for AxonError {
    fn from(err: PolicyCompileError) -> Self {
        AxonError::SchemaValidation(err.to_string())
    }
}

/// Normalized policy plan for one collection schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PolicyPlan {
    /// Operation policies keyed by operation class.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub operations: HashMap<PolicyOperation, CompiledOperationPolicy>,

    /// Field policies keyed by field path.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, CompiledFieldPolicy>,

    /// Transition policies: lifecycle field -> transition name -> policy.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub transitions: HashMap<String, HashMap<String, CompiledOperationPolicy>>,

    /// Decision envelopes keyed by operation class.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub envelopes: HashMap<PolicyOperation, Vec<CompiledPolicyEnvelope>>,
}

/// Normalized allow/deny operation policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CompiledOperationPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<CompiledPolicyRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<CompiledPolicyRule>,
}

/// Normalized field-level policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CompiledFieldPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<CompiledFieldAccessPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<CompiledFieldAccessPolicy>,
}

/// Normalized field access rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CompiledFieldAccessPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<CompiledFieldPolicyRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<CompiledFieldPolicyRule>,
}

/// Normalized operation rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CompiledPolicyRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<CompiledPredicate>,
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<CompiledPredicate>,
}

/// Normalized field rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CompiledFieldPolicyRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<CompiledPredicate>,
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub where_clause: Option<CompiledPredicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redact_as: Option<Value>,
}

/// Normalized decision envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledPolicyEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<CompiledPredicate>,
    pub decision: PolicyDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalRoute>,
}

/// Normalized policy predicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompiledPredicate {
    All(Vec<CompiledPredicate>),
    Any(Vec<CompiledPredicate>),
    Not(Box<CompiledPredicate>),
    Compare(CompiledComparison),
    Operation(PolicyOperation),
    Related(CompiledRelationshipPredicate),
    SharesRelation(CompiledSharesRelationPredicate),
}

/// Normalized field or subject comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledComparison {
    pub target: PredicateTarget,
    pub op: CompiledCompareOp,
}

/// A comparison target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateTarget {
    Subject(String),
    Field(String),
}

/// Normalized comparison operator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompiledCompareOp {
    Eq(Value),
    Ne(Value),
    In(Vec<Value>),
    NotNull(bool),
    IsNull(bool),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    ContainsSubject(String),
    EqSubject(String),
}

/// Normalized link-backed relationship predicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledRelationshipPredicate {
    pub link_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<LinkDirection>,
    pub target_collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_policy: Option<PolicyOperation>,
}

/// Normalized shared-relation predicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledSharesRelationPredicate {
    pub collection: String,
    pub field: String,
    pub subject_field: String,
    pub target_field: String,
}

struct PolicyCompiler<'a> {
    schema: &'a CollectionSchema,
    policy: &'a AccessControlPolicy,
    subject_refs: HashSet<String>,
}

impl<'a> PolicyCompiler<'a> {
    fn new(schema: &'a CollectionSchema, policy: &'a AccessControlPolicy) -> Self {
        let mut subject_refs = builtin_subject_refs();
        if let Some(identity) = &policy.identity {
            subject_refs.extend(identity.subject.keys().cloned());
            subject_refs.extend(identity.attributes.keys().cloned());
            subject_refs.extend(identity.aliases.keys().cloned());
        }
        Self {
            schema,
            policy,
            subject_refs,
        }
    }

    fn compile(&self) -> Result<PolicyPlan, AxonError> {
        let mut plan = PolicyPlan::default();
        self.compile_operation(&mut plan, PolicyOperation::Read, self.policy.read.as_ref())?;
        self.compile_operation(
            &mut plan,
            PolicyOperation::Create,
            self.policy.create.as_ref(),
        )?;
        self.compile_operation(
            &mut plan,
            PolicyOperation::Update,
            self.policy.update.as_ref(),
        )?;
        self.compile_operation(
            &mut plan,
            PolicyOperation::Delete,
            self.policy.delete.as_ref(),
        )?;
        self.compile_operation(
            &mut plan,
            PolicyOperation::Write,
            self.policy.write.as_ref(),
        )?;
        self.compile_operation(
            &mut plan,
            PolicyOperation::Admin,
            self.policy.admin.as_ref(),
        )?;

        for (field, policy) in &self.policy.fields {
            self.validate_field_path(field, &format!("fields.{field}"))?;
            plan.fields
                .insert(field.clone(), self.compile_field_policy(field, policy)?);
        }

        for (field, transitions) in &self.policy.transitions {
            self.validate_field_path(field, &format!("transitions.{field}"))?;
            let mut compiled_transitions = HashMap::new();
            for (transition, policy) in transitions {
                let ctx = format!("transitions.{field}.{transition}");
                compiled_transitions.insert(
                    transition.clone(),
                    self.compile_operation_policy(policy, &ctx)?,
                );
            }
            plan.transitions.insert(field.clone(), compiled_transitions);
        }

        for (operation, envelopes) in &self.policy.envelopes {
            let compiled = envelopes
                .iter()
                .enumerate()
                .map(|(idx, envelope)| {
                    self.compile_envelope(envelope, &format!("envelopes.{operation:?}[{idx}]"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            plan.envelopes.insert(operation.clone(), compiled);
        }

        Ok(plan)
    }

    fn compile_operation(
        &self,
        plan: &mut PolicyPlan,
        operation: PolicyOperation,
        policy: Option<&OperationPolicy>,
    ) -> Result<(), AxonError> {
        if let Some(policy) = policy {
            plan.operations.insert(
                operation.clone(),
                self.compile_operation_policy(policy, &format!("operations.{operation:?}"))?,
            );
        }
        Ok(())
    }

    fn compile_operation_policy(
        &self,
        policy: &OperationPolicy,
        ctx: &str,
    ) -> Result<CompiledOperationPolicy, AxonError> {
        Ok(CompiledOperationPolicy {
            allow: self.compile_policy_rules(&policy.allow, &format!("{ctx}.allow"))?,
            deny: self.compile_policy_rules(&policy.deny, &format!("{ctx}.deny"))?,
        })
    }

    fn compile_policy_rules(
        &self,
        rules: &[PolicyRule],
        ctx: &str,
    ) -> Result<Vec<CompiledPolicyRule>, AxonError> {
        rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| {
                let rule_ctx = format!("{ctx}[{idx}]");
                Ok(CompiledPolicyRule {
                    name: rule.name.clone(),
                    when: self.compile_optional_predicate(
                        rule.when.as_ref(),
                        &format!("{rule_ctx}.when"),
                    )?,
                    where_clause: self.compile_optional_predicate(
                        rule.where_clause.as_ref(),
                        &format!("{rule_ctx}.where"),
                    )?,
                })
            })
            .collect()
    }

    fn compile_field_policy(
        &self,
        field: &str,
        policy: &FieldPolicy,
    ) -> Result<CompiledFieldPolicy, AxonError> {
        Ok(CompiledFieldPolicy {
            read: policy
                .read
                .as_ref()
                .map(|policy| {
                    self.compile_field_access_policy(policy, &format!("fields.{field}.read"))
                })
                .transpose()?,
            write: policy
                .write
                .as_ref()
                .map(|policy| {
                    self.compile_field_access_policy(policy, &format!("fields.{field}.write"))
                })
                .transpose()?,
        })
    }

    fn compile_field_access_policy(
        &self,
        policy: &FieldAccessPolicy,
        ctx: &str,
    ) -> Result<CompiledFieldAccessPolicy, AxonError> {
        Ok(CompiledFieldAccessPolicy {
            allow: self.compile_field_rules(&policy.allow, &format!("{ctx}.allow"))?,
            deny: self.compile_field_rules(&policy.deny, &format!("{ctx}.deny"))?,
        })
    }

    fn compile_field_rules(
        &self,
        rules: &[FieldPolicyRule],
        ctx: &str,
    ) -> Result<Vec<CompiledFieldPolicyRule>, AxonError> {
        rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| {
                let rule_ctx = format!("{ctx}[{idx}]");
                Ok(CompiledFieldPolicyRule {
                    name: rule.name.clone(),
                    when: self.compile_optional_predicate(
                        rule.when.as_ref(),
                        &format!("{rule_ctx}.when"),
                    )?,
                    where_clause: self.compile_optional_predicate(
                        rule.where_clause.as_ref(),
                        &format!("{rule_ctx}.where"),
                    )?,
                    redact_as: rule.redact_as.clone(),
                })
            })
            .collect()
    }

    fn compile_envelope(
        &self,
        envelope: &PolicyEnvelope,
        ctx: &str,
    ) -> Result<CompiledPolicyEnvelope, AxonError> {
        Ok(CompiledPolicyEnvelope {
            name: envelope.name.clone(),
            when: self
                .compile_optional_predicate(envelope.when.as_ref(), &format!("{ctx}.when"))?,
            decision: envelope.decision.clone(),
            approval: envelope.approval.clone(),
        })
    }

    fn compile_optional_predicate(
        &self,
        predicate: Option<&PolicyPredicate>,
        ctx: &str,
    ) -> Result<Option<CompiledPredicate>, AxonError> {
        predicate
            .map(|predicate| self.compile_predicate(predicate, ctx))
            .transpose()
    }

    fn compile_predicate(
        &self,
        predicate: &PolicyPredicate,
        ctx: &str,
    ) -> Result<CompiledPredicate, AxonError> {
        match predicate {
            PolicyPredicate::All { all } => Ok(CompiledPredicate::All(
                all.iter()
                    .enumerate()
                    .map(|(idx, pred)| self.compile_predicate(pred, &format!("{ctx}.all[{idx}]")))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            PolicyPredicate::Any { any } => Ok(CompiledPredicate::Any(
                any.iter()
                    .enumerate()
                    .map(|(idx, pred)| self.compile_predicate(pred, &format!("{ctx}.any[{idx}]")))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            PolicyPredicate::Not { not } => Ok(CompiledPredicate::Not(Box::new(
                self.compile_predicate(not, &format!("{ctx}.not"))?,
            ))),
            PolicyPredicate::Subject { subject, op } => {
                self.validate_subject_ref(subject, ctx)?;
                Ok(CompiledPredicate::Compare(CompiledComparison {
                    target: PredicateTarget::Subject(subject.clone()),
                    op: self.compile_compare_op(op, ctx)?,
                }))
            }
            PolicyPredicate::Field { field, op } => {
                self.validate_field_path(field, ctx)?;
                Ok(CompiledPredicate::Compare(CompiledComparison {
                    target: PredicateTarget::Field(field.clone()),
                    op: self.compile_compare_op(op, ctx)?,
                }))
            }
            PolicyPredicate::Operation { operation } => {
                Ok(CompiledPredicate::Operation(operation.clone()))
            }
            PolicyPredicate::Related { related } => self.compile_related(related, ctx),
            PolicyPredicate::SharesRelation { shares_relation } => {
                self.compile_shares_relation(shares_relation, ctx)
            }
        }
    }

    fn compile_compare_op(
        &self,
        op: &PolicyCompareOp,
        ctx: &str,
    ) -> Result<CompiledCompareOp, AxonError> {
        Ok(match op {
            PolicyCompareOp::Eq(value) => CompiledCompareOp::Eq(value.clone()),
            PolicyCompareOp::Ne(value) => CompiledCompareOp::Ne(value.clone()),
            PolicyCompareOp::In(values) => CompiledCompareOp::In(values.clone()),
            PolicyCompareOp::NotNull(value) => CompiledCompareOp::NotNull(*value),
            PolicyCompareOp::IsNull(value) => CompiledCompareOp::IsNull(*value),
            PolicyCompareOp::Gt(value) => CompiledCompareOp::Gt(value.clone()),
            PolicyCompareOp::Gte(value) => CompiledCompareOp::Gte(value.clone()),
            PolicyCompareOp::Lt(value) => CompiledCompareOp::Lt(value.clone()),
            PolicyCompareOp::Lte(value) => CompiledCompareOp::Lte(value.clone()),
            PolicyCompareOp::ContainsSubject(subject) => {
                self.validate_subject_ref(subject, ctx)?;
                CompiledCompareOp::ContainsSubject(subject.clone())
            }
            PolicyCompareOp::EqSubject(subject) => {
                self.validate_subject_ref(subject, ctx)?;
                CompiledCompareOp::EqSubject(subject.clone())
            }
        })
    }

    fn compile_related(
        &self,
        related: &RelationshipPredicate,
        ctx: &str,
    ) -> Result<CompiledPredicate, AxonError> {
        if !self.schema.link_types.contains_key(&related.link_type) {
            return Err(PolicyCompileError::new(format!(
                "unknown link_type '{}' at {ctx}",
                related.link_type
            ))
            .into());
        }
        Ok(CompiledPredicate::Related(CompiledRelationshipPredicate {
            link_type: related.link_type.clone(),
            direction: related.direction.clone(),
            target_collection: related.target_collection.clone(),
            target_policy: related.target_policy.clone(),
        }))
    }

    fn compile_shares_relation(
        &self,
        relation: &SharesRelationPredicate,
        ctx: &str,
    ) -> Result<CompiledPredicate, AxonError> {
        self.validate_field_path(&relation.field, &format!("{ctx}.field"))?;
        self.validate_subject_ref(&relation.subject_field, &format!("{ctx}.subject_field"))?;
        self.validate_field_path(&relation.target_field, &format!("{ctx}.target_field"))?;
        Ok(CompiledPredicate::SharesRelation(
            CompiledSharesRelationPredicate {
                collection: relation.collection.clone(),
                field: relation.field.clone(),
                subject_field: relation.subject_field.clone(),
                target_field: relation.target_field.clone(),
            },
        ))
    }

    fn validate_subject_ref(&self, subject: &str, ctx: &str) -> Result<(), AxonError> {
        if self.subject_refs.contains(subject) {
            Ok(())
        } else {
            Err(
                PolicyCompileError::new(format!("unknown subject reference '{subject}' at {ctx}"))
                    .into(),
            )
        }
    }

    fn validate_field_path(&self, path: &str, ctx: &str) -> Result<(), AxonError> {
        let Some(entity_schema) = &self.schema.entity_schema else {
            return Err(PolicyCompileError::new(format!(
                "field path '{path}' at {ctx} cannot be validated without entity_schema"
            ))
            .into());
        };
        let mut current = entity_schema;
        for (idx, raw_segment) in path.split('.').enumerate() {
            if raw_segment.is_empty() {
                return Err(PolicyCompileError::new(format!(
                    "empty field path segment in '{path}' at {ctx}"
                ))
                .into());
            }
            let expects_array = raw_segment.ends_with("[]");
            let segment = raw_segment.strip_suffix("[]").unwrap_or(raw_segment);
            current = object_property(current, segment).ok_or_else(|| {
                PolicyCompileError::new(format!(
                    "unknown field path '{path}' at {ctx}: missing segment '{segment}'"
                ))
            })?;

            if expects_array {
                current = array_items(current).ok_or_else(|| {
                    PolicyCompileError::new(format!(
                        "field path '{path}' at {ctx}: segment '{segment}' is not an array"
                    ))
                })?;
            } else if idx + 1 < path.split('.').count() && schema_type(current) == Some("array") {
                return Err(PolicyCompileError::new(format!(
                    "field path '{path}' at {ctx}: array segment '{segment}' must use []"
                ))
                .into());
            }
        }
        Ok(())
    }
}

fn builtin_subject_refs() -> HashSet<String> {
    [
        "user_id",
        "agent_id",
        "delegated_by",
        "tenant_id",
        "database_id",
        "tenant_role",
        "credential_id",
        "grant_version",
        "grants",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn object_property<'a>(schema: &'a Value, segment: &str) -> Option<&'a Value> {
    schema.get("properties")?.get(segment)
}

fn array_items(schema: &Value) -> Option<&Value> {
    if schema_type(schema) != Some("array") {
        return None;
    }
    schema.get("items")
}

fn schema_type(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::EsfDocument;

    const PROCUREMENT_POLICY_ESF: &str = r#"
esf_version: "1.0"
collection: purchase_orders
entity_schema:
  type: object
  required: [status, amount_cents, requester_id, line_items]
  properties:
    status: { type: string, enum: [draft, submitted, approved, rejected] }
    amount_cents: { type: integer }
    requester_id: { type: string }
    department_id: { type: string }
    line_items:
      type: array
      items:
        type: object
        required: [sku, cost_cents]
        properties:
          sku: { type: string }
          cost_cents: { type: integer }
    restricted_notes: { type: string }
link_types:
  belongs_to_department:
    target_collection: departments
    cardinality: many-to-one
    required: false
access_control:
  identity:
    user_id: subject.user_id
    role: subject.attributes.procurement_role
    department_id: subject.attributes.department_id
  read:
    allow:
      - name: buyers-read-department-orders
        when:
          all:
            - { subject: role, in: [buyer, manager] }
            - not:
                field: status
                eq: rejected
        where: { field: department_id, eq_subject: department_id }
      - name: requester-reads-own-orders
        where: { field: requester_id, eq_subject: user_id }
      - name: sku-watchers-read-matching-lines
        where: { field: "line_items[].sku", contains_subject: department_id }
  write:
    allow:
      - name: managers-write-midrange-orders
        when: { subject: role, eq: manager }
        where:
          all:
            - { field: amount_cents, gte: 10000 }
            - { field: amount_cents, lte: 1000000 }
  fields:
    restricted_notes:
      read:
        deny:
          - name: buyers-do-not-see-restricted-notes
            when: { subject: role, eq: buyer }
            redact_as: null
  envelopes:
    write:
      - name: auto-approve-small-order
        when:
          all:
            - { operation: update }
            - { field: amount_cents, lt: 1000000 }
        decision: allow
      - name: require-approval-large-order
        when:
          all:
            - { operation: update }
            - { field: amount_cents, gt: 1000000 }
        decision: needs_approval
        approval:
          role: finance_approver
          reason_required: true
"#;

    fn compile_fixture(input: &str) -> Result<PolicyPlan, AxonError> {
        let schema = EsfDocument::parse(input)
            .expect("fixture should parse")
            .into_collection_schema()
            .expect("fixture should convert");
        compile_policy_plan(&schema).map(|plan| plan.expect("policy should be present"))
    }

    #[test]
    fn compiles_procurement_style_policy_plan() {
        let plan = compile_fixture(PROCUREMENT_POLICY_ESF).expect("policy should compile");
        let read = plan
            .operations
            .get(&PolicyOperation::Read)
            .expect("read policy missing");
        assert_eq!(read.allow.len(), 3);
        assert!(matches!(
            read.allow[0].when.as_ref(),
            Some(CompiledPredicate::All(_))
        ));

        let write = plan
            .operations
            .get(&PolicyOperation::Write)
            .expect("write policy missing");
        assert!(matches!(
            write.allow[0].where_clause.as_ref(),
            Some(CompiledPredicate::All(_))
        ));

        let field = plan
            .fields
            .get("restricted_notes")
            .expect("field policy missing");
        let deny = &field.read.as_ref().expect("read policy missing").deny[0];
        assert_eq!(deny.redact_as.as_ref(), Some(&Value::Null));
    }

    #[test]
    fn compiles_array_membership_path() {
        let plan = compile_fixture(PROCUREMENT_POLICY_ESF).expect("policy should compile");
        let read = plan.operations.get(&PolicyOperation::Read).unwrap();
        let predicate = read.allow[2]
            .where_clause
            .as_ref()
            .expect("array membership predicate missing");
        assert_eq!(
            predicate,
            &CompiledPredicate::Compare(CompiledComparison {
                target: PredicateTarget::Field("line_items[].sku".to_string()),
                op: CompiledCompareOp::ContainsSubject("department_id".to_string()),
            })
        );
    }

    #[test]
    fn compiles_range_predicates_for_filters_and_envelopes() {
        let plan = compile_fixture(PROCUREMENT_POLICY_ESF).expect("policy should compile");
        let write = plan.operations.get(&PolicyOperation::Write).unwrap();
        let Some(CompiledPredicate::All(range_terms)) = write.allow[0].where_clause.as_ref() else {
            panic!("write policy should compile range predicates as all()");
        };
        assert!(matches!(
            range_terms[0],
            CompiledPredicate::Compare(CompiledComparison {
                op: CompiledCompareOp::Gte(_),
                ..
            })
        ));
        assert!(matches!(
            range_terms[1],
            CompiledPredicate::Compare(CompiledComparison {
                op: CompiledCompareOp::Lte(_),
                ..
            })
        ));

        let envelopes = plan.envelopes.get(&PolicyOperation::Write).unwrap();
        let Some(CompiledPredicate::All(envelope_terms)) = envelopes[1].when.as_ref() else {
            panic!("envelope should compile range predicates as all()");
        };
        assert!(matches!(
            envelope_terms[1],
            CompiledPredicate::Compare(CompiledComparison {
                op: CompiledCompareOp::Gt(_),
                ..
            })
        ));
    }

    #[test]
    fn invalid_subject_ref_returns_stable_diagnostic() {
        let input = PROCUREMENT_POLICY_ESF.replace(
            "{ subject: role, eq: manager }",
            "{ subject: missing_role, eq: manager }",
        );
        let err = compile_fixture(&input).expect_err("invalid subject should fail");
        assert_eq!(
            err.to_string(),
            "schema validation failed: policy_expression_invalid: unknown subject reference 'missing_role' at operations.Write.allow[0].when"
        );
    }

    #[test]
    fn invalid_field_ref_returns_stable_diagnostic() {
        let input = PROCUREMENT_POLICY_ESF.replace(
            "{ field: requester_id, eq_subject: user_id }",
            "{ field: approver_id, eq_subject: user_id }",
        );
        let err = compile_fixture(&input).expect_err("invalid field should fail");
        assert_eq!(
            err.to_string(),
            "schema validation failed: policy_expression_invalid: unknown field path 'approver_id' at operations.Read.allow[1].where: missing segment 'approver_id'"
        );
    }

    #[test]
    fn array_path_without_marker_returns_stable_diagnostic() {
        let input = PROCUREMENT_POLICY_ESF.replace("line_items[].sku", "line_items.sku");
        let err = compile_fixture(&input).expect_err("invalid array path should fail");
        assert_eq!(
            err.to_string(),
            "schema validation failed: policy_expression_invalid: field path 'line_items.sku' at operations.Read.allow[2].where: array segment 'line_items' must use []"
        );
    }
}
