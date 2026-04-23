//! MCP tool handlers wired to the Axon API layer.
//!
//! Each handler function creates a [`ToolDef`] that dispatches to the
//! appropriate `AxonHandler` method via a shared `Arc<Mutex<AxonHandler>>`.
//!
//! Uses `std::sync::Mutex` since all `AxonHandler` methods are synchronous.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_graphql::parser::{
    parse_query,
    types::{Field as GraphQlField, OperationType, Selection, SelectionSet},
};
use async_graphql::{
    Request as GraphQlRequest, Value as GraphQlConstValue, Variables as GraphQlVariables,
};
use axon_api::handler::AxonHandler;
use axon_api::intent::{
    canonical_create_entity_operation, canonical_delete_entity_operation,
    canonical_patch_entity_operation, canonical_transition_lifecycle_operation,
    canonicalize_intent_operation, ApprovalState, CanonicalOperationMetadata,
    MutationApprovalRoute, MutationIntent, MutationIntentCommitValidationContext,
    MutationIntentDecision, MutationIntentLifecycleService, MutationIntentScopeBinding,
    MutationIntentSubjectBinding, MutationIntentToken, MutationIntentTokenLookupError,
    MutationIntentTokenSigner, MutationIntentTransactionCommitRequest, MutationOperationKind,
    MutationReviewSummary, PreImageBinding,
};
use axon_api::request::{
    AggregateFunction, AggregateRequest, CountEntitiesRequest, CreateEntityRequest,
    DeleteEntityRequest, ExplainPolicyRequest, FilterNode, FindLinkCandidatesRequest,
    GetEntityRequest, ListCollectionsRequest, ListNamespaceCollectionsRequest,
    ListNamespacesRequest, ListNeighborsRequest, PatchEntityRequest, QueryEntitiesRequest,
    TransitionLifecycleRequest, TraverseDirection, TraverseRequest,
};
use axon_api::response::{EffectivePolicyResponse, PolicyExplanationResponse};
use axon_api::transaction::Transaction;
use axon_audit::entry::compute_diff;
use axon_core::auth::{CallerIdentity, Operation};
use axon_core::error::{AxonError, PolicyDenial};
use axon_core::id::{CollectionId, EntityId, Namespace, DEFAULT_SCHEMA};
use axon_core::types::{Entity, Link};
use axon_schema::validation::validate;
use axon_schema::{
    compile_policy_catalog, CollectionSchema, PolicyCompileReport, PolicyEnvelopeSummary,
};
use axon_storage::adapter::StorageAdapter;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
use tokio::sync::Mutex as TokioMutex;

use crate::protocol::{McpMutationIntentOutcome, McpMutationIntentToolResult};
use crate::tools::{
    ToolDef, ToolError, ToolPolicyCapabilities, ToolPolicyEnvelopeSummary, ToolPolicyMetadata,
};

static MCP_INTENT_COUNTER: AtomicU64 = AtomicU64::new(1);
const MCP_INTENT_TOKEN_SECRET: &[u8] = b"axon-graphql-mutation-intents-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentToolMode {
    Direct,
    Preview,
    Commit,
}

struct MutationPreviewComputation {
    explain_request: ExplainPolicyRequest,
    pre_images: Vec<PreImageBinding>,
    diff: Value,
    affected_fields: Vec<String>,
    schema_version: u32,
}

/// Build CRUD tools for a collection, wired to a shared handler.
///
/// Returns tool definitions for `{collection}.create`, `.get`, `.patch`, `.delete`.
pub fn build_crud_tools<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
    caller: CallerIdentity,
) -> Vec<ToolDef> {
    let col = collection.to_string();
    vec![
        build_create_tool(&col, Arc::clone(&handler), caller.clone()),
        build_get_tool(&col, Arc::clone(&handler)),
        build_patch_tool(&col, Arc::clone(&handler), caller.clone()),
        build_delete_tool(&col, handler, caller),
    ]
}

/// Attach FEAT-029 policy metadata to collection-specific tools.
///
/// The metadata is advisory and caller-specific. Handlers still enforce policy
/// at execution time, but agents can inspect `tools/list` before choosing a
/// tool or deciding whether a mutation requires a preview/approval route.
pub fn apply_policy_metadata_to_registry<S: StorageAdapter>(
    registry: &mut crate::tools::ToolRegistry,
    handler: &AxonHandler<S>,
    current_database: &str,
    collections: &[String],
    caller: &CallerIdentity,
) -> Result<(), AxonError> {
    let report = policy_compile_report_for_database(handler, current_database)?;
    for collection in collections {
        let collection_id = mcp_collection_id(collection, current_database);
        if handler.get_schema(&collection_id)?.is_none() {
            continue;
        }
        let effective =
            handler.effective_policy_with_caller(collection_id.clone(), None, caller, None)?;
        let envelopes = policy_envelopes_for_collection(
            &report,
            collection,
            collection_id.as_str(),
            effective.collection.as_str(),
        );
        registry.set_collection_policy(
            collection.clone(),
            tool_policy_metadata(effective, envelopes),
        );
    }
    Ok(())
}

fn policy_compile_report_for_database<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    current_database: &str,
) -> Result<PolicyCompileReport, AxonError> {
    let schemas = policy_catalog_schemas_for_database(handler, current_database)?;
    Ok(compile_policy_catalog(&schemas)?.report)
}

fn policy_catalog_schemas_for_database<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    current_database: &str,
) -> Result<Vec<CollectionSchema>, AxonError> {
    let namespaces = handler.list_namespaces(ListNamespacesRequest {
        database: current_database.to_string(),
    })?;
    let mut schemas = Vec::new();

    for schema in namespaces.schemas {
        let namespace_collections =
            handler.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: current_database.to_string(),
                schema: schema.clone(),
            })?;
        for collection in namespace_collections.collections {
            let visible_collection = if schema == DEFAULT_SCHEMA {
                collection
            } else {
                format!("{schema}.{collection}")
            };
            let collection_id = mcp_collection_id(&visible_collection, current_database);
            if let Some(schema) = handler.get_schema(&collection_id)? {
                schemas.push(schema);
            }
        }
    }

    Ok(schemas)
}

fn mcp_collection_id(collection: &str, current_database: &str) -> CollectionId {
    CollectionId::new(Namespace::qualify_with_database(
        collection,
        current_database,
    ))
}

fn policy_envelopes_for_collection(
    report: &PolicyCompileReport,
    visible_collection: &str,
    storage_collection: &str,
    effective_collection: &str,
) -> Vec<ToolPolicyEnvelopeSummary> {
    report
        .envelope_summaries
        .iter()
        .filter(|summary| {
            policy_summary_matches_collection(
                summary,
                visible_collection,
                storage_collection,
                effective_collection,
            )
        })
        .cloned()
        .map(ToolPolicyEnvelopeSummary::from)
        .collect()
}

fn policy_summary_matches_collection(
    summary: &PolicyEnvelopeSummary,
    visible_collection: &str,
    storage_collection: &str,
    effective_collection: &str,
) -> bool {
    summary.collection == visible_collection
        || summary.collection == storage_collection
        || summary.collection == effective_collection
}

fn tool_policy_metadata(
    effective: EffectivePolicyResponse,
    envelopes: Vec<ToolPolicyEnvelopeSummary>,
) -> ToolPolicyMetadata {
    ToolPolicyMetadata {
        collection: effective.collection,
        policy_version: effective.policy_version,
        capabilities: ToolPolicyCapabilities {
            can_read: effective.can_read,
            can_create: effective.can_create,
            can_update: effective.can_update,
            can_delete: effective.can_delete,
        },
        tool_operation: None,
        redacted_fields: effective.redacted_fields,
        denied_fields: effective.denied_fields,
        envelopes,
        applicable_envelopes: Vec::new(),
        envelope_summary: None,
    }
}

fn lock_handler<S: StorageAdapter>(
    handler: &Mutex<AxonHandler<S>>,
) -> Result<std::sync::MutexGuard<'_, AxonHandler<S>>, ToolError> {
    handler
        .lock()
        .map_err(|e| ToolError::Internal(format!("mutex poisoned: {e}")))
}

fn text_tool_response<T: Serialize>(payload: &T) -> Result<Value, ToolError> {
    let text = serde_json::to_string(payload).map_err(|e| ToolError::Internal(e.to_string()))?;
    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": text
        }]
    }))
}

fn policy_denial_tool_response(
    operation: &str,
    denial: &PolicyDenial,
    explanation: Option<PolicyExplanationResponse>,
) -> Result<Value, ToolError> {
    let explanation_json = explanation
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| ToolError::Internal(e.to_string()))?;
    let decision = explanation
        .as_ref()
        .map(|explanation| explanation.decision.as_str())
        .unwrap_or("deny");
    let outcome = if decision == "needs_approval" || denial.reason == "needs_approval" {
        "needs_approval"
    } else {
        "denied"
    };
    let error_code = if outcome == "needs_approval" {
        "approval_required"
    } else {
        "denied_policy"
    };
    let mut structured = serde_json::json!({
        "outcome": outcome,
        "errorCode": error_code,
        "operation": operation,
        "reason": denial.reason,
        "collection": denial.collection,
        "message": denial.to_string()
    });
    if let Some(entity_id) = &denial.entity_id {
        structured["entityId"] = serde_json::json!(entity_id);
    }
    if let Some(field_path) = &denial.field_path {
        structured["fieldPath"] = serde_json::json!(field_path);
    }
    if let Some(policy) = &denial.policy {
        structured["ruleId"] = serde_json::json!(policy);
        structured["policy"] = serde_json::json!(policy);
    }
    if let Some(operation_index) = denial.operation_index {
        structured["operationIndex"] = serde_json::json!(operation_index);
    }
    if let Some(explanation) = explanation_json {
        if let Some(rule_ids) = explanation.get("rule_ids") {
            structured["ruleIds"] = rule_ids.clone();
        }
        if let Some(rules) = explanation.get("rules") {
            structured["rules"] = rules.clone();
        }
        if let Some(field_paths) = explanation.get("field_paths") {
            structured["fieldPaths"] = field_paths.clone();
        }
        if let Some(denied_fields) = explanation.get("denied_fields") {
            structured["deniedFields"] = denied_fields.clone();
        }
        if let Some(policy_version) = explanation.get("policy_version") {
            structured["policyVersion"] = policy_version.clone();
        }
        if let Some(approval) = explanation.get("approval") {
            if !approval.is_null() {
                structured["approval"] = approval.clone();
            }
        }
        structured["explanation"] = explanation;
    }

    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": denial.to_string()
        }],
        "structuredContent": structured,
        "isError": true
    }))
}

fn policy_denial_result(
    operation: &str,
    error: AxonError,
    explanation: Option<PolicyExplanationResponse>,
) -> Result<Value, ToolError> {
    match error {
        AxonError::PolicyDenied(denial) => {
            policy_denial_tool_response(operation, &denial, explanation)
        }
        other => Err(to_tool_error(other)),
    }
}

fn explain_write_policy<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    caller: &CallerIdentity,
    request: ExplainPolicyRequest,
) -> Option<PolicyExplanationResponse> {
    handler
        .explain_policy_with_caller(request, caller, None)
        .ok()
}

fn intent_mode(args: &Value) -> Result<IntentToolMode, ToolError> {
    if args
        .get("preview")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(IntentToolMode::Preview);
    }

    let raw = args
        .get("intent_mode")
        .or_else(|| args.get("intentMode"))
        .or_else(|| args.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("direct");
    match raw {
        "direct" | "execute" | "mutation" => Ok(IntentToolMode::Direct),
        "preview" => Ok(IntentToolMode::Preview),
        "commit" => Ok(IntentToolMode::Commit),
        other => Err(ToolError::InvalidArgument(format!(
            "unsupported intent mode: {other}"
        ))),
    }
}

