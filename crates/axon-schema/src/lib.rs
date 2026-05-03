#![forbid(unsafe_code)]
//! Schema definitions, validation, and migration for Axon collections.
//!
//! `axon-schema` provides the schema engine that enforces structure on entities
//! stored in Axon collections. Every collection has an associated schema that
//! validates entity fields, types, and constraints.

pub mod access_control;
pub mod evolution;
pub mod gates;
pub mod named_queries;
pub mod policy;
pub mod rules;
pub mod schema;
pub mod validation;

#[cfg(test)]
mod proptest_schema;

pub use access_control::{
    AccessControlIdentity, AccessControlPolicy, ApprovalRoute, FieldAccessPolicy, FieldPolicy,
    FieldPolicyRule, IdentityAttributeSource, LinkDirection, OperationPolicy, PolicyCompareOp,
    PolicyDecision, PolicyEnvelope, PolicyOperation, PolicyPredicate, PolicyRule,
    RelationshipPredicate, SharesRelationPredicate,
};
pub use evolution::{
    classify, diff_schemas, Compatibility, FieldChange, FieldChangeKind, SchemaDiff,
};
pub use gates::{evaluate_gates, GateEvaluation, GateResult};
pub use named_queries::{
    compile_named_queries, schema_snapshot_from_schemas, CompileReport, NamedQueryDiagnostic,
    NamedQueryStatus,
};
pub use policy::{
    compile_policy_catalog, compile_policy_plan, CompiledCompareOp, CompiledComparison,
    CompiledFieldAccessPolicy, CompiledFieldPolicy, CompiledFieldPolicyRule,
    CompiledOperationPolicy, CompiledPolicyEnvelope, CompiledPolicyRule, CompiledPredicate,
    CompiledRelationshipPredicate, CompiledSharesRelationPredicate, PolicyCatalog,
    PolicyCompileDiagnostic, PolicyCompileError, PolicyCompileReport, PolicyDeniedWriteField,
    PolicyEnvelopeSummary, PolicyExplainEntry, PolicyExplainKind, PolicyExplainPlan,
    PolicyNullableField, PolicyPlan, PredicateTarget, RequiredLinkIndex,
    POLICY_COMPILE_ERROR_DEFAULT_CODE,
};
pub use rules::validate_rule_definitions;
pub use rules::ValidationRule;
pub use schema::{
    Cardinality, CollectionSchema, CollectionView, CompoundIndexDef, CompoundIndexField,
    EsfDocument, GateDef, IndexDef, IndexType, LifecycleDef, LinkTypeDef, NamedQueryDef,
    NamedQueryParameter,
};
pub use validation::{
    compile_entity_schema, validate, validate_entity, validate_link_metadata,
    SchemaValidationError, SchemaValidationErrors,
};
