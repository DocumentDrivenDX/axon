//! Access-control policy compilation.
//!
//! The parser in `access_control` preserves the authoring AST. This module
//! normalizes predicates into a plan shape that downstream policy evaluation,
//! GraphQL generation, and MCP metadata can consume.
//!
//! `clippy::result_large_err` is allowed module-wide: `PolicyCompileError`
//! intentionally carries structured fields (`code`, `path`, `collection`,
//! `rule_id`, `field`) so admin-UI clients can focus the first actionable
//! error. Compile-error paths are cold; the size penalty is irrelevant.

#![allow(clippy::result_large_err)]

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
    let mut catalog = compile_policy_catalog(std::slice::from_ref(schema))?;
    Ok(catalog.plans.remove(schema.collection.as_str()))
}

/// Compile all active collection policies into a catalog-level plan.
///
/// Relationship predicates that reuse another collection's `target_policy`
/// require the full schema set so target collections and recursive policy
/// references can be validated before activation.
///
/// Returns the typed [`PolicyCompileError`] so callers that want to capture
/// the failure as a structured diagnostic (admin UI dry-run path) can do so;
/// the existing `From<PolicyCompileError> for AxonError` keeps the wire
/// format stable for callers that bubble via `?`.
pub fn compile_policy_catalog(
    schemas: &[CollectionSchema],
) -> Result<PolicyCatalog, PolicyCompileError> {
    let schemas_by_collection = schemas_by_collection(schemas)?;
    let mut plans = HashMap::new();
    for schema in schemas {
        if let Some(policy) = &schema.access_control {
            let plan = PolicyCompiler::new(schema, policy, &schemas_by_collection).compile()?;
            plans.insert(schema.collection.to_string(), plan);
        }
    }
    detect_relationship_policy_cycles(&plans)?;
    let report = aggregate_policy_compile_reports(plans.values());
    Ok(PolicyCatalog { plans, report })
}

/// Stable policy compilation diagnostic.
///
/// Carries both the human-readable message and the structured fields the
/// admin UI uses to focus the first actionable error: the JSON path within
/// the `access_control` block, the owning collection, and (when known) the
/// rule ID and entity-schema field path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCompileError {
    code: String,
    message: String,
    path: Option<String>,
    collection: Option<String>,
    rule_id: Option<String>,
    field: Option<String>,
}

/// Default stable code for compile errors.
///
/// Matches the historical Display prefix so `From<PolicyCompileError> for
/// AxonError` keeps the existing
/// `schema validation failed: policy_expression_invalid: …` wire format.
pub const POLICY_COMPILE_ERROR_DEFAULT_CODE: &str = "policy_expression_invalid";

impl PolicyCompileError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            code: POLICY_COMPILE_ERROR_DEFAULT_CODE.to_string(),
            message: message.into(),
            path: None,
            collection: None,
            rule_id: None,
            field: None,
        }
    }

    pub(crate) fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub(crate) fn with_collection(mut self, collection: impl Into<String>) -> Self {
        self.collection = Some(collection.into());
        self
    }

    pub(crate) fn with_rule_id_if_unset(mut self, rule_id: impl Into<String>) -> Self {
        if self.rule_id.is_none() {
            self.rule_id = Some(rule_id.into());
        }
        self
    }

    pub(crate) fn with_field_if_unset(mut self, field: impl Into<String>) -> Self {
        if self.field.is_none() {
            self.field = Some(field.into());
        }
        self
    }

    /// Stable code (defaults to `policy_expression_invalid`).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Human-readable detail without the `SchemaValidation` wrapper.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// JSON path within the `access_control` block where the error fired.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Collection that owns the policy block.
    pub fn collection(&self) -> Option<&str> {
        self.collection.as_deref()
    }

    /// Stable rule identifier when the error originated inside a named rule.
    pub fn rule_id(&self) -> Option<&str> {
        self.rule_id.as_deref()
    }

    /// Entity-schema field path when the error references a field.
    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }
}

impl fmt::Display for PolicyCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<PolicyCompileError> for AxonError {
    fn from(err: PolicyCompileError) -> Self {
        AxonError::SchemaValidation(err.to_string())
    }
}

/// Serializable compile diagnostic surfaced to admin UI clients.
///
/// `PolicyCompileError` is the in-process error value; this is the wire
/// shape carried inside `PolicyCompileReport.errors` and
/// `PolicyCompileReport.warnings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyCompileDiagnostic {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl From<&PolicyCompileError> for PolicyCompileDiagnostic {
    fn from(err: &PolicyCompileError) -> Self {
        Self {
            code: err.code.clone(),
            message: err.message.clone(),
            path: err.path.clone(),
            collection: err.collection.clone(),
            rule_id: err.rule_id.clone(),
            field: err.field.clone(),
        }
    }
}

impl From<PolicyCompileError> for PolicyCompileDiagnostic {
    fn from(err: PolicyCompileError) -> Self {
        (&err).into()
    }
}

/// Catalog-level policy compilation output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PolicyCatalog {
    /// Plans keyed by collection name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub plans: HashMap<String, PolicyPlan>,

    /// Cross-plan compile report.
    #[serde(default, skip_serializing_if = "PolicyCompileReport::is_empty")]
    pub report: PolicyCompileReport,
}

/// Stable policy compile report for activation dry-runs and admin UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PolicyCompileReport {
    /// Compile errors. Non-empty means the report came from a failed compile
    /// and the admin UI should focus the first entry. Activation paths must
    /// refuse to persist when this is non-empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<PolicyCompileDiagnostic>,

    /// Compile warnings. Reserved for forward compatibility; no producer
    /// emits warnings yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PolicyCompileDiagnostic>,

    /// Link-table indexes required to evaluate relationship predicates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_link_indexes: Vec<RequiredLinkIndex>,

    /// GraphQL fields that must become nullable because policy can redact them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nullable_fields: Vec<PolicyNullableField>,

    /// Fields with explicit write-deny policy rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_write_fields: Vec<PolicyDeniedWriteField>,

    /// Decision envelope summaries for GraphQL/MCP introspection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelope_summaries: Vec<PolicyEnvelopeSummary>,
}

impl PolicyCompileReport {
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
            && self.warnings.is_empty()
            && self.required_link_indexes.is_empty()
            && self.nullable_fields.is_empty()
            && self.denied_write_fields.is_empty()
            && self.envelope_summaries.is_empty()
    }

    /// Build a failure report carrying a single structured diagnostic. Used
    /// by callers that want to surface a compile failure as part of a
    /// dry-run response instead of bubbling the error.
    pub fn from_compile_error(err: &PolicyCompileError) -> Self {
        Self {
            errors: vec![err.into()],
            ..Self::default()
        }
    }
}

/// Link-table index requirement emitted by relationship predicates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RequiredLinkIndex {
    /// Stable storage index name.
    pub name: String,
    /// Link source collection in storage.
    pub source_collection: String,
    /// Link type.
    pub link_type: String,
    /// Link target collection in storage.
    pub target_collection: String,
    /// Relationship lookup direction from the policy row.
    pub direction: LinkDirection,
}