fn intent_token_arg(args: &Value) -> Result<MutationIntentToken, ToolError> {
    args.get("intent_token")
        .or_else(|| args.get("intentToken"))
        .and_then(Value::as_str)
        .map(MutationIntentToken::new)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'intent_token'".into()))
}

fn expires_in_seconds(args: &Value) -> u64 {
    args.get("expires_in_seconds")
        .or_else(|| args.get("expiresInSeconds"))
        .and_then(Value::as_u64)
        .unwrap_or(3600)
}

fn intent_tool_result(outcome: McpMutationIntentOutcome) -> Result<Value, ToolError> {
    serde_json::to_value(McpMutationIntentToolResult::from(outcome))
        .map_err(|error| ToolError::Internal(error.to_string()))
}

fn default_intent_scope() -> MutationIntentScopeBinding {
    MutationIntentScopeBinding {
        tenant_id: "default".into(),
        database_id: "default".into(),
    }
}

fn mcp_intent_token_signer() -> MutationIntentTokenSigner {
    MutationIntentTokenSigner::new(MCP_INTENT_TOKEN_SECRET.to_vec())
}

fn mcp_intent_lifecycle_service() -> MutationIntentLifecycleService {
    MutationIntentLifecycleService::new(mcp_intent_token_signer())
}

fn current_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn next_mcp_intent_id(now_ns: u64) -> String {
    let sequence = MCP_INTENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("mint_mcp_{now_ns}_{sequence}")
}

fn operation_kind_label(kind: &MutationOperationKind) -> &'static str {
    match kind {
        MutationOperationKind::CreateEntity => "create_entity",
        MutationOperationKind::UpdateEntity => "update_entity",
        MutationOperationKind::PatchEntity => "patch_entity",
        MutationOperationKind::DeleteEntity => "delete_entity",
        MutationOperationKind::CreateLink => "create_link",
        MutationOperationKind::DeleteLink => "delete_link",
        MutationOperationKind::Transaction => "transaction",
        MutationOperationKind::Transition => "transition",
        MutationOperationKind::Rollback => "rollback",
        MutationOperationKind::Revert => "revert",
    }
}

fn mutation_intent_decision(decision: &str) -> MutationIntentDecision {
    match decision {
        "needs_approval" => MutationIntentDecision::NeedsApproval,
        "deny" | "denied" => MutationIntentDecision::Deny,
        _ => MutationIntentDecision::Allow,
    }
}

fn preview_state_for_decision(decision: &MutationIntentDecision) -> ApprovalState {
    match decision {
        MutationIntentDecision::NeedsApproval => ApprovalState::Pending,
        MutationIntentDecision::Allow | MutationIntentDecision::Deny => ApprovalState::None,
    }
}

fn mutation_approval_route_from_policy(
    approval: &axon_api::response::PolicyApprovalEnvelopeSummary,
) -> MutationApprovalRoute {
    MutationApprovalRoute {
        role: approval.role.clone(),
        reason_required: approval.reason_required,
        deadline_seconds: approval.deadline_seconds,
        separation_of_duties: approval.separation_of_duties,
    }
}

fn mutation_preview_policy_lines(policy: &PolicyExplanationResponse) -> Vec<String> {
    let mut lines = vec![format!("{}: {}", policy.decision, policy.reason)];
    lines.extend(policy.policy_ids.iter().map(|id| format!("policy:{id}")));
    lines.extend(policy.rule_ids.iter().map(|id| format!("rule:{id}")));
    for child in &policy.operations {
        lines.push(format!(
            "operation[{}]: {}: {}",
            child.operation_index.unwrap_or(0),
            child.decision,
            child.reason
        ));
    }
    lines
}

fn subject_binding(caller: &CallerIdentity) -> MutationIntentSubjectBinding {
    MutationIntentSubjectBinding {
        user_id: Some(caller.actor.clone()),
        agent_id: None,
        delegated_by: None,
        tenant_role: Some(caller.role.to_string()),
        credential_id: None,
        grant_version: None,
        attributes: Default::default(),
    }
}

fn required_schema<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &CollectionId,
) -> Result<CollectionSchema, ToolError> {
    handler
        .get_schema(collection)
        .map_err(to_tool_error)?
        .ok_or_else(|| ToolError::NotFound(collection.to_string()))
}

fn required_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &CollectionId,
    id: &EntityId,
) -> Result<Entity, ToolError> {
    handler
        .storage_ref()
        .get(collection, id)
        .map_err(to_tool_error)?
        .ok_or_else(|| ToolError::NotFound(id.to_string()))
}

fn check_expected_version(entity: &Entity, expected_version: Option<u64>) -> Result<(), ToolError> {
    if let Some(expected) = expected_version {
        if entity.version != expected {
            return Err(to_tool_error(AxonError::ConflictingVersion {
                expected,
                actual: entity.version,
                current_entity: Some(Box::new(entity.clone())),
            }));
        }
    }
    Ok(())
}

fn entity_pre_image(entity: &Entity) -> PreImageBinding {
    PreImageBinding::Entity {
        collection: entity.collection.clone(),
        id: entity.id.clone(),
        version: entity.version,
    }
}

fn diff_value(before: &Value, after: &Value) -> Value {
    serde_json::to_value(compute_diff(before, after)).unwrap_or_else(|_| serde_json::json!({}))
}

fn affected_fields_from_diff(diff: &Value) -> Vec<String> {
    let mut fields = match diff {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(items) => items
            .iter()
            .flat_map(|item| {
                item.get("diff")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(|map| map.keys().cloned())
            })
            .collect(),
        _ => Vec::new(),
    };
    fields.sort();
    fields.dedup();
    fields
}

fn preview_result(
    explain_request: ExplainPolicyRequest,
    pre_images: Vec<PreImageBinding>,
    diff: Value,
    schema_version: u32,
) -> MutationPreviewComputation {
    let affected_fields = affected_fields_from_diff(&diff);
    MutationPreviewComputation {
        explain_request,
        pre_images,
        diff,
        affected_fields,
        schema_version,
    }
}

fn preview_create_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    request: &CreateEntityRequest,
) -> Result<MutationPreviewComputation, ToolError> {
    let schema = required_schema(handler, &request.collection)?;
    validate(&schema, &request.data).map_err(to_tool_error)?;
    if handler
        .storage_ref()
        .get(&request.collection, &request.id)
        .map_err(to_tool_error)?
        .is_some()
    {
        return Err(to_tool_error(AxonError::AlreadyExists(format!(
            "{}/{}",
            request.collection, request.id
        ))));
    }
    let mut explain_request = empty_explain_policy_request("create");
    explain_request.collection = Some(request.collection.clone());
    explain_request.entity_id = Some(request.id.clone());
    explain_request.data = Some(request.data.clone());
    Ok(preview_result(
        explain_request,
        Vec::new(),
        diff_value(&serde_json::json!({}), &request.data),
        schema.version,
    ))
}

fn preview_patch_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    request: &PatchEntityRequest,
) -> Result<MutationPreviewComputation, ToolError> {
    let schema = required_schema(handler, &request.collection)?;
    let current = required_entity(handler, &request.collection, &request.id)?;
    check_expected_version(&current, Some(request.expected_version))?;
    let mut merged = current.data.clone();
    json_merge_patch(&mut merged, &request.patch);
    validate(&schema, &merged).map_err(to_tool_error)?;
    let mut explain_request = empty_explain_policy_request("patch");
    explain_request.collection = Some(request.collection.clone());
    explain_request.entity_id = Some(request.id.clone());
    explain_request.expected_version = Some(request.expected_version);
    explain_request.patch = Some(request.patch.clone());
    Ok(preview_result(
        explain_request,
        vec![entity_pre_image(&current)],
        diff_value(&current.data, &merged),
        schema.version,
    ))
}

fn preview_delete_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    request: &DeleteEntityRequest,
    expected_version: Option<u64>,
) -> Result<MutationPreviewComputation, ToolError> {
    let schema = required_schema(handler, &request.collection)?;
    let current = required_entity(handler, &request.collection, &request.id)?;
    check_expected_version(&current, expected_version)?;
    let mut explain_request = empty_explain_policy_request("delete");
    explain_request.collection = Some(request.collection.clone());
    explain_request.entity_id = Some(request.id.clone());
    explain_request.expected_version = expected_version;
    Ok(preview_result(
        explain_request,
        vec![entity_pre_image(&current)],
        diff_value(&current.data, &serde_json::json!({})),
        schema.version,
    ))
}

fn preview_transition_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    request: &TransitionLifecycleRequest,
) -> Result<MutationPreviewComputation, ToolError> {
    let schema = required_schema(handler, &request.collection_id)?;
    let lifecycle = schema
        .lifecycles
        .get(&request.lifecycle_name)
        .ok_or_else(|| {
            to_tool_error(AxonError::LifecycleNotFound {
                lifecycle_name: request.lifecycle_name.clone(),
            })
        })?;
    let current = required_entity(handler, &request.collection_id, &request.entity_id)?;
    check_expected_version(&current, Some(request.expected_version))?;
    let mut candidate = current.data.clone();
    candidate[&lifecycle.field] = Value::String(request.target_state.clone());
    validate(&schema, &candidate).map_err(to_tool_error)?;
    let mut explain_request = empty_explain_policy_request("transition");
    explain_request.collection = Some(request.collection_id.clone());
    explain_request.entity_id = Some(request.entity_id.clone());
    explain_request.expected_version = Some(request.expected_version);
    explain_request.lifecycle_name = Some(request.lifecycle_name.clone());
    explain_request.target_state = Some(request.target_state.clone());
    Ok(preview_result(
        explain_request,
        vec![entity_pre_image(&current)],
        diff_value(&current.data, &candidate),
        schema.version,
    ))
}

fn execute_intent_preview<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    caller: &CallerIdentity,
    scope: MutationIntentScopeBinding,
    operation: CanonicalOperationMetadata,
    preview: MutationPreviewComputation,
    expires_in_seconds: u64,
) -> Result<Value, ToolError> {
    let policy = handler
        .explain_policy_with_caller(preview.explain_request, caller, None)
        .map_err(to_tool_error)?;
    let decision = mutation_intent_decision(&policy.decision);
    let approval_route = policy
        .approval
        .as_ref()
        .map(mutation_approval_route_from_policy);
    let policy_version = policy.policy_version.max(preview.schema_version);
    let policy_explanation = mutation_preview_policy_lines(&policy);
    let now_ns = current_time_ns();
    let intent = MutationIntent {
        intent_id: next_mcp_intent_id(now_ns),
        scope,
        subject: subject_binding(caller),
        schema_version: preview.schema_version.max(policy_version),
        policy_version,
        operation: operation.clone(),
        pre_images: preview.pre_images.clone(),
        decision: decision.clone(),
        approval_state: preview_state_for_decision(&decision),
        approval_route,
        expires_at: now_ns.saturating_add(expires_in_seconds.saturating_mul(1_000_000_000)),
        review_summary: MutationReviewSummary {
            title: Some(format!(
                "{} preview",
                operation_kind_label(&operation.operation_kind)
            )),
            summary: policy.reason.clone(),
            risk: (decision == MutationIntentDecision::NeedsApproval)
                .then(|| "needs_approval".into()),
            affected_records: preview.pre_images,
            affected_fields: preview.affected_fields,
            diff: preview.diff,
            policy_explanation,
        },
    };

    let service = mcp_intent_lifecycle_service();
    let record = service
        .create_preview_record(handler.storage_mut(), intent)
        .map_err(|error| ToolError::Internal(error.to_string()))?;
    match (&record.intent.decision, record.intent_token.as_ref()) {
        (MutationIntentDecision::Allow, Some(token)) => {
            intent_tool_result(McpMutationIntentOutcome::allowed(&record.intent, token))
        }
        (MutationIntentDecision::NeedsApproval, Some(token)) => intent_tool_result(
            McpMutationIntentOutcome::needs_approval(&record.intent, token),
        ),
        (MutationIntentDecision::Deny, _) => {
            intent_tool_result(McpMutationIntentOutcome::denied_policy(
                record.intent.review_summary.summary.clone(),
                Some(record.intent.intent_id.clone()),
                record.intent.review_summary.policy_explanation.clone(),
            ))
        }
        (_, None) => intent_tool_result(McpMutationIntentOutcome::from_token_lookup_error(
            MutationIntentTokenLookupError::Unauthorized,
        )),
    }
}

fn execute_intent_commit<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    caller: &CallerIdentity,
    scope: MutationIntentScopeBinding,
    token: MutationIntentToken,
) -> Result<Value, ToolError> {
    let service = mcp_intent_lifecycle_service();
    let token_intent_id = match mcp_intent_token_signer().verify(&token) {
        Ok(intent_id) => intent_id,
        Err(error) => {
            return intent_tool_result(McpMutationIntentOutcome::from_token_lookup_error(error))
        }
    };
    let stored_intent = match handler.storage_ref().get_mutation_intent(
        &scope.tenant_id,
        &scope.database_id,
        &token_intent_id,
    ) {
        Ok(Some(intent)) => intent,
        Ok(None) => {
            return intent_tool_result(McpMutationIntentOutcome::from_token_lookup_error(
                MutationIntentTokenLookupError::NotFound,
            ))
        }
        Err(error) => return Err(to_tool_error(error)),
    };
    let operation = stored_intent.operation.clone();
    let operation_payload = canonical_operation_payload(&operation)?;
    let schema_version =
        schema_version_for_intent_operation(handler, &operation.operation_kind, operation_payload)?;
    let current = MutationIntentCommitValidationContext {
        subject: stored_intent.subject,
        schema_version,
        policy_version: schema_version,
        operation_hash: operation.operation_hash.clone(),
        caller_authorized: caller.check(Operation::Write).is_ok(),
    };
    let transaction = transaction_from_intent_operation(handler, &operation)?;
    let now_ns = current_time_ns();
    let (storage, audit) = handler.storage_and_audit_mut();
    match service.commit_transaction_intent(
        storage,
        audit,
        MutationIntentTransactionCommitRequest {
            scope,
            token,
            transaction,
            canonical_operation: Some(operation),
            current,
            now_ns,
            actor: Some(caller.actor.clone()),
            attribution: None,
        },
    ) {
        Ok(result) => intent_tool_result(McpMutationIntentOutcome::committed(
            &result.intent,
            result.transaction_id,
            result
                .written
                .into_iter()
                .map(|entity| serde_json::to_value(entity).unwrap_or(Value::Null))
                .collect(),
        )),
        Err(error) => intent_tool_result(McpMutationIntentOutcome::from_commit_validation_error(
            error,
        )),
    }
}

fn empty_explain_policy_request(operation: &str) -> ExplainPolicyRequest {
    ExplainPolicyRequest {
        operation: operation.into(),
        collection: None,
        entity_id: None,
        expected_version: None,
        data: None,
        patch: None,
        lifecycle_name: None,
        target_state: None,
        to_version: None,
        operations: Vec::new(),
    }
}

fn canonical_operation_payload(
    operation: &CanonicalOperationMetadata,
) -> Result<&Value, ToolError> {
    operation
        .canonical_operation
        .as_ref()
        .ok_or_else(|| ToolError::InvalidArgument("canonical operation payload is required".into()))
}

fn required_object<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a serde_json::Map<String, Value>, ToolError> {
    value
        .as_object()
        .ok_or_else(|| ToolError::InvalidArgument(format!("{name} must be an object")))
}

fn required_str<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, ToolError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArgument(format!("{key} is required")))
}

fn schema_version_for_intent_operation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    kind: &MutationOperationKind,
    operation: &Value,
) -> Result<u32, ToolError> {
    let obj = required_object(operation, "operation")?;
    match kind {
        MutationOperationKind::CreateEntity
        | MutationOperationKind::UpdateEntity
        | MutationOperationKind::PatchEntity
        | MutationOperationKind::DeleteEntity
        | MutationOperationKind::Transition => {
            let collection = CollectionId::new(required_str(obj, "collection")?);
            Ok(required_schema(handler, &collection)?.version)
        }
        MutationOperationKind::CreateLink | MutationOperationKind::DeleteLink => {
            let link = link_from_operation(obj)?;
            let source_version = required_schema(handler, &link.source_collection)?.version;
            let target_version = required_schema(handler, &link.target_collection)?.version;
            Ok(source_version.max(target_version))
        }
        MutationOperationKind::Transaction => {
            let operations = obj
                .get("operations")
                .and_then(Value::as_array)
                .ok_or_else(|| ToolError::InvalidArgument("operations must be a list".into()))?;
            let mut schema_version = 0;
            for child in operations {
                let child_obj = required_object(child, "operation")?;
                let op = required_str(child_obj, "op")?;
                let kind = parse_mutation_operation_kind(op)?;
                schema_version =
                    schema_version.max(schema_version_for_intent_operation(handler, &kind, child)?);
            }
            Ok(schema_version)
        }
        MutationOperationKind::Rollback | MutationOperationKind::Revert => {
            Err(ToolError::InvalidArgument(format!(
                "MCP commit does not support {} operations",
                operation_kind_label(kind)
            )))
        }
    }
}

fn parse_mutation_operation_kind(value: &str) -> Result<MutationOperationKind, ToolError> {
    match value {
        "create_entity" => Ok(MutationOperationKind::CreateEntity),
        "update_entity" => Ok(MutationOperationKind::UpdateEntity),
        "patch_entity" => Ok(MutationOperationKind::PatchEntity),
        "delete_entity" => Ok(MutationOperationKind::DeleteEntity),
        "create_link" => Ok(MutationOperationKind::CreateLink),
        "delete_link" => Ok(MutationOperationKind::DeleteLink),
        "transaction" => Ok(MutationOperationKind::Transaction),
        "transition" => Ok(MutationOperationKind::Transition),
        "rollback" => Ok(MutationOperationKind::Rollback),
        "revert" => Ok(MutationOperationKind::Revert),
        other => Err(ToolError::InvalidArgument(format!(
            "unsupported mutation operation kind: {other}"
        ))),
    }
}

fn transaction_from_intent_operation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    operation: &CanonicalOperationMetadata,
) -> Result<Transaction, ToolError> {
    let mut transaction = Transaction::new();
    let payload = canonical_operation_payload(operation)?;
    stage_intent_operation(
        handler,
        &mut transaction,
        &operation.operation_kind,
        payload,
    )?;
    Ok(transaction)
}

fn stage_intent_operation<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    kind: &MutationOperationKind,
    operation: &Value,
) -> Result<(), ToolError> {
    let obj = required_object(operation, "operation")?;
    match kind {
        MutationOperationKind::CreateEntity => stage_create_entity(transaction, obj),
        MutationOperationKind::UpdateEntity => stage_update_entity(handler, transaction, obj),
        MutationOperationKind::PatchEntity => stage_patch_entity(handler, transaction, obj),
        MutationOperationKind::DeleteEntity => stage_delete_entity(handler, transaction, obj),
        MutationOperationKind::CreateLink => stage_create_link(transaction, obj),
        MutationOperationKind::DeleteLink => stage_delete_link(transaction, obj),
        MutationOperationKind::Transition => stage_transition_entity(handler, transaction, obj),
        MutationOperationKind::Transaction => stage_transaction(handler, transaction, obj),
        MutationOperationKind::Rollback | MutationOperationKind::Revert => {
            Err(ToolError::InvalidArgument(format!(
                "MCP commit does not support {} operations",
                operation_kind_label(kind)
            )))
        }
    }
}

fn stage_create_entity(
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    let collection = CollectionId::new(required_str(operation, "collection")?);
    let id = EntityId::new(required_str(operation, "id")?);
    let data = operation.get("data").cloned().unwrap_or(Value::Null);
    transaction
        .create(Entity::new(collection, id, data))
        .map_err(to_tool_error)
}

fn stage_update_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    let collection = CollectionId::new(required_str(operation, "collection")?);
    let id = EntityId::new(required_str(operation, "id")?);
    let data = operation.get("data").cloned().unwrap_or(Value::Null);
    let schema = required_schema(handler, &collection)?;
    validate(&schema, &data).map_err(to_tool_error)?;
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    transaction
        .update(
            Entity::new(collection, id, data),
            expected_version,
            Some(current.data),
        )
        .map_err(to_tool_error)
}

fn stage_patch_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    let collection = CollectionId::new(required_str(operation, "collection")?);
    let id = EntityId::new(required_str(operation, "id")?);
    let patch = operation.get("patch").cloned().unwrap_or(Value::Null);
    let schema = required_schema(handler, &collection)?;
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    let mut merged = current.data.clone();
    json_merge_patch(&mut merged, &patch);
    validate(&schema, &merged).map_err(to_tool_error)?;
    transaction
        .update(
            Entity::new(collection, id, merged),
            expected_version,
            Some(current.data),
        )
        .map_err(to_tool_error)
}

fn stage_delete_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    let collection = CollectionId::new(required_str(operation, "collection")?);
    let id = EntityId::new(required_str(operation, "id")?);
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    transaction
        .delete(collection, id, expected_version, Some(current.data))
        .map_err(to_tool_error)
}

fn stage_transition_entity<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    let collection = CollectionId::new(required_str(operation, "collection")?);
    let id = EntityId::new(required_str(operation, "id")?);
    let lifecycle_name = required_str(operation, "lifecycle_name")?.to_string();
    let target_state = required_str(operation, "target_state")?.to_string();
    let schema = required_schema(handler, &collection)?;
    let lifecycle = schema.lifecycles.get(&lifecycle_name).ok_or_else(|| {
        to_tool_error(AxonError::LifecycleNotFound {
            lifecycle_name: lifecycle_name.clone(),
        })
    })?;
    let current = required_entity(handler, &collection, &id)?;
    let expected_version = operation
        .get("expected_version")
        .and_then(Value::as_u64)
        .unwrap_or(current.version);
    let mut candidate = current.data.clone();
    candidate[&lifecycle.field] = Value::String(target_state);
    validate(&schema, &candidate).map_err(to_tool_error)?;
    transaction
        .update(
            Entity::new(collection, id, candidate),
            expected_version,
            Some(current.data),
        )
        .map_err(to_tool_error)
}

fn stage_create_link(
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    transaction
        .create_link(link_from_operation(operation)?)
        .map_err(to_tool_error)
}

fn stage_delete_link(
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    transaction
        .delete_link(link_from_operation(operation)?)
        .map_err(to_tool_error)
}

fn stage_transaction<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    transaction: &mut Transaction,
    operation: &serde_json::Map<String, Value>,
) -> Result<(), ToolError> {
    let operations = operation
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::InvalidArgument("operations must be a list".into()))?;
    for child in operations {
        let obj = required_object(child, "operation")?;
        let kind = parse_mutation_operation_kind(required_str(obj, "op")?)?;
        stage_intent_operation(handler, transaction, &kind, child)?;
    }
    Ok(())
}