impl RequiredLinkIndex {
    fn new(
        direction: LinkDirection,
        source_collection: impl Into<String>,
        link_type: impl Into<String>,
        target_collection: impl Into<String>,
    ) -> Self {
        let name = match &direction {
            LinkDirection::Outgoing => "links_primary",
            LinkDirection::Incoming => "idx_links_target",
        };
        Self {
            name: name.to_string(),
            source_collection: source_collection.into(),
            link_type: link_type.into(),
            target_collection: target_collection.into(),
            direction,
        }
    }
}

/// Field nullability consequence emitted by field read-redaction policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyNullableField {
    pub collection: String,
    pub field: String,
    pub required_by_schema: bool,
    pub graphql_nullable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rule_ids: Vec<String>,
}

/// Field write-denial consequence emitted by field write policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDeniedWriteField {
    pub collection: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rule_ids: Vec<String>,
}

/// Summary of a policy decision envelope for capability metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEnvelopeSummary {
    pub collection: String,
    pub operation: PolicyOperation,
    pub envelope_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub decision: PolicyDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalRoute>,
}

/// Explanation metadata for policy rules and envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PolicyExplainPlan {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<PolicyExplainEntry>,
}

impl PolicyExplainPlan {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One rule/envelope explanation entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyExplainEntry {
    pub rule_id: String,
    pub collection: String,
    pub operation: PolicyOperation,
    pub kind: PolicyExplainKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<PolicyDecision>,
}

/// Explanation entry type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyExplainKind {
    OperationAllow,
    OperationDeny,
    FieldReadAllow,
    FieldReadDeny,
    FieldWriteAllow,
    FieldWriteDeny,
    TransitionAllow,
    TransitionDeny,
    Envelope,
}

/// Normalized policy plan for one collection schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PolicyPlan {
    /// Policy version bound to the collection schema version.
    pub policy_version: u32,

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

    /// Collection-local compile report.
    #[serde(default, skip_serializing_if = "PolicyCompileReport::is_empty")]
    pub report: PolicyCompileReport,

    /// Explanation metadata consumed by GraphQL/MCP policy introspection.
    #[serde(default, skip_serializing_if = "PolicyExplainPlan::is_empty")]
    pub explain: PolicyExplainPlan,
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
    pub rule_id: String,
    pub path: String,
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
    pub rule_id: String,
    pub path: String,
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
    pub envelope_id: String,
    pub path: String,
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
    pub direction: LinkDirection,
    /// Related entity collection named by the authored predicate.
    pub target_collection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_policy: Option<PolicyOperation>,
    /// Link source collection in storage.
    pub link_source_collection: String,
    /// Link target collection in storage.
    pub link_target_collection: String,
    /// Link-table index required by this relationship lookup.
    pub required_link_index: RequiredLinkIndex,
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
    schemas: &'a HashMap<String, &'a CollectionSchema>,
    subject_refs: HashSet<String>,
}

impl<'a> PolicyCompiler<'a> {
    fn new(
        schema: &'a CollectionSchema,
        policy: &'a AccessControlPolicy,
        schemas: &'a HashMap<String, &'a CollectionSchema>,
    ) -> Self {
        let mut subject_refs = builtin_subject_refs();
        if let Some(identity) = &policy.identity {
            subject_refs.extend(identity.subject.keys().cloned());
            subject_refs.extend(identity.attributes.keys().cloned());
            subject_refs.extend(identity.aliases.keys().cloned());
        }
        Self {
            schema,
            policy,
            schemas,
            subject_refs,
        }
    }

    fn compile(&self) -> Result<PolicyPlan, PolicyCompileError> {
        let mut plan = PolicyPlan {
            policy_version: self.schema.version,
            ..PolicyPlan::default()
        };
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
            self.validate_field_path(field, &format!("fields.{field}"))
                .map_err(|e| e.with_field_if_unset(field.clone()))?;
            plan.fields.insert(
                field.clone(),
                self.compile_field_policy(field, policy)
                    .map_err(|e| e.with_field_if_unset(field.clone()))?,
            );
        }

        for (field, transitions) in &self.policy.transitions {
            self.validate_field_path(field, &format!("transitions.{field}"))
                .map_err(|e| e.with_field_if_unset(field.clone()))?;
            let mut compiled_transitions = HashMap::new();
            for (transition, policy) in transitions {
                let ctx = format!("transitions.{field}.{transition}");
                compiled_transitions.insert(
                    transition.clone(),
                    self.compile_operation_policy(policy, &ctx)
                        .map_err(|e| e.with_field_if_unset(field.clone()))?,
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

        plan.explain = build_explain_plan(self.schema.collection.as_str(), &plan);
        plan.report = self.compile_report(&plan);
        Ok(plan)
    }

    /// Build a [`PolicyCompileError`] pre-populated with the access_control
    /// JSON path and the owning collection. Used at every error site inside
    /// the compiler so admin-UI clients can focus the failing rule.
    fn err_at(&self, ctx: &str, message: impl Into<String>) -> PolicyCompileError {
        PolicyCompileError::new(message)
            .with_path(ctx.to_string())
            .with_collection(self.schema.collection.to_string())
    }

    fn compile_operation(
        &self,
        plan: &mut PolicyPlan,
        operation: PolicyOperation,
        policy: Option<&OperationPolicy>,
    ) -> Result<(), PolicyCompileError> {
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
    ) -> Result<CompiledOperationPolicy, PolicyCompileError> {
        Ok(CompiledOperationPolicy {
            allow: self.compile_policy_rules(&policy.allow, &format!("{ctx}.allow"))?,
            deny: self.compile_policy_rules(&policy.deny, &format!("{ctx}.deny"))?,
        })
    }

    fn compile_policy_rules(
        &self,
        rules: &[PolicyRule],
        ctx: &str,
    ) -> Result<Vec<CompiledPolicyRule>, PolicyCompileError> {
        rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| {
                let rule_ctx = format!("{ctx}[{idx}]");
                let rule_id = self.stable_policy_id("rule", &rule_ctx, rule.name.as_deref());
                let when = self
                    .compile_optional_predicate(rule.when.as_ref(), &format!("{rule_ctx}.when"))
                    .map_err(|e| e.with_rule_id_if_unset(rule_id.clone()))?;
                let where_clause = self
                    .compile_optional_predicate(
                        rule.where_clause.as_ref(),
                        &format!("{rule_ctx}.where"),
                    )
                    .map_err(|e| e.with_rule_id_if_unset(rule_id.clone()))?;
                Ok(CompiledPolicyRule {
                    rule_id,
                    path: rule_ctx,
                    name: rule.name.clone(),
                    when,
                    where_clause,
                })
            })
            .collect()
    }

    fn compile_field_policy(
        &self,
        field: &str,
        policy: &FieldPolicy,
    ) -> Result<CompiledFieldPolicy, PolicyCompileError> {
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
    ) -> Result<CompiledFieldAccessPolicy, PolicyCompileError> {
        Ok(CompiledFieldAccessPolicy {
            allow: self.compile_field_rules(&policy.allow, &format!("{ctx}.allow"))?,
            deny: self.compile_field_rules(&policy.deny, &format!("{ctx}.deny"))?,
        })
    }

    fn compile_field_rules(
        &self,
        rules: &[FieldPolicyRule],
        ctx: &str,
    ) -> Result<Vec<CompiledFieldPolicyRule>, PolicyCompileError> {
        rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| {
                let rule_ctx = format!("{ctx}[{idx}]");
                let rule_id = self.stable_policy_id("rule", &rule_ctx, rule.name.as_deref());
                let when = self
                    .compile_optional_predicate(rule.when.as_ref(), &format!("{rule_ctx}.when"))
                    .map_err(|e| e.with_rule_id_if_unset(rule_id.clone()))?;
                let where_clause = self
                    .compile_optional_predicate(
                        rule.where_clause.as_ref(),
                        &format!("{rule_ctx}.where"),
                    )
                    .map_err(|e| e.with_rule_id_if_unset(rule_id.clone()))?;
                Ok(CompiledFieldPolicyRule {
                    rule_id,
                    path: rule_ctx,
                    name: rule.name.clone(),
                    when,
                    where_clause,
                    redact_as: rule.redact_as.clone(),
                })
            })
            .collect()
    }