fn link_from_operation(operation: &serde_json::Map<String, Value>) -> Result<Link, ToolError> {
    Ok(Link {
        source_collection: CollectionId::new(required_str(operation, "source_collection")?),
        source_id: EntityId::new(required_str(operation, "source_id")?),
        target_collection: CollectionId::new(required_str(operation, "target_collection")?),
        target_id: EntityId::new(required_str(operation, "target_id")?),
        link_type: required_str(operation, "link_type")?.to_string(),
        metadata: operation.get("metadata").cloned().unwrap_or(Value::Null),
    })
}

fn json_merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(patch_obj) => {
            if !target.is_object() {
                *target = serde_json::json!({});
            }
            if let Some(target_obj) = target.as_object_mut() {
                for (key, value) in patch_obj {
                    if value.is_null() {
                        target_obj.remove(key);
                    } else {
                        let entry = target_obj.entry(key.clone()).or_insert(Value::Null);
                        json_merge_patch(entry, value);
                    }
                }
            }
        }
        value => *target = value.clone(),
    }
}

fn parse_optional_filter(value: Option<Value>) -> Result<Option<FilterNode>, ToolError> {
    value
        .map(|raw| {
            serde_json::from_value(raw)
                .map_err(|e| ToolError::InvalidArgument(format!("invalid 'filter': {e}")))
        })
        .transpose()
}

fn get_required_string(args: &Value, key: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| ToolError::InvalidArgument(format!("missing '{key}'")))
}

fn get_optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn get_optional_usize(args: &Value, key: &str) -> Result<Option<usize>, ToolError> {
    args.get(key)
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                ToolError::InvalidArgument(format!("'{key}' must be an unsigned integer"))
            })
        })
        .transpose()
        .and_then(|value| {
            value
                .map(|raw| {
                    usize::try_from(raw)
                        .map_err(|_| ToolError::InvalidArgument(format!("'{key}' is too large")))
                })
                .transpose()
        })
}

fn get_direction(direction: Option<&str>) -> Result<Option<TraverseDirection>, ToolError> {
    match direction.map(|value| value.to_ascii_lowercase()) {
        None => Ok(None),
        Some(value) if value == "both" => Ok(None),
        Some(value) if value == "outbound" || value == "forward" => {
            Ok(Some(TraverseDirection::Forward))
        }
        Some(value) if value == "inbound" || value == "reverse" => {
            Ok(Some(TraverseDirection::Reverse))
        }
        Some(other) => Err(ToolError::InvalidArgument(format!(
            "unsupported direction: {other}"
        ))),
    }
}

fn get_aggregate_function(function: &str) -> Result<Option<AggregateFunction>, ToolError> {
    match function.to_ascii_lowercase().as_str() {
        "count" => Ok(None),
        "sum" => Ok(Some(AggregateFunction::Sum)),
        "avg" => Ok(Some(AggregateFunction::Avg)),
        "min" => Ok(Some(AggregateFunction::Min)),
        "max" => Ok(Some(AggregateFunction::Max)),
        other => Err(ToolError::InvalidArgument(format!(
            "unknown aggregation function: {other}"
        ))),
    }
}

fn graphql_variables(variables: &Value) -> Result<&Map<String, Value>, ToolError> {
    variables.as_object().ok_or_else(|| {
        ToolError::InvalidArgument("'variables' must be an object when provided".into())
    })
}

fn graphql_argument_json(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<Option<Value>, ToolError> {
    let Some(argument) = field.get_argument(name) else {
        return Ok(None);
    };

    let resolved = argument.node.clone().into_const_with(|variable_name| {
        let key = variable_name.to_string();
        let value = variables.get(&key).cloned().ok_or_else(|| {
            ToolError::InvalidArgument(format!("missing GraphQL variable '${key}'"))
        })?;
        GraphQlConstValue::from_json(value).map_err(|e| {
            ToolError::InvalidArgument(format!("invalid GraphQL variable '${key}': {e}"))
        })
    })?;

    resolved
        .into_json()
        .map(Some)
        .map_err(|e| ToolError::InvalidArgument(format!("invalid GraphQL argument '{name}': {e}")))
}

fn graphql_required<T: DeserializeOwned>(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<T, ToolError> {
    let value = graphql_argument_json(field, name, variables)?
        .ok_or_else(|| ToolError::InvalidArgument(format!("missing GraphQL argument '{name}'")))?;
    serde_json::from_value(value)
        .map_err(|e| ToolError::InvalidArgument(format!("invalid GraphQL argument '{name}': {e}")))
}

fn graphql_optional<T: DeserializeOwned>(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<Option<T>, ToolError> {
    graphql_argument_json(field, name, variables)?
        .map(|value| {
            serde_json::from_value(value).map_err(|e| {
                ToolError::InvalidArgument(format!("invalid GraphQL argument '{name}': {e}"))
            })
        })
        .transpose()
}

fn graphql_optional_filter(
    field: &GraphQlField,
    name: &str,
    variables: &Map<String, Value>,
) -> Result<Option<FilterNode>, ToolError> {
    parse_optional_filter(graphql_argument_json(field, name, variables)?)
}

fn graph_query_schemas<S: StorageAdapter>(
    handler: &AxonHandler<S>,
) -> Result<Vec<CollectionSchema>, ToolError> {
    let names = handler
        .list_collections(ListCollectionsRequest {})
        .map_err(to_tool_error)?;
    Ok(names
        .collections
        .iter()
        .filter_map(|meta| {
            handler
                .get_schema(&CollectionId::new(&meta.name))
                .ok()
                .flatten()
        })
        .collect())
}

fn validate_graphql_read_document(query: &str) -> Result<(), ToolError> {
    if query.trim().is_empty() {
        return Err(ToolError::InvalidArgument("empty query string".into()));
    }

    let document = parse_query(query)
        .map_err(|e| ToolError::InvalidArgument(format!("GraphQL syntax error: {e}")))?;
    for (_, operation) in document.operations.iter() {
        if operation.node.ty != OperationType::Query {
            return Err(ToolError::InvalidArgument(format!(
                "unsupported GraphQL operation: {}",
                operation.node.ty
            )));
        }
    }
    Ok(())
}

fn graphql_variables_value(variables: Value) -> Result<GraphQlVariables, ToolError> {
    if !variables.is_object() {
        return Err(ToolError::InvalidArgument(
            "'variables' must be an object when provided".into(),
        ));
    }
    Ok(GraphQlVariables::from_json(variables))
}

fn execute_dynamic_graphql_query<S: StorageAdapter + 'static>(
    handler: Arc<TokioMutex<AxonHandler<S>>>,
    query: &str,
    variables: Value,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    validate_graphql_read_document(query)?;

    let schemas = {
        let guard = handler.blocking_lock();
        graph_query_schemas(&guard)?
    };
    let gql_schema = axon_graphql::build_schema_with_handler(&schemas, Arc::clone(&handler))
        .map_err(ToolError::Internal)?;
    let request = GraphQlRequest::new(query.to_string())
        .variables(graphql_variables_value(variables)?)
        .data(caller.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .map_err(|e| ToolError::Internal(format!("failed to build GraphQL runtime: {e}")))?;
    let response = runtime.block_on(gql_schema.schema.execute(request));
    let body = serde_json::to_value(response).map_err(|e| ToolError::Internal(e.to_string()))?;
    text_tool_response(&body)
}

fn entity_to_graphql_json(entity: &Entity) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(entity.id.to_string()));
    map.insert(
        "collection".into(),
        Value::String(entity.collection.to_string()),
    );
    map.insert("version".into(), serde_json::json!(entity.version));
    map.insert("data".into(), entity.data.clone());
    if let Some(ns) = entity.created_at_ns {
        map.insert("createdAtNs".into(), serde_json::json!(ns));
    }
    if let Some(ns) = entity.updated_at_ns {
        map.insert("updatedAtNs".into(), serde_json::json!(ns));
    }
    Value::Object(map)
}

fn entity_connection_json(
    entities: Vec<Entity>,
    total_count: usize,
    next_cursor: Option<String>,
    has_previous_page: bool,
) -> Value {
    let start_cursor = entities.first().map(|entity| entity.id.to_string());
    let end_cursor = entities.last().map(|entity| entity.id.to_string());
    let edges = entities
        .into_iter()
        .map(|entity| {
            serde_json::json!({
                "node": entity_to_graphql_json(&entity),
                "cursor": entity.id.to_string()
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "edges": edges,
        "pageInfo": {
            "hasNextPage": next_cursor.is_some(),
            "hasPreviousPage": has_previous_page,
            "startCursor": start_cursor,
            "endCursor": end_cursor
        },
        "totalCount": total_count
    })
}

fn data_path_value<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = data;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn group_key_value(data: &Value, group_by: Option<&str>) -> Value {
    group_by
        .and_then(|field| data_path_value(data, field))
        .cloned()
        .unwrap_or(Value::Null)
}

fn group_key_sort_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

fn count_entities_json(entities: &[Entity], group_by: Option<&str>) -> Value {
    let mut groups = std::collections::BTreeMap::<String, (Value, usize)>::new();
    if group_by.is_some() {
        for entity in entities {
            let key = group_key_value(&entity.data, group_by);
            let sort_key = group_key_sort_value(&key);
            groups
                .entry(sort_key)
                .and_modify(|(_, count)| *count += 1)
                .or_insert((key, 1));
        }
    }

    serde_json::json!({
        "totalCount": entities.len(),
        "groups": groups.into_values().map(|(key, count)| {
            serde_json::json!({ "key": key, "count": count })
        }).collect::<Vec<_>>()
    })
}

fn numeric_value(entity: &Entity, field: &str) -> Result<Option<f64>, ToolError> {
    let Some(value) = data_path_value(&entity.data, field) else {
        return Ok(None);
    };
    match value {
        Value::Number(number) => number.as_f64().map(Some).ok_or_else(|| {
            ToolError::InvalidArgument(format!("field '{field}' is not a finite number"))
        }),
        Value::Null => Ok(None),
        _ => Err(ToolError::InvalidArgument(format!(
            "field '{field}' is not numeric"
        ))),
    }
}

fn aggregate_value(function: &AggregateFunction, values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let result = match function {
        AggregateFunction::Sum => values.iter().sum(),
        AggregateFunction::Avg => {
            let count = values.iter().fold(0.0, |acc, _| acc + 1.0);
            values.iter().sum::<f64>() / count
        }
        AggregateFunction::Min => values
            .iter()
            .copied()
            .fold(f64::INFINITY, |acc, value| acc.min(value)),
        AggregateFunction::Max => values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, |acc, value| acc.max(value)),
    };
    serde_json::json!(result)
}

fn aggregate_entities_json(
    entities: &[Entity],
    function: Option<AggregateFunction>,
    field: Option<&str>,
    group_by: Option<&str>,
) -> Result<Value, ToolError> {
    let mut grouped = std::collections::BTreeMap::<String, (Value, Vec<&Entity>)>::new();
    if group_by.is_some() {
        for entity in entities {
            let key = group_key_value(&entity.data, group_by);
            grouped
                .entry(group_key_sort_value(&key))
                .or_insert_with(|| (key, Vec::new()))
                .1
                .push(entity);
        }
    } else {
        grouped.insert("null".into(), (Value::Null, entities.iter().collect()));
    }

    let mut results = Vec::new();
    for (_sort_key, (key, group_entities)) in grouped {
        let count = group_entities.len();
        let value = if let Some(function) = function.as_ref() {
            let field = field.ok_or_else(|| {
                ToolError::InvalidArgument("missing GraphQL argument 'field'".into())
            })?;
            let mut numbers = Vec::new();
            for entity in &group_entities {
                if let Some(value) = numeric_value(entity, field)? {
                    numbers.push(value);
                }
            }
            aggregate_value(function, &numbers)
        } else {
            serde_json::json!(count)
        };
        results.push(serde_json::json!({
            "key": key,
            "value": value,
            "count": count
        }));
    }

    Ok(serde_json::json!({ "results": results }))
}

fn graphql_root_field_response<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    field: &GraphQlField,
    variables: &Map<String, Value>,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    match field.name.node.as_str() {
        "collections" => {
            let response = handler
                .list_collections(ListCollectionsRequest::default())
                .map_err(to_tool_error)?;
            Ok(Value::Array(
                response
                    .collections
                    .into_iter()
                    .map(|collection| {
                        serde_json::json!({
                            "name": collection.name,
                            "entityCount": collection.entity_count,
                            "schemaVersion": collection.schema_version,
                            "createdAtNs": collection.created_at_ns,
                            "updatedAtNs": collection.updated_at_ns
                        })
                    })
                    .collect(),
            ))
        }
        "entity" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let id: String = graphql_required(field, "id", variables)?;
            match handler.get_entity_with_caller(
                GetEntityRequest {
                    collection: CollectionId::new(&collection),
                    id: EntityId::new(&id),
                },
                caller,
                None,
            ) {
                Ok(response) => Ok(entity_to_graphql_json(&response.entity)),
                Err(AxonError::NotFound(_)) => Ok(Value::Null),
                Err(error) => Err(to_tool_error(error)),
            }
        }
        "entities" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let limit = graphql_optional(field, "limit", variables)?;
            let after = graphql_optional::<String>(field, "after", variables)?
                .map(|cursor| EntityId::new(&cursor));
            let has_previous_page = after.is_some();
            let response = handler
                .query_entities_with_caller(
                    QueryEntitiesRequest {
                        collection: CollectionId::new(&collection),
                        filter,
                        sort: Vec::new(),
                        limit,
                        after_id: after,
                        count_only: false,
                    },
                    caller,
                    None,
                )
                .map_err(to_tool_error)?;

            Ok(entity_connection_json(
                response.entities,
                response.total_count,
                response.next_cursor,
                has_previous_page,
            ))
        }
        "countEntities" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let group_by = graphql_optional::<String>(field, "groupBy", variables)?;
            let response = handler
                .query_entities_with_caller(
                    QueryEntitiesRequest {
                        collection: CollectionId::new(&collection),
                        filter,
                        sort: Vec::new(),
                        limit: None,
                        after_id: None,
                        count_only: false,
                    },
                    caller,
                    None,
                )
                .map_err(to_tool_error)?;

            Ok(count_entities_json(&response.entities, group_by.as_deref()))
        }
        "aggregate" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let function: String = graphql_required(field, "function", variables)?;
            let aggregate_function = get_aggregate_function(&function)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let group_by = graphql_optional::<String>(field, "groupBy", variables)?;
            let field_name = if aggregate_function.is_some() {
                Some(graphql_required::<String>(field, "field", variables)?)
            } else {
                None
            };

            let response = handler
                .query_entities_with_caller(
                    QueryEntitiesRequest {
                        collection: CollectionId::new(&collection),
                        filter,
                        sort: Vec::new(),
                        limit: None,
                        after_id: None,
                        count_only: false,
                    },
                    caller,
                    None,
                )
                .map_err(to_tool_error)?;
            aggregate_entities_json(
                &response.entities,
                aggregate_function,
                field_name.as_deref(),
                group_by.as_deref(),
            )
        }
        "linkCandidates" => {
            let source_collection: String = graphql_required(field, "sourceCollection", variables)?;
            let source_id: String = graphql_required(field, "sourceId", variables)?;
            let link_type: String = graphql_required(field, "linkType", variables)?;
            let filter = graphql_optional_filter(field, "filter", variables)?;
            let limit = graphql_optional(field, "limit", variables)?;
            let response = handler
                .find_link_candidates_with_caller(
                    FindLinkCandidatesRequest {
                        source_collection: CollectionId::new(&source_collection),
                        source_id: EntityId::new(&source_id),
                        link_type,
                        filter,
                        limit,
                    },
                    caller,
                    None,
                )
                .map_err(to_tool_error)?;

            Ok(serde_json::json!({
                "targetCollection": response.target_collection,
                "linkType": response.link_type,
                "cardinality": response.cardinality,
                "existingLinkCount": response.existing_link_count,
                "candidates": response.candidates.into_iter().map(|candidate| {
                    serde_json::json!({
                        "entity": entity_to_graphql_json(&candidate.entity),
                        "alreadyLinked": candidate.already_linked
                    })
                }).collect::<Vec<_>>()
            }))
        }
        "neighbors" => {
            let collection: String = graphql_required(field, "collection", variables)?;
            let id: String = graphql_required(field, "id", variables)?;
            let link_type = graphql_optional(field, "linkType", variables)?;
            let direction = get_direction(
                graphql_optional::<String>(field, "direction", variables)?.as_deref(),
            )?;
            let directions = match direction {
                Some(TraverseDirection::Forward) => vec![(TraverseDirection::Forward, "outbound")],
                Some(TraverseDirection::Reverse) => vec![(TraverseDirection::Reverse, "inbound")],
                None => vec![
                    (TraverseDirection::Forward, "outbound"),
                    (TraverseDirection::Reverse, "inbound"),
                ],
            };
            let collection_id = CollectionId::new(&collection);
            let entity_id = EntityId::new(&id);
            handler
                .get_entity_with_caller(
                    GetEntityRequest {
                        collection: collection_id.clone(),
                        id: entity_id.clone(),
                    },
                    caller,
                    None,
                )
                .map_err(to_tool_error)?;

            let mut groups = std::collections::BTreeMap::<(String, String), Vec<Value>>::new();
            let mut total_count = 0usize;
            for (direction, label) in directions {
                let response = handler
                    .traverse_with_caller(
                        TraverseRequest {
                            collection: collection_id.clone(),
                            id: entity_id.clone(),
                            link_type: link_type.clone(),
                            max_depth: Some(1),
                            direction,
                            hop_filter: None,
                        },
                        caller,
                        None,
                    )
                    .map_err(to_tool_error)?;
                for entity in response.entities {
                    total_count += 1;
                    let link_type = link_type.clone().unwrap_or_default();
                    groups
                        .entry((link_type.clone(), label.to_string()))
                        .or_default()
                        .push(serde_json::json!({
                            "cursor": entity.id.to_string(),
                            "node": entity_to_graphql_json(&entity),
                            "linkType": link_type,
                            "direction": label
                        }));
                }
            }
            let groups = groups
                .into_iter()
                .map(|((link_type, direction), edges)| {
                    serde_json::json!({
                        "linkType": link_type,
                        "direction": direction,
                        "edges": edges,
                        "totalCount": edges.len()
                    })
                })
                .collect::<Vec<_>>();

            Ok(serde_json::json!({
                "groups": groups,
                "pageInfo": {
                    "hasNextPage": false,
                    "hasPreviousPage": false,
                    "startCursor": Value::Null,
                    "endCursor": Value::Null
                },
                "totalCount": total_count
            }))
        }
        other => Err(ToolError::InvalidArgument(format!(
            "unsupported GraphQL root field: {other}"
        ))),
    }
}

fn select_value(value: Value, selection_set: &SelectionSet) -> Result<Value, ToolError> {
    if selection_set.items.is_empty() {
        return Ok(value);
    }

    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| select_value(item, selection_set))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut selected = Map::new();
            for selection in &selection_set.items {
                let Selection::Field(field) = &selection.node else {
                    return Err(ToolError::InvalidArgument(
                        "GraphQL fragments are not supported by axon.query".into(),
                    ));
                };

                let response_key = field.node.response_key().node.to_string();
                let field_name = field.node.name.node.to_string();
                let value = map.get(&field_name).cloned().unwrap_or(Value::Null);
                let projected = select_value(value, &field.node.selection_set.node)?;
                selected.insert(response_key, projected);
            }
            Ok(Value::Object(selected))
        }
        primitive => Ok(primitive),
    }
}

fn execute_graphql_query<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    query: &str,
    variables: &Value,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    if query.trim().is_empty() {
        return Err(ToolError::InvalidArgument("empty query string".into()));
    }

    let document = parse_query(query)
        .map_err(|e| ToolError::InvalidArgument(format!("GraphQL syntax error: {e}")))?;
    let mut operations = document.operations.iter();
    let (_, operation) = operations.next().ok_or_else(|| {
        ToolError::InvalidArgument("GraphQL document must contain an operation".into())
    })?;
    if operations.next().is_some() {
        return Err(ToolError::InvalidArgument(
            "multiple GraphQL operations are not supported".into(),
        ));
    }
    if operation.node.ty != OperationType::Query {
        return Err(ToolError::InvalidArgument(format!(
            "unsupported GraphQL operation: {}",
            operation.node.ty
        )));
    }
    if !document.fragments.is_empty() {
        return Err(ToolError::InvalidArgument(
            "GraphQL fragments are not supported by axon.query".into(),
        ));
    }

    let variables = graphql_variables(variables)?;
    let mut data = Map::new();
    for selection in &operation.node.selection_set.node.items {
        let Selection::Field(field) = &selection.node else {
            return Err(ToolError::InvalidArgument(
                "GraphQL fragments are not supported by axon.query".into(),
            ));
        };
        let response_key = field.node.response_key().node.to_string();
        let value = graphql_root_field_response(handler, &field.node, variables, caller)?;
        let projected = select_value(value, &field.node.selection_set.node)?;
        data.insert(response_key, projected);
    }

    Ok(Value::Object(data))
}

fn execute_create<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    collection: &str,
    args: &Value,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    if intent_mode(args)? == IntentToolMode::Commit {
        return execute_intent_commit(
            handler,
            caller,
            default_intent_scope(),
            intent_token_arg(args)?,
        );
    }

    let id = args
        .get("id")
        .and_then(|value| value.as_str())
        .map(EntityId::new)
        .unwrap_or_else(EntityId::generate);
    let data = args
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let request = CreateEntityRequest {
        collection: CollectionId::new(collection),
        id,
        data,
        actor: Some("mcp".into()),
        audit_metadata: None,
        attribution: None,
    };
    if intent_mode(args)? == IntentToolMode::Preview {
        let operation = canonical_create_entity_operation(&request);
        let preview = preview_create_entity(handler, &request)?;
        return execute_intent_preview(
            handler,
            caller,
            default_intent_scope(),
            operation,
            preview,
            expires_in_seconds(args),
        );
    }

    let explanation = explain_write_policy(
        handler,
        caller,
        ExplainPolicyRequest {
            operation: "create".into(),
            collection: Some(request.collection.clone()),
            entity_id: Some(request.id.clone()),
            expected_version: None,
            data: Some(request.data.clone()),
            patch: None,
            lifecycle_name: None,
            target_state: None,
            to_version: None,
            operations: Vec::new(),
        },
    );
    match handler.create_entity_with_caller(request, caller, None) {
        Ok(response) => text_tool_response(&response.entity),
        Err(error) => policy_denial_result("create", error, explanation),
    }
}