    fn compile_envelope(
        &self,
        envelope: &PolicyEnvelope,
        ctx: &str,
    ) -> Result<CompiledPolicyEnvelope, PolicyCompileError> {
        Ok(CompiledPolicyEnvelope {
            envelope_id: self.stable_policy_id("envelope", ctx, envelope.name.as_deref()),
            path: ctx.to_string(),
            name: envelope.name.clone(),
            when: self
                .compile_optional_predicate(envelope.when.as_ref(), &format!("{ctx}.when"))?,
            decision: envelope.decision.clone(),
            approval: envelope.approval.clone(),
        })
    }

    fn compile_report(&self, plan: &PolicyPlan) -> PolicyCompileReport {
        let mut report = PolicyCompileReport {
            errors: Vec::new(),
            warnings: Vec::new(),
            required_link_indexes: aggregate_required_link_indexes(std::iter::once(plan)),
            nullable_fields: nullable_fields_for_plan(self.schema, plan),
            denied_write_fields: denied_write_fields_for_plan(
                self.schema.collection.as_str(),
                plan,
            ),
            envelope_summaries: envelope_summaries_for_plan(self.schema.collection.as_str(), plan),
        };
        sort_policy_compile_report(&mut report);
        report
    }

    fn stable_policy_id(&self, prefix: &str, ctx: &str, name: Option<&str>) -> String {
        stable_policy_id(self.schema.collection.as_str(), prefix, ctx, name)
    }

    fn compile_optional_predicate(
        &self,
        predicate: Option<&PolicyPredicate>,
        ctx: &str,
    ) -> Result<Option<CompiledPredicate>, PolicyCompileError> {
        predicate
            .map(|predicate| self.compile_predicate(predicate, ctx))
            .transpose()
    }

    fn compile_predicate(
        &self,
        predicate: &PolicyPredicate,
        ctx: &str,
    ) -> Result<CompiledPredicate, PolicyCompileError> {
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
    ) -> Result<CompiledCompareOp, PolicyCompileError> {
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
    ) -> Result<CompiledPredicate, PolicyCompileError> {
        let direction = related.direction.clone().unwrap_or(LinkDirection::Outgoing);
        let related_schema = self
            .schemas
            .get(&related.target_collection)
            .ok_or_else(|| {
                self.err_at(
                    ctx,
                    format!(
                        "unknown target_collection '{}' at {ctx}",
                        related.target_collection
                    ),
                )
            })?;
        let current_collection = self.schema.collection.as_str();

        let (link_source_collection, link_target_collection) = match direction {
            LinkDirection::Outgoing => {
                let link_def = self
                    .schema
                    .link_types
                    .get(&related.link_type)
                    .ok_or_else(|| {
                        self.err_at(
                            ctx,
                            format!("unknown link_type '{}' at {ctx}", related.link_type),
                        )
                    })?;
                if link_def.target_collection != related.target_collection {
                    return Err(self.err_at(
                        ctx,
                        format!(
                            "link_type '{}' at {ctx} targets collection '{}', not '{}'",
                            related.link_type,
                            link_def.target_collection,
                            related.target_collection
                        ),
                    ));
                }
                (
                    current_collection.to_string(),
                    related.target_collection.clone(),
                )
            }
            LinkDirection::Incoming => {
                let link_def = related_schema
                    .link_types
                    .get(&related.link_type)
                    .ok_or_else(|| {
                        self.err_at(
                            ctx,
                            format!(
                                "unknown incoming link_type '{}' on source collection '{}' at {ctx}",
                                related.link_type, related.target_collection
                            ),
                        )
                    })?;
                if link_def.target_collection != current_collection {
                    return Err(self.err_at(
                        ctx,
                        format!(
                            "incoming link_type '{}' from collection '{}' at {ctx} targets collection '{}', not '{}'",
                            related.link_type,
                            related.target_collection,
                            link_def.target_collection,
                            current_collection
                        ),
                    ));
                }
                (
                    related.target_collection.clone(),
                    current_collection.to_string(),
                )
            }
        };

        if let Some(target_policy) = &related.target_policy {
            validate_target_policy(related_schema, target_policy, ctx)?;
        }
        let required_link_index = RequiredLinkIndex::new(
            direction.clone(),
            link_source_collection.clone(),
            related.link_type.clone(),
            link_target_collection.clone(),
        );
        Ok(CompiledPredicate::Related(CompiledRelationshipPredicate {
            link_type: related.link_type.clone(),
            direction,
            target_collection: related.target_collection.clone(),
            target_policy: related.target_policy.clone(),
            link_source_collection,
            link_target_collection,
            required_link_index,
        }))
    }

    fn compile_shares_relation(
        &self,
        relation: &SharesRelationPredicate,
        ctx: &str,
    ) -> Result<CompiledPredicate, PolicyCompileError> {
        self.validate_field_path(&relation.field, &format!("{ctx}.field"))
            .map_err(|e| e.with_field_if_unset(relation.field.clone()))?;
        self.validate_subject_ref(&relation.subject_field, &format!("{ctx}.subject_field"))?;
        self.validate_field_path(&relation.target_field, &format!("{ctx}.target_field"))
            .map_err(|e| e.with_field_if_unset(relation.target_field.clone()))?;
        Ok(CompiledPredicate::SharesRelation(
            CompiledSharesRelationPredicate {
                collection: relation.collection.clone(),
                field: relation.field.clone(),
                subject_field: relation.subject_field.clone(),
                target_field: relation.target_field.clone(),
            },
        ))
    }

    fn validate_subject_ref(&self, subject: &str, ctx: &str) -> Result<(), PolicyCompileError> {
        if self.subject_refs.contains(subject) {
            Ok(())
        } else {
            Err(self.err_at(
                ctx,
                format!("unknown subject reference '{subject}' at {ctx}"),
            ))
        }
    }