fn execute_get<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let id = get_required_string(args, "id")?;
    let response = handler
        .get_entity(GetEntityRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(&id),
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response.entity)
}

fn execute_patch<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    collection: &str,
    args: &Value,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    if intent_mode(args)? == IntentToolMode::Commit {
        return execute_intent_commit(
            handler,
            caller,
            default_intent_scope(),
            intent_token_arg(args)?,
        );
    }

    let id = get_required_string(args, "id")?;
    let data = args
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let expected_version = args
        .get("expected_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'expected_version'".into()))?;

    let request = PatchEntityRequest {
        collection: CollectionId::new(collection),
        id: EntityId::new(&id),
        patch: data,
        expected_version,
        actor: Some("mcp".into()),
        audit_metadata: None,
        attribution: None,
    };
    if intent_mode(args)? == IntentToolMode::Preview {
        let operation = canonical_patch_entity_operation(&request);
        let preview = preview_patch_entity(handler, &request)?;
        return execute_intent_preview(
            handler,
            caller,
            default_intent_scope(),
            operation,
            preview,
            expires_in_seconds(args),
        );
    }

    let explanation = explain_write_policy(
        handler,
        caller,
        ExplainPolicyRequest {
            operation: "patch".into(),
            collection: Some(request.collection.clone()),
            entity_id: Some(request.id.clone()),
            expected_version: Some(request.expected_version),
            data: None,
            patch: Some(request.patch.clone()),
            lifecycle_name: None,
            target_state: None,
            to_version: None,
            operations: Vec::new(),
        },
    );
    match handler.patch_entity_with_caller(request, caller, None) {
        Ok(response) => text_tool_response(&response.entity),
        Err(error) => policy_denial_result("patch", error, explanation),
    }
}

fn execute_delete<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
    collection: &str,
    args: &Value,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    if intent_mode(args)? == IntentToolMode::Commit {
        return execute_intent_commit(
            handler,
            caller,
            default_intent_scope(),
            intent_token_arg(args)?,
        );
    }

    let id = get_required_string(args, "id")?;
    let request = DeleteEntityRequest {
        collection: CollectionId::new(collection),
        id: EntityId::new(&id),
        actor: Some("mcp".into()),
        audit_metadata: None,
        force: false,
        attribution: None,
    };
    if intent_mode(args)? == IntentToolMode::Preview {
        let expected_version = args.get("expected_version").and_then(Value::as_u64);
        let operation = if expected_version.is_some() {
            canonicalize_intent_operation(
                MutationOperationKind::DeleteEntity,
                serde_json::json!({
                    "collection": &request.collection,
                    "id": &request.id,
                    "expected_version": expected_version,
                }),
            )
        } else {
            canonical_delete_entity_operation(&request)
        };
        let preview = preview_delete_entity(handler, &request, expected_version)?;
        return execute_intent_preview(
            handler,
            caller,
            default_intent_scope(),
            operation,
            preview,
            expires_in_seconds(args),
        );
    }

    let explanation = explain_write_policy(
        handler,
        caller,
        ExplainPolicyRequest {
            operation: "delete".into(),
            collection: Some(request.collection.clone()),
            entity_id: Some(request.id.clone()),
            expected_version: None,
            data: None,
            patch: None,
            lifecycle_name: None,
            target_state: None,
            to_version: None,
            operations: Vec::new(),
        },
    );
    match handler.delete_entity_with_caller(request, caller, None) {
        Ok(response) => text_tool_response(&serde_json::json!({
            "collection": response.collection,
            "id": response.id,
            "status": "deleted",
        })),
        Err(error) => policy_denial_result("delete", error, explanation),
    }
}

fn execute_query<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    args: &Value,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'query' string".into()))?;
    let variables = args
        .get("variables")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let data = execute_graphql_query(handler, query, &variables, caller)?;
    text_tool_response(&serde_json::json!({ "data": data }))
}

fn execute_query_tokio<S: StorageAdapter + 'static>(
    handler: Arc<TokioMutex<AxonHandler<S>>>,
    args: &Value,
    caller: &CallerIdentity,
) -> Result<Value, ToolError> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'query' string".into()))?;
    let variables = args
        .get("variables")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    execute_dynamic_graphql_query(handler, query, variables, caller)
}

fn execute_aggregate<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let function_name = get_required_string(args, "function")?;
    let aggregate_function = get_aggregate_function(&function_name)?;
    let filter = parse_optional_filter(args.get("filter").cloned())?;
    let group_by = get_optional_string(args, "group_by");

    let result = if let Some(function) = aggregate_function {
        let field = get_required_string(args, "field")?;
        let response = handler
            .aggregate(AggregateRequest {
                collection: CollectionId::new(collection),
                function,
                field,
                filter,
                group_by,
            })
            .map_err(to_tool_error)?;
        serde_json::json!({ "results": response.results })
    } else {
        let response = handler
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new(collection),
                filter,
                group_by,
            })
            .map_err(to_tool_error)?;
        return text_tool_response(&response);
    };

    text_tool_response(&result)
}

fn execute_link_candidates<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let source_id = args
        .get("source_id")
        .and_then(Value::as_str)
        .or_else(|| args.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ToolError::InvalidArgument("missing 'source_id'".into()))?;
    let link_type = get_required_string(args, "link_type")?;
    let filter = parse_optional_filter(args.get("filter").cloned())?;
    let limit = get_optional_usize(args, "limit")?;

    let response = handler
        .find_link_candidates(FindLinkCandidatesRequest {
            source_collection: CollectionId::new(collection),
            source_id: EntityId::new(&source_id),
            link_type,
            filter,
            limit,
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response)
}

fn execute_neighbors<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    collection: &str,
    args: &Value,
) -> Result<Value, ToolError> {
    let id = get_required_string(args, "id")?;
    let link_type = get_optional_string(args, "link_type");
    let direction = get_direction(args.get("direction").and_then(Value::as_str))?;

    let response = handler
        .list_neighbors(ListNeighborsRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(&id),
            link_type,
            direction,
        })
        .map_err(to_tool_error)?;
    text_tool_response(&response)
}

fn with_intent_mode_schema(mut schema: Value) -> Value {
    if let Value::Object(map) = &mut schema {
        if let Some(Value::Object(properties)) = map.get_mut("properties") {
            properties.insert(
                "preview".into(),
                serde_json::json!({
                    "type": "boolean",
                    "description": "Preview the mutation and return a mutation intent outcome without committing"
                }),
            );
            properties.insert(
                "intent_mode".into(),
                serde_json::json!({
                    "type": "string",
                    "enum": ["direct", "preview", "commit"],
                    "description": "Mutation intent mode"
                }),
            );
            properties.insert(
                "intent_token".into(),
                serde_json::json!({
                    "type": "string",
                    "description": "Opaque mutation intent token for commit mode"
                }),
            );
            properties.insert(
                "expires_in_seconds".into(),
                serde_json::json!({
                    "type": "integer",
                    "description": "Preview token TTL in seconds"
                }),
            );
        }
    }
    schema
}

fn build_create_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
    caller: CallerIdentity,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.create"),
        description: format!("Create a new entity in the {col} collection"),
        input_schema: with_intent_mode_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID (optional, auto-generated UUIDv7 if omitted)" },
                "data": { "type": "object", "description": "Entity data" }
            },
            "required": ["data"]
        })),
        handler: Box::new(move |args| {
            let mut guard = lock_handler(&handler)?;
            execute_create(&mut guard, &col, args, &caller)
        }),
    }
}

fn build_get_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.get"),
        description: format!("Get an entity from the {col} collection by ID"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_get(&guard, &col, args)
        }),
    }
}

fn build_patch_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
    caller: CallerIdentity,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.patch"),
        description: format!("Merge-patch an entity in the {col} collection"),
        input_schema: with_intent_mode_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "data": { "type": "object", "description": "Merge-patch data" },
                "expected_version": { "type": "integer", "description": "Expected version for OCC" }
            },
            "required": ["id", "data", "expected_version"]
        })),
        handler: Box::new(move |args| {
            let mut guard = lock_handler(&handler)?;
            execute_patch(&mut guard, &col, args, &caller)
        }),
    }
}

fn build_delete_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
    caller: CallerIdentity,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.delete"),
        description: format!("Delete an entity from the {col} collection"),
        input_schema: with_intent_mode_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "expected_version": { "type": "integer", "description": "Optional expected version for intent previews" }
            },
            "required": ["id"]
        })),
        handler: Box::new(move |args| {
            let mut guard = lock_handler(&handler)?;
            execute_delete(&mut guard, &col, args, &caller)
        }),
    }
}

/// Build the `axon.query` tool for GraphQL queries via MCP.
///
/// Accepts a `query` string and optional `variables` object and executes a
/// limited GraphQL read surface against the live handler.
pub fn build_query_tool<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
    caller: CallerIdentity,
) -> ToolDef {
    ToolDef {
        name: "axon.query".into(),
        description: "Execute a live GraphQL read query against Axon. Accepts a query string and optional variables.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "GraphQL query string"
                },
                "variables": {
                    "type": "object",
                    "description": "Optional variables for the query"
                }
            },
            "required": ["query"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_query(&guard, args, &caller)
        }),
    }
}

/// Build the global `axon.transition_lifecycle` tool (FEAT-015).
///
/// Transitions an entity through a named lifecycle state machine declared in
/// its collection schema. `expected_version` is optional: when omitted, the
/// tool reads the current entity version and uses it, which is the usual
/// ergonomic mode for agent-driven callers. Supply it explicitly for strict
/// OCC-guarded transitions.
///
/// Error mapping flows through [`to_tool_error`] so that `LifecycleNotFound`
/// becomes `ToolError::NotFound` and `InvalidTransition` becomes
/// `ToolError::InvalidArgument` with the list of valid transitions embedded
/// in the message.
pub fn build_transition_lifecycle_tool<S: StorageAdapter + 'static>(
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    ToolDef {
        name: "axon.transition_lifecycle".into(),
        description: "Transition an entity through a named lifecycle state machine. \
            Returns the updated entity on success, or a structured error listing the \
            valid transitions if the requested target state is not reachable."
            .into(),
        input_schema: with_intent_mode_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "collection_id": {
                    "type": "string",
                    "description": "Collection name containing the entity"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity ID to transition"
                },
                "lifecycle_name": {
                    "type": "string",
                    "description": "Name of the lifecycle declared in the collection schema"
                },
                "target_state": {
                    "type": "string",
                    "description": "The state to transition to"
                },
                "expected_version": {
                    "type": "integer",
                    "description": "Optional OCC guard — if omitted, the current entity version is used"
                }
            },
            "required": ["collection_id", "entity_id", "lifecycle_name", "target_state"]
        })),
        handler: Box::new(move |args| {
            let caller = CallerIdentity::anonymous();
            if intent_mode(args)? == IntentToolMode::Commit {
                let mut guard = lock_handler(&handler)?;
                return execute_intent_commit(
                    &mut guard,
                    &caller,
                    default_intent_scope(),
                    intent_token_arg(args)?,
                );
            }

            let collection_id = get_required_string(args, "collection_id")?;
            let entity_id = get_required_string(args, "entity_id")?;
            let lifecycle_name = get_required_string(args, "lifecycle_name")?;
            let target_state = get_required_string(args, "target_state")?;

            let cid = CollectionId::new(&collection_id);
            let eid = EntityId::new(&entity_id);

            let expected_version = match args.get("expected_version") {
                Some(Value::Null) | None => {
                    // Read current version so callers can omit the OCC guard.
                    let guard = lock_handler(&handler)?;
                    let resp = guard
                        .get_entity(GetEntityRequest {
                            collection: cid.clone(),
                            id: eid.clone(),
                        })
                        .map_err(to_tool_error)?;
                    resp.entity.version
                }
                Some(v) => v.as_u64().ok_or_else(|| {
                    ToolError::InvalidArgument("'expected_version' must be a u64".into())
                })?,
            };

            let mut guard = lock_handler(&handler)?;
            let request = TransitionLifecycleRequest {
                collection_id: cid,
                entity_id: eid,
                lifecycle_name,
                target_state,
                expected_version,
                actor: Some("mcp".into()),
                audit_metadata: None,
                attribution: None,
            };
            if intent_mode(args)? == IntentToolMode::Preview {
                let operation = canonical_transition_lifecycle_operation(&request);
                let preview = preview_transition_entity(&guard, &request)?;
                return execute_intent_preview(
                    &mut guard,
                    &caller,
                    default_intent_scope(),
                    operation,
                    preview,
                    expires_in_seconds(args),
                );
            }

            let resp = guard.transition_lifecycle(request).map_err(to_tool_error)?;

            text_tool_response(&resp.entity)
        }),
    }
}

/// Build a `{collection}.aggregate` tool for MCP.
///
/// Accepts structured aggregation requests: function, field, optional filter and group_by.
pub fn build_aggregate_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.aggregate"),
        description: format!("Run an aggregation query on the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "function": {
                    "type": "string",
                    "enum": ["count", "sum", "avg", "min", "max"],
                    "description": "Aggregation function"
                },
                "field": {
                    "type": "string",
                    "description": "Field to aggregate"
                },
                "filter": {
                    "type": "object",
                    "description": "Optional filter to restrict entities"
                },
                "group_by": {
                    "type": "string",
                    "description": "Optional field to group results by"
                }
            },
            "required": ["function"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_aggregate(&guard, &col, args)
        }),
    }
}

/// Build `{collection}.link_candidates` tool.
///
/// Returns candidate entities that can be linked from the given source entity.
pub fn build_link_candidates_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.link_candidates"),
        description: format!("Find candidate target entities for a link from the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Source entity ID" },
                "link_type": { "type": "string", "description": "Link type to discover candidates for" },
                "filter": { "type": "object", "description": "Optional filter applied to candidate entities" },
                "limit": { "type": "integer", "description": "Maximum number of candidates to return" }
            },
            "required": ["id", "link_type"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_link_candidates(&guard, &col, args)
        }),
    }
}

/// Build `{collection}.neighbors` tool.
///
/// Returns linked entities for a given entity.
pub fn build_neighbors_tool<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<Mutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.neighbors"),
        description: format!("Find entities linked to a {col} entity"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "link_type": { "type": "string", "description": "Optional link type filter" },
                "direction": {
                    "type": "string",
                    "enum": ["outbound", "inbound", "both"],
                    "description": "Link direction (default: both)"
                }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let guard = lock_handler(&handler)?;
            execute_neighbors(&guard, &col, args)
        }),
    }
}

/// Build CRUD tools backed by a Tokio mutex.
pub fn build_crud_tools_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
    caller: CallerIdentity,
) -> Vec<ToolDef> {
    let col = collection.to_string();
    vec![
        ToolDef {
            name: format!("{col}.create"),
            description: format!("Create a new entity in the {col} collection"),
            input_schema: with_intent_mode_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID (optional, auto-generated UUIDv7 if omitted)" },
                    "data": { "type": "object", "description": "Entity data" }
                },
                "required": ["data"]
            })),
            handler: {
                let handler = Arc::clone(&handler);
                let col = col.clone();
                let caller = caller.clone();
                Box::new(move |args| {
                    let mut guard = handler.blocking_lock();
                    execute_create(&mut guard, &col, args, &caller)
                })
            },
        },
        ToolDef {
            name: format!("{col}.get"),
            description: format!("Get an entity from the {col} collection by ID"),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" }
                },
                "required": ["id"]
            }),
            handler: {
                let handler = Arc::clone(&handler);
                let col = col.clone();
                Box::new(move |args| {
                    let guard = handler.blocking_lock();
                    execute_get(&guard, &col, args)
                })
            },
        },
        ToolDef {
            name: format!("{col}.patch"),
            description: format!("Merge-patch an entity in the {col} collection"),
            input_schema: with_intent_mode_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" },
                    "data": { "type": "object", "description": "Merge-patch data" },
                    "expected_version": { "type": "integer", "description": "Expected version for OCC" }
                },
                "required": ["id", "data", "expected_version"]
            })),
            handler: {
                let handler = Arc::clone(&handler);
                let col = col.clone();
                let caller = caller.clone();
                Box::new(move |args| {
                    let mut guard = handler.blocking_lock();
                    execute_patch(&mut guard, &col, args, &caller)
                })
            },
        },
        ToolDef {
            name: format!("{col}.delete"),
            description: format!("Delete an entity from the {col} collection"),
            input_schema: with_intent_mode_schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Entity ID" },
                    "expected_version": { "type": "integer", "description": "Optional expected version for intent previews" }
                },
                "required": ["id"]
            })),
            handler: {
                let handler = Arc::clone(&handler);
                let caller = caller.clone();
                Box::new(move |args| {
                    let mut guard = handler.blocking_lock();
                    execute_delete(&mut guard, &col, args, &caller)
                })
            },
        },
    ]
}

/// Build the `axon.query` tool backed by a Tokio mutex.
pub fn build_query_tool_tokio<S: StorageAdapter + 'static>(
    handler: Arc<TokioMutex<AxonHandler<S>>>,
    caller: CallerIdentity,
) -> ToolDef {
    ToolDef {
        name: "axon.query".into(),
        description: "Execute a live GraphQL read query against Axon. Accepts a query string and optional variables.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "GraphQL query string"
                },
                "variables": {
                    "type": "object",
                    "description": "Optional variables for the query"
                }
            },
            "required": ["query"]
        }),
        handler: Box::new(move |args| {
            execute_query_tokio(Arc::clone(&handler), args, &caller)
        }),
    }
}

/// Build a collection aggregate tool backed by a Tokio mutex.
pub fn build_aggregate_tool_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.aggregate"),
        description: format!("Run an aggregation query on the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "function": {
                    "type": "string",
                    "enum": ["count", "sum", "avg", "min", "max"],
                    "description": "Aggregation function"
                },
                "field": {
                    "type": "string",
                    "description": "Field to aggregate"
                },
                "filter": {
                    "type": "object",
                    "description": "Optional filter to restrict entities"
                },
                "group_by": {
                    "type": "string",
                    "description": "Optional field to group results by"
                }
            },
            "required": ["function"]
        }),
        handler: Box::new(move |args| {
            let guard = handler.blocking_lock();
            execute_aggregate(&guard, &col, args)
        }),
    }
}

/// Build `{collection}.link_candidates` backed by a Tokio mutex.
pub fn build_link_candidates_tool_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.link_candidates"),
        description: format!("Find candidate target entities for a link from the {col} collection"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Source entity ID" },
                "link_type": { "type": "string", "description": "Link type to discover candidates for" },
                "filter": { "type": "object", "description": "Optional filter applied to candidate entities" },
                "limit": { "type": "integer", "description": "Maximum number of candidates to return" }
            },
            "required": ["id", "link_type"]
        }),
        handler: Box::new(move |args| {
            let translated = serde_json::json!({
                "source_id": args.get("id").cloned().unwrap_or(Value::Null),
                "link_type": args.get("link_type").cloned().unwrap_or(Value::Null),
                "filter": args.get("filter").cloned().unwrap_or(Value::Null),
                "limit": args.get("limit").cloned().unwrap_or(Value::Null),
            });
            let guard = handler.blocking_lock();
            execute_link_candidates(&guard, &col, &translated)
        }),
    }
}

/// Build `{collection}.neighbors` backed by a Tokio mutex.
pub fn build_neighbors_tool_tokio<S: StorageAdapter + 'static>(
    collection: &str,
    handler: Arc<TokioMutex<AxonHandler<S>>>,
) -> ToolDef {
    let col = collection.to_string();
    ToolDef {
        name: format!("{col}.neighbors"),
        description: format!("Find entities linked to a {col} entity"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Entity ID" },
                "link_type": { "type": "string", "description": "Optional link type filter" },
                "direction": {
                    "type": "string",
                    "enum": ["outbound", "inbound", "both"],
                    "description": "Link direction (default: both)"
                }
            },
            "required": ["id"]
        }),
        handler: Box::new(move |args| {
            let guard = handler.blocking_lock();
            execute_neighbors(&guard, &col, args)
        }),
    }
}