    fn validate_field_path(&self, path: &str, ctx: &str) -> Result<(), PolicyCompileError> {
        let Some(entity_schema) = &self.schema.entity_schema else {
            return Err(self
                .err_at(
                    ctx,
                    format!("field path '{path}' at {ctx} cannot be validated without entity_schema"),
                )
                .with_field_if_unset(path.to_string()));
        };
        let mut current = entity_schema;
        for (idx, raw_segment) in path.split('.').enumerate() {
            if raw_segment.is_empty() {
                return Err(self
                    .err_at(ctx, format!("empty field path segment in '{path}' at {ctx}"))
                    .with_field_if_unset(path.to_string()));
            }
            let expects_array = raw_segment.ends_with("[]");
            let segment = raw_segment.strip_suffix("[]").unwrap_or(raw_segment);
            current = object_property(current, segment).ok_or_else(|| {
                self.err_at(
                    ctx,
                    format!("unknown field path '{path}' at {ctx}: missing segment '{segment}'"),
                )
                .with_field_if_unset(path.to_string())
            })?;

            if expects_array {
                current = array_items(current).ok_or_else(|| {
                    self.err_at(
                        ctx,
                        format!("field path '{path}' at {ctx}: segment '{segment}' is not an array"),
                    )
                    .with_field_if_unset(path.to_string())
                })?;
            } else if idx + 1 < path.split('.').count() && schema_type(current) == Some("array") {
                return Err(self
                    .err_at(
                        ctx,
                        format!("field path '{path}' at {ctx}: array segment '{segment}' must use []"),
                    )
                    .with_field_if_unset(path.to_string()));
            }
        }
        Ok(())
    }
}

fn schemas_by_collection(
    schemas: &[CollectionSchema],
) -> Result<HashMap<String, &CollectionSchema>, PolicyCompileError> {
    let mut by_collection = HashMap::new();
    for schema in schemas {
        let collection = schema.collection.to_string();
        if by_collection.insert(collection.clone(), schema).is_some() {
            return Err(PolicyCompileError::new(format!(
                "duplicate collection schema '{collection}' in policy catalog"
            ))
            .with_collection(collection));
        }
    }
    Ok(by_collection)
}

fn validate_target_policy(
    schema: &CollectionSchema,
    operation: &PolicyOperation,
    ctx: &str,
) -> Result<(), PolicyCompileError> {
    let Some(policy) = &schema.access_control else {
        return Err(PolicyCompileError::new(format!(
            "target_policy '{}' at {ctx} references collection '{}' without access_control",
            operation.as_str(),
            schema.collection
        ))
        .with_path(ctx.to_string())
        .with_collection(schema.collection.to_string()));
    };
    if operation_policy(policy, operation).is_some() {
        Ok(())
    } else {
        Err(PolicyCompileError::new(format!(
            "target_policy '{}' at {ctx} is not declared on collection '{}'",
            operation.as_str(),
            schema.collection
        ))
        .with_path(ctx.to_string())
        .with_collection(schema.collection.to_string()))
    }
}

fn operation_policy<'a>(
    policy: &'a AccessControlPolicy,
    operation: &PolicyOperation,
) -> Option<&'a OperationPolicy> {
    match operation {
        PolicyOperation::Read => policy.read.as_ref(),
        PolicyOperation::Create => policy.create.as_ref(),
        PolicyOperation::Update => policy.update.as_ref(),
        PolicyOperation::Delete => policy.delete.as_ref(),
        PolicyOperation::Write => policy.write.as_ref(),
        PolicyOperation::Admin => policy.admin.as_ref(),
    }
}

fn aggregate_policy_compile_reports<'a>(
    plans: impl IntoIterator<Item = &'a PolicyPlan>,
) -> PolicyCompileReport {
    let mut report = PolicyCompileReport::default();
    for plan in plans {
        report
            .required_link_indexes
            .extend(plan.report.required_link_indexes.iter().cloned());
        report
            .nullable_fields
            .extend(plan.report.nullable_fields.iter().cloned());
        report
            .denied_write_fields
            .extend(plan.report.denied_write_fields.iter().cloned());
        report
            .envelope_summaries
            .extend(plan.report.envelope_summaries.iter().cloned());
    }
    sort_policy_compile_report(&mut report);
    report
}

fn sort_policy_compile_report(report: &mut PolicyCompileReport) {
    report.required_link_indexes.sort();
    report.required_link_indexes.dedup();
    report.nullable_fields.sort_by(|left, right| {
        (
            left.collection.as_str(),
            left.field.as_str(),
            left.required_by_schema,
        )
            .cmp(&(
                right.collection.as_str(),
                right.field.as_str(),
                right.required_by_schema,
            ))
    });
    report.nullable_fields.dedup();
    report.denied_write_fields.sort_by(|left, right| {
        (left.collection.as_str(), left.field.as_str())
            .cmp(&(right.collection.as_str(), right.field.as_str()))
    });
    report.denied_write_fields.dedup();
    report.envelope_summaries.sort_by(|left, right| {
        (
            left.collection.as_str(),
            &left.operation,
            left.envelope_id.as_str(),
        )
            .cmp(&(
                right.collection.as_str(),
                &right.operation,
                right.envelope_id.as_str(),
            ))
    });
    report.envelope_summaries.dedup();
}

fn aggregate_required_link_indexes<'a>(
    plans: impl IntoIterator<Item = &'a PolicyPlan>,
) -> Vec<RequiredLinkIndex> {
    let mut indexes = Vec::new();
    for plan in plans {
        collect_required_link_indexes_from_plan(plan, &mut indexes);
    }
    indexes.sort();
    indexes.dedup();
    indexes
}

fn collect_required_link_indexes_from_plan(
    plan: &PolicyPlan,
    indexes: &mut Vec<RequiredLinkIndex>,
) {
    for operation in plan.operations.values() {
        collect_required_link_indexes_from_operation(operation, indexes);
    }
    for field_policy in plan.fields.values() {
        if let Some(read) = &field_policy.read {
            for rule in read.allow.iter().chain(read.deny.iter()) {
                collect_required_link_indexes_from_optional_predicate(rule.when.as_ref(), indexes);
                collect_required_link_indexes_from_optional_predicate(
                    rule.where_clause.as_ref(),
                    indexes,
                );
            }
        }
        if let Some(write) = &field_policy.write {
            for rule in write.allow.iter().chain(write.deny.iter()) {
                collect_required_link_indexes_from_optional_predicate(rule.when.as_ref(), indexes);
                collect_required_link_indexes_from_optional_predicate(
                    rule.where_clause.as_ref(),
                    indexes,
                );
            }
        }
    }
    for transitions in plan.transitions.values() {
        for operation in transitions.values() {
            collect_required_link_indexes_from_operation(operation, indexes);
        }
    }
    for envelopes in plan.envelopes.values() {
        for envelope in envelopes {
            collect_required_link_indexes_from_optional_predicate(envelope.when.as_ref(), indexes);
        }
    }
}

fn collect_required_link_indexes_from_operation(
    operation: &CompiledOperationPolicy,
    indexes: &mut Vec<RequiredLinkIndex>,
) {
    for rule in operation.allow.iter().chain(operation.deny.iter()) {
        collect_required_link_indexes_from_optional_predicate(rule.when.as_ref(), indexes);
        collect_required_link_indexes_from_optional_predicate(rule.where_clause.as_ref(), indexes);
    }
}

fn collect_required_link_indexes_from_optional_predicate(
    predicate: Option<&CompiledPredicate>,
    indexes: &mut Vec<RequiredLinkIndex>,
) {
    if let Some(predicate) = predicate {
        collect_required_link_indexes_from_predicate(predicate, indexes);
    }
}