fn to_tool_error(err: axon_core::error::AxonError) -> ToolError {
    use axon_core::error::AxonError;
    match err {
        AxonError::NotFound(msg) => ToolError::NotFound(msg),
        AxonError::ConflictingVersion {
            expected,
            actual,
            current_entity,
        } => {
            let entity_json = current_entity
                .as_ref()
                .and_then(|e| serde_json::to_string(e).ok())
                .unwrap_or_else(|| "null".to_string());
            ToolError::Conflict(format!(
                "version conflict: expected {expected}, actual {actual}, current_entity: {entity_json}"
            ))
        }
        AxonError::InvalidArgument(msg) | AxonError::InvalidOperation(msg) => {
            ToolError::InvalidArgument(msg)
        }
        AxonError::SchemaValidation(msg) => ToolError::InvalidArgument(msg),
        AxonError::AlreadyExists(msg) => ToolError::Conflict(msg),
        AxonError::UniqueViolation { field, value } => {
            ToolError::Conflict(format!("unique violation on {field}: {value}"))
        }
        AxonError::Storage(msg) => ToolError::Internal(msg),
        AxonError::Serialization(e) => ToolError::Internal(e.to_string()),
        AxonError::LifecycleNotFound { lifecycle_name } => {
            ToolError::NotFound(format!("lifecycle not found: {lifecycle_name}"))
        }
        AxonError::InvalidTransition {
            lifecycle_name,
            current_state,
            target_state,
            valid_transitions,
        } => ToolError::InvalidArgument(format!(
            "invalid transition in lifecycle `{lifecycle_name}`: \
             cannot go from `{current_state}` to `{target_state}`; \
             valid transitions: [{}]",
            valid_transitions.join(", ")
        )),
        AxonError::LifecycleFieldMissing { field } => ToolError::InvalidArgument(format!(
            "lifecycle field `{field}` is missing from entity data"
        )),
        AxonError::LifecycleStateInvalid { field, actual } => ToolError::InvalidArgument(format!(
            "lifecycle field `{field}` has invalid value {actual}"
        )),
        AxonError::RateLimitExceeded { actor, retry_after_ms } => ToolError::InvalidArgument(format!(
            "rate limit exceeded for actor '{actor}'; retry after {retry_after_ms}ms"
        )),
        AxonError::Forbidden(msg) => ToolError::InvalidArgument(format!("forbidden: {msg}")),
        AxonError::PolicyDenied(denial) => ToolError::InvalidArgument(denial.to_string()),
        AxonError::ScopeViolation {
            actor,
            entity_id,
            filter_field,
            filter_value,
        } => ToolError::InvalidArgument(format!(
            "scope violation: actor '{actor}' denied access to entity '{entity_id}' (filter {filter_field}={filter_value})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolDef;
    use axon_api::handler::AxonHandler;
    use axon_api::request::{CreateCollectionRequest, CreateLinkRequest};
    use axon_api::test_fixtures::seed_procurement_fixture;
    use axon_schema::schema::{Cardinality, CollectionSchema, LinkTypeDef};
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::{json, Value};

    fn make_handler() -> Arc<Mutex<AxonHandler<MemoryStorageAdapter>>> {
        Arc::new(Mutex::new(
            AxonHandler::new(MemoryStorageAdapter::default()),
        ))
    }

    fn make_graph_handler() -> Arc<Mutex<AxonHandler<MemoryStorageAdapter>>> {
        let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

        let mut tasks_schema = CollectionSchema::new(CollectionId::new("tasks"));
        tasks_schema.link_types.insert(
            "depends-on".into(),
            LinkTypeDef {
                target_collection: "tasks".into(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );

        handler
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("tasks"),
                schema: tasks_schema,
                actor: Some("test".into()),
            })
            .expect("tasks collection fixture should be created");
        handler
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("users"),
                schema: CollectionSchema::new(CollectionId::new("users")),
                actor: Some("test".into()),
            })
            .expect("users collection fixture should be created");

        for (collection, id, title, status, points) in [
            ("tasks", "t-001", "First task", "ready", 10),
            ("tasks", "t-002", "Second task", "in_progress", 20),
            ("tasks", "t-003", "Third task", "ready", 5),
        ] {
            handler
                .create_entity(CreateEntityRequest {
                    collection: CollectionId::new(collection),
                    id: EntityId::new(id),
                    data: json!({
                        "title": title,
                        "status": status,
                        "points": points
                    }),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("graph fixture entities should be created");
        }

        handler
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-001"),
                data: json!({ "title": "Owner" }),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .expect("users entity fixture should be created");

        for (source_collection, source_id, target_collection, target_id, link_type) in [
            ("tasks", "t-001", "tasks", "t-002", "depends-on"),
            ("tasks", "t-001", "tasks", "t-003", "depends-on"),
            ("users", "u-001", "tasks", "t-001", "assigned-to"),
        ] {
            handler
                .create_link(CreateLinkRequest {
                    source_collection: CollectionId::new(source_collection),
                    source_id: EntityId::new(source_id),
                    target_collection: CollectionId::new(target_collection),
                    target_id: EntityId::new(target_id),
                    link_type: link_type.into(),
                    metadata: Value::Null,
                    actor: None,
                    attribution: None,
                })
                .expect("graph fixture links should be created");
        }

        Arc::new(Mutex::new(handler))
    }

    fn parse_tool_payload(result: &Value) -> Value {
        serde_json::from_str(
            result["content"][0]["text"]
                .as_str()
                .expect("tool response should contain text content"),
        )
        .expect("tool response payload should be valid JSON")
    }

    fn invoke_tool(tool: &ToolDef, args: Value) -> Value {
        (tool.handler)(&args).expect("tool invocation should succeed")
    }

    fn invoke_tool_err(tool: &ToolDef, args: Value) -> ToolError {
        (tool.handler)(&args).expect_err("tool invocation should fail")
    }

    #[test]
    fn create_and_get_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler), CallerIdentity::anonymous());

        // Create
        let create_tool = &tools[0];
        assert_eq!(create_tool.name, "tasks.create");
        let result = invoke_tool(
            create_tool,
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Test task"}
            }),
        );
        let entity = parse_tool_payload(&result);
        assert_eq!(entity["id"], "t-001");
        assert_eq!(entity["data"]["title"], "Test task");

        // Get
        let get_tool = &tools[1];
        assert_eq!(get_tool.name, "tasks.get");
        let result = invoke_tool(get_tool, serde_json::json!({"id": "t-001"}));
        let entity = parse_tool_payload(&result);
        assert_eq!(entity["data"]["title"], "Test task");
    }

    #[test]
    fn procurement_fixture_can_seed_mcp_handler() {
        let handler = make_handler();
        let fixture = {
            let mut guard = handler
                .lock()
                .expect("procurement fixture handler should lock");
            seed_procurement_fixture(&mut guard).expect("procurement fixture should seed")
        };

        let tools = build_crud_tools(
            fixture.collections.users.as_str(),
            Arc::clone(&handler),
            CallerIdentity::anonymous(),
        );
        let get_tool = &tools[1];
        assert_eq!(get_tool.name, "users.get");

        let result = invoke_tool(
            get_tool,
            json!({ "id": fixture.ids.finance_agent.as_str() }),
        );
        let user = parse_tool_payload(&result);
        let expected = fixture
            .entity(&fixture.collections.users, &fixture.ids.finance_agent)
            .expect("finance agent should be fixture data");

        assert_eq!(user["data"]["user_id"], expected.data["user_id"]);
        assert_eq!(user["data"]["procurement_role"], json!("finance_agent"));
    }

    #[test]
    fn patch_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler), CallerIdentity::anonymous());

        // Create first
        invoke_tool(
            &tools[0],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Original"}
            }),
        );

        // Patch
        let patch_tool = &tools[2];
        let result = invoke_tool(
            patch_tool,
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Updated"},
                "expected_version": 1
            }),
        );
        let entity = parse_tool_payload(&result);
        assert_eq!(entity["data"]["title"], "Updated");
        assert_eq!(entity["version"], 2);
    }

    #[test]
    fn delete_via_mcp_tools() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler), CallerIdentity::anonymous());

        // Create
        invoke_tool(
            &tools[0],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "Delete me"}
            }),
        );

        // Delete
        let delete_tool = &tools[3];
        let result = invoke_tool(delete_tool, serde_json::json!({"id": "t-001"}));
        let text = result["content"][0]["text"]
            .as_str()
            .expect("delete tool should return text content");
        assert!(text.contains("t-001"));

        // Verify deleted
        let get_result = (tools[1].handler)(&serde_json::json!({"id": "t-001"}));
        assert!(get_result.is_err());
    }

    #[test]
    fn version_conflict_returns_current_entity() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler), CallerIdentity::anonymous());

        // Create
        invoke_tool(
            &tools[0],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "V1"}
            }),
        );

        // Patch with wrong version
        let err = invoke_tool_err(
            &tools[2],
            serde_json::json!({
                "id": "t-001",
                "data": {"title": "V2"},
                "expected_version": 99
            }),
        );

        match err {
            ToolError::Conflict(msg) => {
                assert!(msg.contains("version conflict"));
                assert!(msg.contains("current_entity"));
            }
            other => panic!("expected Conflict, got: {other:?}"),
        }
    }

    #[test]
    fn missing_id_returns_invalid_argument() {
        let handler = make_handler();
        let tools = build_crud_tools("tasks", Arc::clone(&handler), CallerIdentity::anonymous());

        let err = invoke_tool_err(&tools[1], serde_json::json!({}));
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── axon.query tool tests ──────────────────────────────────────────

    #[test]
    fn query_tool_executes_live_handler_queries() {
        let handler = make_graph_handler();
        let tool = build_query_tool(Arc::clone(&handler), CallerIdentity::anonymous());
        assert_eq!(tool.name, "axon.query");
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "query": r"query($collection: String!, $id: String!) {
                collections { name entityCount }
                entity(collection: $collection, id: $id) { id data }
                entities(collection: $collection, limit: 2) {
                    totalCount
                    pageInfo { hasNextPage endCursor }
                    edges { cursor node { id data } }
                }
            }",
                "variables": {
                    "collection": "tasks",
                    "id": "t-001"
                }
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["data"]["collections"][0]["name"], "tasks");
        assert_eq!(parsed["data"]["collections"][0]["entityCount"], 3);
        assert_eq!(parsed["data"]["entity"]["id"], "t-001");
        assert_eq!(parsed["data"]["entity"]["data"]["title"], "First task");
        assert_eq!(parsed["data"]["entities"]["totalCount"], 3);
        assert_eq!(
            parsed["data"]["entities"]["edges"][0]["node"]["id"],
            "t-001"
        );
    }

    #[test]
    fn query_tool_rejects_invalid_graphql_syntax() {
        let tool = build_query_tool(make_graph_handler(), CallerIdentity::anonymous());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "query": "{ collections { name }"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_unsupported_root_fields() {
        let tool = build_query_tool(make_graph_handler(), CallerIdentity::anonymous());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "query": "{ tasks { id } }"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_mutations_until_graphql_writes_exist() {
        let tool = build_query_tool(make_graph_handler(), CallerIdentity::anonymous());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "query": "mutation { collections { name } }"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn query_tool_rejects_missing_query() {
        let tool = build_query_tool(make_graph_handler(), CallerIdentity::anonymous());
        let err = invoke_tool_err(&tool, serde_json::json!({}));
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── aggregate tool tests ───────────────────────────────────────────

    #[test]
    fn aggregate_tool_returns_live_count_results() {
        let handler = make_graph_handler();
        let tool = build_aggregate_tool("tasks", Arc::clone(&handler));
        assert_eq!(tool.name, "tasks.aggregate");
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "function": "count",
                "group_by": "status"
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["total_count"], 3);
        assert_eq!(
            parsed["groups"]
                .as_array()
                .expect("aggregate groups should be an array")
                .len(),
            2
        );
    }

    #[test]
    fn aggregate_tool_returns_live_numeric_aggregates() {
        let handler = make_graph_handler();
        let tool = build_aggregate_tool("tasks", Arc::clone(&handler));
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "function": "sum",
                "field": "points"
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["results"][0]["value"], 35.0);
        assert_eq!(parsed["results"][0]["count"], 3);
    }

    #[test]
    fn aggregate_tool_rejects_unknown_function() {
        let tool = build_aggregate_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "function": "median",
                "field": "x"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[test]
    fn aggregate_tool_requires_function() {
        let tool = build_aggregate_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(&tool, serde_json::json!({"field": "x"}));
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    // ── link tools tests ───────────────────────────────────────────────

    #[test]
    fn link_candidates_tool_returns_live_candidates() {
        let handler = make_graph_handler();
        let tool = build_link_candidates_tool("tasks", Arc::clone(&handler));
        assert_eq!(tool.name, "tasks.link_candidates");
        let result = invoke_tool(
            &tool,
            serde_json::json!({
                "id": "t-001",
                "link_type": "depends-on"
            }),
        );
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["target_collection"], "tasks");
        assert_eq!(parsed["existing_link_count"], 2);
        let already_linked = parsed["candidates"]
            .as_array()
            .expect("link candidate payload should include a candidates array")
            .iter()
            .find(|candidate| candidate["entity"]["id"] == "t-002")
            .expect("existing linked entity should appear in the candidates list");
        assert!(already_linked["already_linked"]
            .as_bool()
            .expect("candidate payload should include already_linked"));
    }

    #[test]
    fn link_candidates_tool_maps_not_found_errors() {
        let tool = build_link_candidates_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "id": "ghost",
                "link_type": "depends-on"
            }),
        );
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn neighbors_tool_returns_live_neighbors() {
        let handler = make_graph_handler();
        let tool = build_neighbors_tool("tasks", Arc::clone(&handler));
        assert_eq!(tool.name, "tasks.neighbors");
        let result = invoke_tool(&tool, serde_json::json!({"id": "t-001"}));
        let parsed = parse_tool_payload(&result);
        assert_eq!(parsed["total_count"], 3);
        assert_eq!(
            parsed["groups"]
                .as_array()
                .expect("neighbors payload should include grouped results")
                .len(),
            2
        );
    }

    #[test]
    fn neighbors_tool_rejects_invalid_direction() {
        let tool = build_neighbors_tool("tasks", make_graph_handler());
        let err = invoke_tool_err(
            &tool,
            serde_json::json!({
                "id": "t-001",
                "direction": "sideways"
            }),
        );
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