fn collect_required_link_indexes_from_predicate(
    predicate: &CompiledPredicate,
    indexes: &mut Vec<RequiredLinkIndex>,
) {
    match predicate {
        CompiledPredicate::All(predicates) | CompiledPredicate::Any(predicates) => {
            for predicate in predicates {
                collect_required_link_indexes_from_predicate(predicate, indexes);
            }
        }
        CompiledPredicate::Not(predicate) => {
            collect_required_link_indexes_from_predicate(predicate, indexes);
        }
        CompiledPredicate::Related(related) => {
            indexes.push(related.required_link_index.clone());
        }
        CompiledPredicate::Compare(_)
        | CompiledPredicate::Operation(_)
        | CompiledPredicate::SharesRelation(_) => {}
    }
}

fn nullable_fields_for_plan(
    schema: &CollectionSchema,
    plan: &PolicyPlan,
) -> Vec<PolicyNullableField> {
    let collection = schema.collection.to_string();
    let mut fields = Vec::new();
    for (field, policy) in &plan.fields {
        let Some(read) = &policy.read else {
            continue;
        };
        let mut rule_ids = read
            .deny
            .iter()
            .filter(|rule| rule.redact_as.is_some())
            .map(|rule| rule.rule_id.clone())
            .collect::<Vec<_>>();
        if rule_ids.is_empty() {
            continue;
        }
        rule_ids.sort();
        rule_ids.dedup();
        fields.push(PolicyNullableField {
            collection: collection.clone(),
            field: field.clone(),
            required_by_schema: field_path_required(schema.entity_schema.as_ref(), field),
            graphql_nullable: true,
            rule_ids,
        });
    }
    fields
}

fn denied_write_fields_for_plan(
    collection: &str,
    plan: &PolicyPlan,
) -> Vec<PolicyDeniedWriteField> {
    let mut fields = Vec::new();
    for (field, policy) in &plan.fields {
        let Some(write) = &policy.write else {
            continue;
        };
        let mut rule_ids = write
            .deny
            .iter()
            .map(|rule| rule.rule_id.clone())
            .collect::<Vec<_>>();
        if rule_ids.is_empty() {
            continue;
        }
        rule_ids.sort();
        rule_ids.dedup();
        fields.push(PolicyDeniedWriteField {
            collection: collection.to_string(),
            field: field.clone(),
            rule_ids,
        });
    }
    fields
}

fn envelope_summaries_for_plan(collection: &str, plan: &PolicyPlan) -> Vec<PolicyEnvelopeSummary> {
    let mut summaries = Vec::new();
    for (operation, envelopes) in &plan.envelopes {
        for envelope in envelopes {
            summaries.push(PolicyEnvelopeSummary {
                collection: collection.to_string(),
                operation: operation.clone(),
                envelope_id: envelope.envelope_id.clone(),
                name: envelope.name.clone(),
                decision: envelope.decision.clone(),
                approval: envelope.approval.clone(),
            });
        }
    }
    summaries
}

fn build_explain_plan(collection: &str, plan: &PolicyPlan) -> PolicyExplainPlan {
    let mut entries = Vec::new();
    for (operation, policy) in &plan.operations {
        collect_operation_explain_entries(collection, operation, policy, &mut entries);
    }
    for (field, policy) in &plan.fields {
        if let Some(read) = &policy.read {
            collect_field_explain_entries(
                collection,
                PolicyOperation::Read,
                field,
                read,
                PolicyExplainKind::FieldReadAllow,
                PolicyExplainKind::FieldReadDeny,
                &mut entries,
            );
        }
        if let Some(write) = &policy.write {
            collect_field_explain_entries(
                collection,
                PolicyOperation::Write,
                field,
                write,
                PolicyExplainKind::FieldWriteAllow,
                PolicyExplainKind::FieldWriteDeny,
                &mut entries,
            );
        }
    }
    for (field, transitions) in &plan.transitions {
        for (transition, policy) in transitions {
            for rule in &policy.allow {
                entries.push(explain_entry(ExplainEntrySpec {
                    collection,
                    operation: PolicyOperation::Update,
                    kind: PolicyExplainKind::TransitionAllow,
                    rule_id: &rule.rule_id,
                    path: &rule.path,
                    name: rule.name.clone(),
                    field_path: Some(field.clone()),
                    transition: Some(transition.clone()),
                    decision: None,
                }));
            }
            for rule in &policy.deny {
                entries.push(explain_entry(ExplainEntrySpec {
                    collection,
                    operation: PolicyOperation::Update,
                    kind: PolicyExplainKind::TransitionDeny,
                    rule_id: &rule.rule_id,
                    path: &rule.path,
                    name: rule.name.clone(),
                    field_path: Some(field.clone()),
                    transition: Some(transition.clone()),
                    decision: None,
                }));
            }
        }
    }
    for (operation, envelopes) in &plan.envelopes {
        for envelope in envelopes {
            entries.push(explain_entry(ExplainEntrySpec {
                collection,
                operation: operation.clone(),
                kind: PolicyExplainKind::Envelope,
                rule_id: &envelope.envelope_id,
                path: &envelope.path,
                name: envelope.name.clone(),
                field_path: None,
                transition: None,
                decision: Some(envelope.decision.clone()),
            }));
        }
    }
    entries.sort_by(|left, right| {
        (left.collection.as_str(), left.rule_id.as_str())
            .cmp(&(right.collection.as_str(), right.rule_id.as_str()))
    });
    entries.dedup();
    PolicyExplainPlan { entries }
}

fn collect_operation_explain_entries(
    collection: &str,
    operation: &PolicyOperation,
    policy: &CompiledOperationPolicy,
    entries: &mut Vec<PolicyExplainEntry>,
) {
    for rule in &policy.allow {
        entries.push(explain_entry(ExplainEntrySpec {
            collection,
            operation: operation.clone(),
            kind: PolicyExplainKind::OperationAllow,
            rule_id: &rule.rule_id,
            path: &rule.path,
            name: rule.name.clone(),
            field_path: None,
            transition: None,
            decision: None,
        }));
    }
    for rule in &policy.deny {
        entries.push(explain_entry(ExplainEntrySpec {
            collection,
            operation: operation.clone(),
            kind: PolicyExplainKind::OperationDeny,
            rule_id: &rule.rule_id,
            path: &rule.path,
            name: rule.name.clone(),
            field_path: None,
            transition: None,
            decision: None,
        }));
    }
}

fn collect_field_explain_entries(
    collection: &str,
    operation: PolicyOperation,
    field: &str,
    policy: &CompiledFieldAccessPolicy,
    allow_kind: PolicyExplainKind,
    deny_kind: PolicyExplainKind,
    entries: &mut Vec<PolicyExplainEntry>,
) {
    for rule in &policy.allow {
        entries.push(explain_entry(ExplainEntrySpec {
            collection,
            operation: operation.clone(),
            kind: allow_kind.clone(),
            rule_id: &rule.rule_id,
            path: &rule.path,
            name: rule.name.clone(),
            field_path: Some(field.to_string()),
            transition: None,
            decision: None,
        }));
    }
    for rule in &policy.deny {
        entries.push(explain_entry(ExplainEntrySpec {
            collection,
            operation: operation.clone(),
            kind: deny_kind.clone(),
            rule_id: &rule.rule_id,
            path: &rule.path,
            name: rule.name.clone(),
            field_path: Some(field.to_string()),
            transition: None,
            decision: None,
        }));
    }
}

struct ExplainEntrySpec<'a> {
    collection: &'a str,
    operation: PolicyOperation,
    kind: PolicyExplainKind,
    rule_id: &'a str,
    path: &'a str,
    name: Option<String>,
    field_path: Option<String>,
    transition: Option<String>,
    decision: Option<PolicyDecision>,
}

fn explain_entry(spec: ExplainEntrySpec<'_>) -> PolicyExplainEntry {
    PolicyExplainEntry {
        rule_id: spec.rule_id.to_string(),
        collection: spec.collection.to_string(),
        operation: spec.operation,
        kind: spec.kind,
        path: spec.path.to_string(),
        name: spec.name,
        field_path: spec.field_path,
        transition: spec.transition,
        decision: spec.decision,
    }
}

fn stable_policy_id(collection: &str, prefix: &str, ctx: &str, name: Option<&str>) -> String {
    let collection = normalize_policy_id_part(collection);
    let ctx = normalize_policy_id_part(ctx);
    match name {
        Some(name) if !name.is_empty() => {
            format!(
                "{prefix}:{collection}:{ctx}:{}",
                normalize_policy_id_part(name)
            )
        }
        _ => format!("{prefix}:{collection}:{ctx}"),
    }
}

fn normalize_policy_id_part(input: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if matches!(ch, '-' | '_') {
            output.push(ch);
            last_was_separator = false;
        } else if !last_was_separator && !output.is_empty() {
            output.push('.');
            last_was_separator = true;
        }
    }
    while output.ends_with('.') {
        let _ = output.pop();
    }
    if output.is_empty() {
        "unnamed".to_string()
    } else {
        output
    }
}

fn field_path_required(schema: Option<&Value>, path: &str) -> bool {
    let Some(mut current) = schema else {
        return false;
    };
    for raw_segment in path.split('.') {
        if raw_segment.is_empty() {
            return false;
        }
        let expects_array = raw_segment.ends_with("[]");
        let segment = raw_segment.strip_suffix("[]").unwrap_or(raw_segment);
        if !object_required_contains(current, segment) {
            return false;
        }
        let Some(next) = object_property(current, segment) else {
            return false;
        };
        current = if expects_array {
            match array_items(next) {
                Some(items) => items,
                None => return false,
            }
        } else {
            next
        };
    }
    true
}

fn object_required_contains(schema: &Value, segment: &str) -> bool {
    schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.iter().any(|value| value.as_str() == Some(segment)))
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PolicyNode {
    collection: String,
    operation: PolicyOperation,
}

fn detect_relationship_policy_cycles(
    plans: &HashMap<String, PolicyPlan>,
) -> Result<(), PolicyCompileError> {
    let mut adjacency: HashMap<PolicyNode, Vec<PolicyNode>> = HashMap::new();
    for (collection, plan) in plans {
        collect_relationship_policy_edges(collection, plan, &mut adjacency);
    }
    for targets in adjacency.values_mut() {
        targets.sort();
        targets.dedup();
    }
    let mut nodes: Vec<PolicyNode> = adjacency.keys().cloned().collect();
    nodes.sort();
    nodes.dedup();
    let mut permanent = HashSet::new();
    let mut stack = Vec::new();
    for node in nodes {
        if let Some(cycle) =
            relationship_policy_cycle_from(&node, &adjacency, &mut permanent, &mut stack)
        {
            let labels = cycle
                .iter()
                .map(policy_node_label)
                .collect::<Vec<_>>()
                .join(" -> ");
            let collection = cycle
                .first()
                .map(|node| node.collection.clone())
                .unwrap_or_default();
            return Err(PolicyCompileError::new(format!(
                "relationship target_policy cycle detected: {labels}"
            ))
            .with_collection(collection));
        }
    }
    Ok(())
}

fn collect_relationship_policy_edges(
    collection: &str,
    plan: &PolicyPlan,
    adjacency: &mut HashMap<PolicyNode, Vec<PolicyNode>>,
) {
    for (operation, policy) in &plan.operations {
        let from = PolicyNode {
            collection: collection.to_string(),
            operation: operation.clone(),
        };
        collect_relationship_policy_edges_from_operation(&from, policy, adjacency);
    }
    for field_policy in plan.fields.values() {
        if let Some(read) = &field_policy.read {
            let from = PolicyNode {
                collection: collection.to_string(),
                operation: PolicyOperation::Read,
            };
            collect_relationship_policy_edges_from_field_access(&from, read, adjacency);
        }
        if let Some(write) = &field_policy.write {
            let from = PolicyNode {
                collection: collection.to_string(),
                operation: PolicyOperation::Write,
            };
            collect_relationship_policy_edges_from_field_access(&from, write, adjacency);
        }
    }
    for transitions in plan.transitions.values() {
        for policy in transitions.values() {
            let from = PolicyNode {
                collection: collection.to_string(),
                operation: PolicyOperation::Update,
            };
            collect_relationship_policy_edges_from_operation(&from, policy, adjacency);
        }
    }
    for (operation, envelopes) in &plan.envelopes {
        let from = PolicyNode {
            collection: collection.to_string(),
            operation: operation.clone(),
        };
        for envelope in envelopes {
            collect_relationship_policy_edges_from_optional_predicate(
                &from,
                envelope.when.as_ref(),
                adjacency,
            );
        }
    }
}

fn collect_relationship_policy_edges_from_operation(
    from: &PolicyNode,
    policy: &CompiledOperationPolicy,
    adjacency: &mut HashMap<PolicyNode, Vec<PolicyNode>>,
) {
    for rule in policy.allow.iter().chain(policy.deny.iter()) {
        collect_relationship_policy_edges_from_optional_predicate(
            from,
            rule.when.as_ref(),
            adjacency,
        );
        collect_relationship_policy_edges_from_optional_predicate(
            from,
            rule.where_clause.as_ref(),
            adjacency,
        );
    }
}

fn collect_relationship_policy_edges_from_field_access(
    from: &PolicyNode,
    policy: &CompiledFieldAccessPolicy,
    adjacency: &mut HashMap<PolicyNode, Vec<PolicyNode>>,
) {
    for rule in policy.allow.iter().chain(policy.deny.iter()) {
        collect_relationship_policy_edges_from_optional_predicate(
            from,
            rule.when.as_ref(),
            adjacency,
        );
        collect_relationship_policy_edges_from_optional_predicate(
            from,
            rule.where_clause.as_ref(),
            adjacency,
        );
    }
}

fn collect_relationship_policy_edges_from_optional_predicate(
    from: &PolicyNode,
    predicate: Option<&CompiledPredicate>,
    adjacency: &mut HashMap<PolicyNode, Vec<PolicyNode>>,
) {
    if let Some(predicate) = predicate {
        collect_relationship_policy_edges_from_predicate(from, predicate, adjacency);
    }
}

fn collect_relationship_policy_edges_from_predicate(
    from: &PolicyNode,
    predicate: &CompiledPredicate,
    adjacency: &mut HashMap<PolicyNode, Vec<PolicyNode>>,
) {
    match predicate {
        CompiledPredicate::All(predicates) | CompiledPredicate::Any(predicates) => {
            for predicate in predicates {
                collect_relationship_policy_edges_from_predicate(from, predicate, adjacency);
            }
        }
        CompiledPredicate::Not(predicate) => {
            collect_relationship_policy_edges_from_predicate(from, predicate, adjacency);
        }
        CompiledPredicate::Related(related) => {
            if let Some(target_policy) = &related.target_policy {
                adjacency.entry(from.clone()).or_default().push(PolicyNode {
                    collection: related.target_collection.clone(),
                    operation: target_policy.clone(),
                });
            }
        }
        CompiledPredicate::Compare(_)
        | CompiledPredicate::Operation(_)
        | CompiledPredicate::SharesRelation(_) => {}
    }
}

fn relationship_policy_cycle_from(
    node: &PolicyNode,
    adjacency: &HashMap<PolicyNode, Vec<PolicyNode>>,
    permanent: &mut HashSet<PolicyNode>,
    stack: &mut Vec<PolicyNode>,
) -> Option<Vec<PolicyNode>> {
    if permanent.contains(node) {
        return None;
    }
    if let Some(position) = stack.iter().position(|entry| entry == node) {
        let mut cycle = stack[position..].to_vec();
        cycle.push(node.clone());
        return Some(cycle);
    }
    stack.push(node.clone());
    if let Some(targets) = adjacency.get(node) {
        for target in targets {
            if let Some(cycle) = relationship_policy_cycle_from(target, adjacency, permanent, stack)
            {
                return Some(cycle);
            }
        }
    }
    let _ = stack.pop();
    permanent.insert(node.clone());
    None
}

fn policy_node_label(node: &PolicyNode) -> String {
    format!("{}.{}", node.collection, node.operation.as_str())
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
  required: [status, amount_cents, requester_id, line_items, restricted_notes]
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
      write:
        deny:
          - name: buyers-cannot-write-restricted-notes
            when: { subject: role, eq: buyer }
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
          deadline_seconds: 86400
          separation_of_duties: true
      - name: deny-rejected-order-updates
        when:
          all:
            - { operation: update }
            - { field: status, eq: rejected }
        decision: deny
"#;

    fn compile_fixture(input: &str) -> Result<PolicyPlan, AxonError> {
        let schema = EsfDocument::parse(input)
            .expect("fixture should parse")
            .into_collection_schema()
            .expect("fixture should convert");
        compile_policy_plan(&schema).map(|plan| plan.expect("policy should be present"))
    }

    fn parse_schema(input: &str) -> CollectionSchema {
        EsfDocument::parse(input)
            .expect("fixture should parse")
            .into_collection_schema()
            .expect("fixture should convert")
    }

    const ENGAGEMENTS_POLICY_ESF: &str = r#"
esf_version: "1.0"
collection: engagements
entity_schema:
  type: object
  required: [members]
  properties:
    members:
      type: array
      items:
        type: object
        required: [user_id]
        properties:
          user_id: { type: string }
access_control:
  identity:
    user_id: subject.user_id
  read:
    allow:
      - name: members-read-engagements
        where: { field: "members[].user_id", contains_subject: user_id }
"#;

    const CONTRACTS_RELATED_POLICY_ESF: &str = r#"
esf_version: "1.0"
collection: contracts
entity_schema:
  type: object
  required: [title]
  properties:
    title: { type: string }
link_types:
  belongs_to_engagement:
    target_collection: engagements
    cardinality: many-to-one
access_control:
  identity:
    user_id: subject.user_id
  read:
    allow:
      - name: contracts-visible-through-engagement
        where:
          related:
            link_type: belongs_to_engagement
            direction: outgoing
            target_collection: engagements
            target_policy: read
"#;

    const CONTRACTS_SIMPLE_POLICY_ESF: &str = r#"
esf_version: "1.0"
collection: contracts
entity_schema:
  type: object
  required: [title]
  properties:
    title: { type: string }
link_types:
  belongs_to_engagement:
    target_collection: engagements
    cardinality: many-to-one
access_control:
  read:
    allow:
      - name: contracts-read
        where: { field: title, is_null: false }
"#;

    const ENGAGEMENTS_INCOMING_POLICY_ESF: &str = r#"
esf_version: "1.0"
collection: engagements
entity_schema:
  type: object
  required: [name]
  properties:
    name: { type: string }
access_control:
  read:
    allow:
      - name: engagements-visible-through-contracts
        where:
          related:
            link_type: belongs_to_engagement
            direction: incoming
            target_collection: contracts
            target_policy: read
"#;

    const A_CYCLE_POLICY_ESF: &str = r#"
esf_version: "1.0"
collection: a
entity_schema:
  type: object
  properties:
    name: { type: string }
link_types:
  to_b:
    target_collection: b
    cardinality: many-to-one
access_control:
  read:
    allow:
      - name: a-through-b
        where:
          related:
            link_type: to_b
            target_collection: b
            target_policy: read
"#;

    const B_CYCLE_POLICY_ESF: &str = r#"
esf_version: "1.0"
collection: b
entity_schema:
  type: object
  properties:
    name: { type: string }
link_types:
  to_a:
    target_collection: a
    cardinality: many-to-one
access_control:
  read:
    allow:
      - name: b-through-a
        where:
          related:
            link_type: to_a
            target_collection: a
            target_policy: read
"#;

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
        assert_eq!(plan.policy_version, 1);
    }

    #[test]
    fn compile_report_exposes_nullability_denied_fields_envelopes_and_explain_ids() {
        let plan = compile_fixture(PROCUREMENT_POLICY_ESF).expect("policy should compile");
        let nullable = plan
            .report
            .nullable_fields
            .iter()
            .find(|field| field.field == "restricted_notes")
            .expect("restricted_notes should be nullable");
        assert_eq!(nullable.collection, "purchase_orders");
        assert!(nullable.required_by_schema);
        assert!(nullable.graphql_nullable);
        assert_eq!(
            nullable.rule_ids,
            vec![
                "rule:purchase_orders:fields.restricted_notes.read.deny.0:buyers-do-not-see-restricted-notes"
                    .to_string()
            ]
        );

        let denied_write = plan
            .report
            .denied_write_fields
            .iter()
            .find(|field| field.field == "restricted_notes")
            .expect("restricted_notes write denial should be reported");
        assert_eq!(
            denied_write.rule_ids,
            vec![
                "rule:purchase_orders:fields.restricted_notes.write.deny.0:buyers-cannot-write-restricted-notes"
                    .to_string()
            ]
        );

        let decisions = plan
            .report
            .envelope_summaries
            .iter()
            .map(|summary| summary.decision.clone())
            .collect::<Vec<_>>();
        assert!(decisions.contains(&PolicyDecision::Allow));
        assert!(decisions.contains(&PolicyDecision::NeedsApproval));
        assert!(decisions.contains(&PolicyDecision::Deny));

        let approval = plan
            .report
            .envelope_summaries
            .iter()
            .find(|summary| summary.decision == PolicyDecision::NeedsApproval)
            .and_then(|summary| summary.approval.as_ref())
            .expect("needs_approval envelope should carry approval metadata");
        assert_eq!(approval.role.as_deref(), Some("finance_approver"));
        assert!(approval.reason_required);
        assert_eq!(approval.deadline_seconds, Some(86400));
        assert!(approval.separation_of_duties);

        let explain = plan
            .explain
            .entries
            .iter()
            .find(|entry| {
                entry.rule_id
                    == "rule:purchase_orders:fields.restricted_notes.read.deny.0:buyers-do-not-see-restricted-notes"
            })
            .expect("field redaction rule should be explainable");
        assert_eq!(explain.kind, PolicyExplainKind::FieldReadDeny);
        assert_eq!(explain.field_path.as_deref(), Some("restricted_notes"));
        assert_eq!(
            explain.name.as_deref(),
            Some("buyers-do-not-see-restricted-notes")
        );
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

    #[test]
    fn compiles_outgoing_target_policy_and_reports_forward_link_index() {
        let schemas = [
            parse_schema(ENGAGEMENTS_POLICY_ESF),
            parse_schema(CONTRACTS_RELATED_POLICY_ESF),
        ];
        let catalog = compile_policy_catalog(&schemas).expect("catalog should compile");
        let contracts = catalog
            .plans
            .get("contracts")
            .expect("contracts plan missing");
        let read = contracts
            .operations
            .get(&PolicyOperation::Read)
            .expect("read policy missing");
        let related = match read.allow[0].where_clause.as_ref() {
            Some(CompiledPredicate::Related(related)) => related,
            other => panic!("expected related predicate, got {other:?}"),
        };
        assert_eq!(related.direction, LinkDirection::Outgoing);
        assert_eq!(related.target_collection, "engagements");
        assert_eq!(related.target_policy, Some(PolicyOperation::Read));
        assert_eq!(related.link_source_collection, "contracts");
        assert_eq!(related.link_target_collection, "engagements");

        let expected = vec![RequiredLinkIndex {
            name: "links_primary".to_string(),
            source_collection: "contracts".to_string(),
            link_type: "belongs_to_engagement".to_string(),
            target_collection: "engagements".to_string(),
            direction: LinkDirection::Outgoing,
        }];
        assert_eq!(contracts.report.required_link_indexes, expected);
        assert_eq!(catalog.report.required_link_indexes, expected);
    }

    #[test]
    fn compiles_incoming_target_policy_and_reports_reverse_link_index() {
        let schemas = [
            parse_schema(ENGAGEMENTS_INCOMING_POLICY_ESF),
            parse_schema(CONTRACTS_SIMPLE_POLICY_ESF),
        ];
        let catalog = compile_policy_catalog(&schemas).expect("catalog should compile");
        let engagements = catalog
            .plans
            .get("engagements")
            .expect("engagements plan missing");
        let read = engagements
            .operations
            .get(&PolicyOperation::Read)
            .expect("read policy missing");
        let related = match read.allow[0].where_clause.as_ref() {
            Some(CompiledPredicate::Related(related)) => related,
            other => panic!("expected related predicate, got {other:?}"),
        };
        assert_eq!(related.direction, LinkDirection::Incoming);
        assert_eq!(related.target_collection, "contracts");
        assert_eq!(related.target_policy, Some(PolicyOperation::Read));
        assert_eq!(related.link_source_collection, "contracts");
        assert_eq!(related.link_target_collection, "engagements");
        assert_eq!(
            engagements.report.required_link_indexes,
            vec![RequiredLinkIndex {
                name: "idx_links_target".to_string(),
                source_collection: "contracts".to_string(),
                link_type: "belongs_to_engagement".to_string(),
                target_collection: "engagements".to_string(),
                direction: LinkDirection::Incoming,
            }]
        );
    }

    #[test]
    fn missing_relationship_link_type_returns_stable_diagnostic() {
        let contracts = CONTRACTS_RELATED_POLICY_ESF.replace(
            "link_type: belongs_to_engagement",
            "link_type: missing_link",
        );
        let schemas = [
            parse_schema(ENGAGEMENTS_POLICY_ESF),
            parse_schema(&contracts),
        ];
        let err = compile_policy_catalog(&schemas).expect_err("missing link type should fail");
        assert_eq!(
            err.to_string(),
            "policy_expression_invalid: unknown link_type 'missing_link' at operations.Read.allow[0].where"
        );
        assert_eq!(err.code(), POLICY_COMPILE_ERROR_DEFAULT_CODE);
        assert_eq!(err.path(), Some("operations.Read.allow[0].where"));
        assert_eq!(err.collection(), Some("contracts"));
        // AxonError conversion preserves the historical wire format.
        let axon_err: AxonError = err.into();
        assert_eq!(
            axon_err.to_string(),
            "schema validation failed: policy_expression_invalid: unknown link_type 'missing_link' at operations.Read.allow[0].where"
        );
    }

    #[test]
    fn missing_relationship_target_collection_returns_stable_diagnostic() {
        let schemas = [parse_schema(CONTRACTS_RELATED_POLICY_ESF)];
        let err = compile_policy_catalog(&schemas).expect_err("missing collection should fail");
        assert_eq!(
            err.to_string(),
            "policy_expression_invalid: unknown target_collection 'engagements' at operations.Read.allow[0].where"
        );
        assert_eq!(err.path(), Some("operations.Read.allow[0].where"));
        assert_eq!(err.collection(), Some("contracts"));
    }

    #[test]
    fn relationship_target_policy_cycles_return_stable_diagnostic() {
        let schemas = [
            parse_schema(A_CYCLE_POLICY_ESF),
            parse_schema(B_CYCLE_POLICY_ESF),
        ];
        let err = compile_policy_catalog(&schemas).expect_err("cycle should fail");
        assert_eq!(
            err.to_string(),
            "policy_expression_invalid: relationship target_policy cycle detected: a.read -> b.read -> a.read"
        );
        // Cycle errors carry a collection but no JSON path (the cycle spans
        // multiple plans).
        assert!(err.collection().is_some());
        assert!(err.path().is_none());
    }

    #[test]
    fn rule_compile_error_carries_rule_id_and_field() {
        // Reuse an existing failing fixture and assert the rule-id and
        // field annotations the admin UI relies on to focus the first
        // actionable error.
        let contracts = CONTRACTS_RELATED_POLICY_ESF.replace(
            "link_type: belongs_to_engagement",
            "link_type: missing_link",
        );
        let schemas = [
            parse_schema(ENGAGEMENTS_POLICY_ESF),
            parse_schema(&contracts),
        ];
        let err = compile_policy_catalog(&schemas).expect_err("compile should fail");
        assert!(
            err.rule_id().is_some(),
            "rule-scoped errors should carry the stable rule id, got {err:?}"
        );
    }

    #[test]
    fn from_compile_error_builds_failure_report() {
        let err = compile_policy_catalog(&[parse_schema(CONTRACTS_RELATED_POLICY_ESF)])
            .expect_err("missing collection should fail");
        let report = PolicyCompileReport::from_compile_error(&err);
        assert_eq!(report.errors.len(), 1);
        let diag = &report.errors[0];
        assert_eq!(diag.code, POLICY_COMPILE_ERROR_DEFAULT_CODE);
        assert!(diag.message.contains("unknown target_collection"));
        assert_eq!(diag.path.as_deref(), Some("operations.Read.allow[0].where"));
        assert_eq!(diag.collection.as_deref(), Some("contracts"));
        assert!(report.warnings.is_empty());
        assert!(report.required_link_indexes.is_empty());
    }
}
