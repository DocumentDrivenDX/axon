use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn query_compile_summary(report: &axon_schema::CompileReport) -> String {
    report
        .queries
        .iter()
        .filter(|diagnostic| diagnostic.status != axon_schema::NamedQueryStatus::Ok)
        .map(|diagnostic| {
            format!(
                "{}:{}:{}",
                diagnostic.name, diagnostic.code, diagnostic.message
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

use axon_audit::entry::{compute_diff, AuditAttribution, AuditEntry, FieldDiff, MutationType};
use axon_audit::log::{AuditLog, AuditPage, AuditQuery, MemoryAuditLog};
use axon_core::auth::CallerIdentity;
use axon_core::error::{AxonError, PolicyDenial};
use axon_core::id::{
    CollectionId, EntityId, Namespace, QualifiedCollectionId, DEFAULT_DATABASE, DEFAULT_SCHEMA,
};
use axon_core::types::{Entity, Link};
use axon_schema::gates::evaluate_gates;
use axon_schema::policy::{
    compile_policy_catalog, CompiledCompareOp, CompiledComparison, CompiledFieldAccessPolicy,
    CompiledFieldPolicyRule, CompiledOperationPolicy, CompiledPolicyEnvelope, CompiledPolicyRule,
    CompiledPredicate, PredicateTarget,
};
use axon_schema::schema::{CollectionSchema, CollectionView};
use axon_schema::validation::{compile_entity_schema, validate, validate_link_metadata};
use axon_schema::{
    AccessControlIdentity, AccessControlPolicy, FieldAccessPolicy, IdentityAttributeSource,
    LinkDirection, OperationPolicy, PolicyDecision, PolicyOperation, PolicyPlan, PolicyPredicate,
};
use axon_storage::adapter::{
    extract_index_value, extract_index_values, resolve_field_path, StorageAdapter,
};

use crate::intent::{
    MutationIntent, MutationOperationKind, MutationReviewSummary, PreImageBinding,
};
use crate::policy::{PolicyRequestSnapshot, PolicySubjectSnapshot};
use crate::request::{
    AggregateFunction, AggregateRequest, CountEntitiesRequest, CreateCollectionRequest,
    CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest, CreateNamespaceRequest,
    DeleteCollectionTemplateRequest, DeleteEntityRequest, DeleteLinkRequest,
    DescribeCollectionRequest, DiffSchemaRequest, DropCollectionRequest, DropDatabaseRequest,
    DropNamespaceRequest, ExplainActorOverride, ExplainPolicyRequest, FieldFilter, FilterNode,
    FilterOp, GetCollectionTemplateRequest, GetEntityRequest, GetSchemaRequest,
    ListCollectionsRequest, ListDatabasesRequest, ListNamespaceCollectionsRequest,
    ListNamespacesRequest, PatchEntityRequest, PutCollectionTemplateRequest, PutSchemaRequest,
    QueryAuditRequest, QueryEntitiesRequest, ReachableRequest, RevalidateRequest,
    RevertEntityRequest, RollbackCollectionRequest, RollbackEntityRequest, RollbackEntityTarget,
    RollbackTransactionRequest, SnapshotRequest, SortDirection, TransitionLifecycleRequest,
    TraverseDirection, TraverseRequest, UpdateEntityRequest,
};
use crate::response::{
    AggregateGroup, AggregateResponse, CollectionMetadata, CountEntitiesResponse, CountGroup,
    CreateCollectionResponse, CreateDatabaseResponse, CreateEntityResponse, CreateLinkResponse,
    CreateNamespaceResponse, DeleteCollectionTemplateResponse, DeleteEntityResponse,
    DeleteLinkResponse, DescribeCollectionResponse, DiffSchemaResponse, DropCollectionResponse,
    DropDatabaseResponse, DropNamespaceResponse, EffectivePolicyResponse,
    GetCollectionTemplateResponse, GetEntityMarkdownResponse, GetEntityResponse, GetSchemaResponse,
    InvalidEntity, ListCollectionsResponse, ListDatabasesResponse,
    ListNamespaceCollectionsResponse, ListNamespacesResponse, PatchEntityResponse,
    PolicyApprovalEnvelopeSummary, PolicyExplanationResponse, PolicyQueryPlanDiagnostics,
    PolicyRuleMatch, PutCollectionTemplateResponse, PutSchemaResponse, QueryAuditResponse,
    QueryEntitiesResponse, ReachableResponse, RevalidateResponse, RevertEntityResponse,
    RollbackCollectionEntityResult, RollbackCollectionResponse, RollbackEntityResponse,
    RollbackTransactionEntityResult, RollbackTransactionResponse, SnapshotResponse,
    TransitionLifecycleResponse, TraverseHop, TraversePath, TraverseResponse, UpdateEntityResponse,
};

const DEFAULT_MAX_DEPTH: usize = 3;
const MAX_DEPTH_CAP: usize = 10;
const DEFAULT_MARKDOWN_TEMPLATE_CACHE_CAPACITY: usize = 256;
const POLICY_POST_FILTER_COST_LIMIT: usize = 128;

#[derive(Debug, Clone, Copy)]
enum FieldWriteScope<'a> {
    PresentFields(&'a Value),
    Patch(&'a Value),
}

#[derive(Debug, Clone, Copy)]
struct IntentReadContext<'a> {
    caller: Option<&'a CallerIdentity>,
    attribution: Option<&'a AuditAttribution>,
}

struct IntentEntityRedaction<'a> {
    collection: &'a CollectionId,
    id: &'a EntityId,
    before: Option<&'a Value>,
    after: Option<&'a Value>,
    operation_data: Option<&'a mut Value>,
    diff: Option<&'a mut Value>,
}

#[derive(Debug, Clone)]
struct PolicyWriteCheck<'a> {
    collection: &'a CollectionId,
    entity_id: Option<&'a EntityId>,
    operation: PolicyOperation,
    current_data: Option<&'a Value>,
    candidate_data: &'a Value,
    field_scope: FieldWriteScope<'a>,
    operation_index: Option<usize>,
}

#[derive(Debug, Clone)]
struct PolicyOperationCheck<'a> {
    collection: &'a CollectionId,
    entity_id: Option<&'a EntityId>,
    operation: PolicyOperation,
    data: &'a Value,
    operation_index: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct PolicyPredicateContext<'a> {
    snapshot: &'a PolicyRequestSnapshot,
    operation: &'a PolicyOperation,
    data: &'a Value,
    preview: Option<&'a PreviewedSchemaPlan<'a>>,
}

/// Override pair used by the `putSchema` dry-run fixture path so explain
/// resolution sees the proposed (in-memory) schema and plan for the root
/// collection instead of the active stored ones.
///
/// `plans` carries the full proposed catalog so a self-referential
/// `target_policy` recursion (target collection equals the previewed root)
/// resolves through the proposed plan instead of storage.
///
/// `actor_override` (when present) injects subject bindings into the policy
/// snapshot so the admin UI can preview decisions as a different subject
/// without changing the authenticated caller.
#[derive(Debug, Clone, Copy)]
struct PreviewedSchemaPlan<'a> {
    schema: &'a CollectionSchema,
    plan: &'a PolicyPlan,
    plans: &'a HashMap<String, PolicyPlan>,
    actor_override: Option<&'a ExplainActorOverride>,
}

#[derive(Debug, Clone, Default)]
struct PolicyStoragePlan {
    candidate_ids: Option<Vec<EntityId>>,
    post_filter: bool,
    missing_index: Option<String>,
    storage_filters: Vec<String>,
    explain: Vec<String>,
}

impl PolicyStoragePlan {
    fn diagnostics(&self) -> PolicyQueryPlanDiagnostics {
        PolicyQueryPlanDiagnostics {
            operation: PolicyOperation::Read.as_str().to_string(),
            storage_filters: self.storage_filters.clone(),
            post_filter: self.post_filter,
            missing_index: self.missing_index.clone(),
            explain: self.explain.clone(),
        }
    }
}

#[derive(Debug, Clone)]
enum PolicyRuleCandidatePlan {
    All,
    Indexed {
        entity_ids: Vec<EntityId>,
        storage_filters: Vec<String>,
    },
    Unindexed {
        missing_index: Option<String>,
    },
}

#[derive(Debug, Clone, Copy)]
struct TransitionPolicyCheck<'a> {
    collection: &'a CollectionId,
    entity_id: &'a EntityId,
    lifecycle_field: &'a str,
    target_state: &'a str,
    data: &'a Value,
}

#[derive(Debug)]
struct CachedMarkdownTemplate {
    version: u32,
    template: Arc<axon_render::CompiledTemplate>,
}

#[derive(Debug)]
struct MarkdownTemplateCache {
    entries: HashMap<CollectionId, CachedMarkdownTemplate>,
    lru: VecDeque<CollectionId>,
    capacity: usize,
}

impl Default for MarkdownTemplateCache {
    fn default() -> Self {
        Self::new(DEFAULT_MARKDOWN_TEMPLATE_CACHE_CAPACITY)
    }
}

impl MarkdownTemplateCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            capacity,
        }
    }

    fn get(
        &mut self,
        collection: &CollectionId,
        version: u32,
    ) -> Option<Arc<axon_render::CompiledTemplate>> {
        let template = match self.entries.get(collection) {
            Some(cached) if cached.version == version => Arc::clone(&cached.template),
            _ => return None,
        };

        self.touch(collection);
        Some(template)
    }

    fn insert(&mut self, collection: CollectionId, cached: CachedMarkdownTemplate) {
        if self.capacity == 0 {
            return;
        }

        self.entries.insert(collection.clone(), cached);
        self.touch(&collection);
        self.evict_to_capacity();
    }

    fn remove(&mut self, collection: &CollectionId) -> Option<CachedMarkdownTemplate> {
        let removed = self.entries.remove(collection);
        if removed.is_some() {
            self.lru.retain(|candidate| candidate != collection);
        }
        removed
    }

    fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&CollectionId, &CachedMarkdownTemplate) -> bool,
    {
        self.entries
            .retain(|collection, cached| keep(collection, cached));
        self.lru
            .retain(|collection| self.entries.contains_key(collection));
    }

    fn touch(&mut self, collection: &CollectionId) {
        self.lru.retain(|candidate| candidate != collection);
        self.lru.push_back(collection.clone());
    }

    fn evict_to_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            match self.lru.pop_front() {
                Some(collection) => {
                    self.entries.remove(&collection);
                }
                None => break,
            }
        }
    }
}

/// Core API handler: coordinates storage, schema validation, and audit.
///
/// Schemas and collection registrations are persisted via the `StorageAdapter`;
/// there is no separate in-memory state. Swap `S` for any [`StorageAdapter`]
/// implementation.
pub struct AxonHandler<S: StorageAdapter> {
    storage: S,
    audit: MemoryAuditLog,
    markdown_template_cache: Mutex<MarkdownTemplateCache>,
}

impl<S: StorageAdapter> AxonHandler<S> {
    fn present_entity(requested: &CollectionId, mut entity: Entity) -> Entity {
        entity.collection = requested.clone();
        entity
    }

    fn present_entities(requested: &CollectionId, entities: Vec<Entity>) -> Vec<Entity> {
        entities
            .into_iter()
            .map(|entity| Self::present_entity(requested, entity))
            .collect()
    }

    fn present_schema(requested: &CollectionId, mut schema: CollectionSchema) -> CollectionSchema {
        schema.collection = requested.clone();
        schema
    }

    fn present_collection_view(
        requested: &CollectionId,
        mut view: CollectionView,
    ) -> CollectionView {
        view.collection = requested.clone();
        view
    }

    fn collection_view_audit_state(
        requested: &CollectionId,
        view: CollectionView,
    ) -> Result<Value, AxonError> {
        Ok(serde_json::to_value(Self::present_collection_view(
            requested, view,
        ))?)
    }

    pub fn new(storage: S) -> Self {
        Self::new_with_markdown_template_cache_capacity(
            storage,
            DEFAULT_MARKDOWN_TEMPLATE_CACHE_CAPACITY,
        )
    }

    fn new_with_markdown_template_cache_capacity(storage: S, cache_capacity: usize) -> Self {
        Self {
            storage,
            audit: MemoryAuditLog::default(),
            markdown_template_cache: Mutex::new(MarkdownTemplateCache::new(cache_capacity)),
        }
    }

    fn markdown_template_cache(&self) -> Result<MutexGuard<'_, MarkdownTemplateCache>, AxonError> {
        self.markdown_template_cache
            .lock()
            .map_err(|_| AxonError::InvalidOperation("markdown template cache is poisoned".into()))
    }

    fn compiled_markdown_template(
        &self,
        collection: &CollectionId,
        view: &axon_schema::schema::CollectionView,
    ) -> Result<Arc<axon_render::CompiledTemplate>, AxonError> {
        let mut cache = self.markdown_template_cache()?;

        if let Some(cached) = cache.get(collection, view.version) {
            return Ok(cached);
        }

        let compiled = Arc::new(axon_render::compile(view.markdown_template.clone())?);
        cache.insert(
            collection.clone(),
            CachedMarkdownTemplate {
                version: view.version,
                template: Arc::clone(&compiled),
            },
        );
        Ok(compiled)
    }

    fn invalidate_markdown_template(&self, collection: &CollectionId) -> Result<(), AxonError> {
        self.markdown_template_cache()?.remove(collection);
        Ok(())
    }

    fn invalidate_markdown_templates_for_collections(
        &self,
        collections: &[QualifiedCollectionId],
    ) -> Result<(), AxonError> {
        if collections.is_empty() {
            return Ok(());
        }

        let doomed: HashSet<_> = collections.iter().cloned().collect();
        let doomed_bare_names: HashSet<_> = collections
            .iter()
            .map(|collection| collection.collection.clone())
            .collect();
        self.markdown_template_cache()?.retain(|collection, _| {
            let (namespace, bare_collection) = Namespace::parse(collection.as_str());
            if bare_collection == collection.as_str() {
                !doomed_bare_names.contains(collection)
            } else {
                let key = QualifiedCollectionId::from_parts(
                    &namespace,
                    &CollectionId::new(bare_collection),
                );
                !doomed.contains(&key)
            }
        });
        Ok(())
    }

    /// Persist a schema for a collection via the storage adapter.
    ///
    /// Validates the `entity_schema` (if present) before persisting.
    /// Subsequent creates and updates for that collection are validated
    /// against this schema. Replaces any previously stored schema.
    pub fn put_schema(&mut self, schema: CollectionSchema) -> Result<(), AxonError> {
        if let Some(entity_schema) = &schema.entity_schema {
            compile_entity_schema(entity_schema)?;
        }

        // Validate index declarations (FEAT-013).
        for idx in &schema.indexes {
            if idx.field.is_empty() {
                return Err(AxonError::SchemaValidation(
                    "index declaration has an empty field path".into(),
                ));
            }
        }

        // Validate rule definitions (US-069).
        if !schema.validation_rules.is_empty() {
            let rule_errors = axon_schema::rules::validate_rule_definitions(
                &schema.validation_rules,
                schema.entity_schema.as_ref(),
            );
            if !rule_errors.is_empty() {
                let msgs: Vec<String> = rule_errors.iter().map(|e| e.to_string()).collect();
                return Err(AxonError::SchemaValidation(format!(
                    "invalid validation rules: {}",
                    msgs.join("; ")
                )));
            }
        }

        let compile_report = self.compile_report_for_schema(&schema)?;
        if !compile_report.is_success() {
            return Err(AxonError::SchemaValidation(format!(
                "query_compile_failed: {}",
                query_compile_summary(&compile_report)
            )));
        }

        self.storage.put_schema(&schema)
    }

    /// Retrieve the persisted schema for a collection, if one exists.
    pub fn get_schema(
        &self,
        collection: &CollectionId,
    ) -> Result<Option<CollectionSchema>, AxonError> {
        self.storage.get_schema(collection)
    }

    /// Returns a reference to the internal audit log (useful in tests).
    pub fn audit_log(&self) -> &MemoryAuditLog {
        &self.audit
    }

    /// Mutable reference to the internal audit log (used by transaction tests).
    pub fn audit_log_mut(&mut self) -> &mut MemoryAuditLog {
        &mut self.audit
    }

    /// Read-only access to the underlying storage adapter.
    pub fn storage_ref(&self) -> &S {
        &self.storage
    }

    /// Mutable access to the underlying storage adapter (used by simulation framework).
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// Mutable access to both storage and audit log for transaction commit.
    pub fn storage_and_audit_mut(&mut self) -> (&mut S, &mut MemoryAuditLog) {
        (&mut self.storage, &mut self.audit)
    }

    fn policy_snapshot_for_request(
        &self,
        collection: &CollectionId,
        schema: Option<&CollectionSchema>,
        actor: Option<&str>,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<Option<PolicyRequestSnapshot>, AxonError> {
        let Some(schema) = schema else {
            return Ok(None);
        };
        let Some(policy) = &schema.access_control else {
            return Ok(None);
        };

        let qualified = self.storage.resolve_collection_key(collection)?;
        let database_id = qualified.namespace.database.clone();
        let namespace = qualified.namespace.to_string();
        let tenant_id = attribution.map(|attr| attr.tenant_id.clone());
        let subject = self.policy_subject_for_request(
            policy.identity.as_ref(),
            actor,
            caller,
            attribution,
            &database_id,
        )?;

        Ok(Some(PolicyRequestSnapshot {
            collection: collection.clone(),
            namespace,
            database_id,
            tenant_id,
            schema_version: Some(schema.version),
            policy_version: Some(schema.version),
            subject,
        }))
    }

    fn compile_policy_plan_for_schema(
        &self,
        schema: &CollectionSchema,
    ) -> Result<Option<PolicyPlan>, AxonError> {
        let schemas = self.policy_catalog_schemas(schema)?;
        let mut catalog = compile_policy_catalog(&schemas)?;
        Ok(catalog.plans.remove(schema.collection.as_str()))
    }

    pub fn policy_catalog_schemas(
        &self,
        root: &CollectionSchema,
    ) -> Result<Vec<CollectionSchema>, AxonError> {
        let mut schemas = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([root.clone()]);

        while let Some(schema) = queue.pop_front() {
            if !seen.insert(schema.collection.to_string()) {
                continue;
            }

            for target in policy_related_target_collections(schema.access_control.as_ref()) {
                if seen.contains(&target) {
                    continue;
                }
                if let Some(target_schema) = self.storage.get_schema(&CollectionId::new(&target))? {
                    queue.push_back(target_schema);
                }
            }

            schemas.push(schema);
        }

        Ok(schemas)
    }

    /// Build the policy compile report for a proposed schema.
    ///
    /// Storage / IO errors bubble as `AxonError`. A failed
    /// [`PolicyCompileError`] is captured into
    /// `PolicyCompileReport::errors` so the admin-UI dry-run path can
    /// surface a structured diagnostic instead of bubbling the error.
    fn policy_compile_report_for_schema(
        &self,
        schema: &CollectionSchema,
    ) -> Result<axon_schema::PolicyCompileReport, AxonError> {
        let schemas = self.policy_catalog_schemas(schema)?;
        match compile_policy_catalog(&schemas) {
            Ok(catalog) => Ok(catalog.report),
            Err(err) => Ok(axon_schema::PolicyCompileReport::from_compile_error(&err)),
        }
    }

    fn compile_report_for_schema(
        &self,
        schema: &CollectionSchema,
    ) -> Result<axon_schema::CompileReport, AxonError> {
        if schema.queries.is_empty() {
            return Ok(axon_schema::CompileReport::default());
        }

        let mut schemas = Vec::new();
        let mut seen = HashSet::from([schema.collection.to_string()]);

        for link_type in schema.link_types.values() {
            if !seen.insert(link_type.target_collection.clone()) {
                continue;
            }
            if let Some(active) = self
                .storage
                .get_schema(&CollectionId::new(&link_type.target_collection))?
            {
                schemas.push(active);
            }
        }

        let mut estimated_counts = HashMap::new();
        for active in schemas.iter().chain(std::iter::once(schema)) {
            let count = self.storage.count(&active.collection).unwrap_or(0) as u64;
            estimated_counts.insert(active.collection.to_string(), count);
        }

        Ok(axon_schema::compile_named_queries(
            schema,
            &schemas,
            &estimated_counts,
        ))
    }

    fn enforce_write_policy(
        &self,
        schema: Option<&CollectionSchema>,
        snapshot: Option<&PolicyRequestSnapshot>,
        check: PolicyWriteCheck<'_>,
    ) -> Result<(), AxonError> {
        let Some(schema) = schema else {
            return Ok(());
        };
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let Some(plan) = self.compile_policy_plan_for_schema(schema)? else {
            return Ok(());
        };

        if let Some(current_data) = check.current_data {
            self.enforce_policy_operation(
                &plan,
                snapshot,
                PolicyOperationCheck {
                    collection: check.collection,
                    entity_id: check.entity_id,
                    operation: PolicyOperation::Read,
                    data: current_data,
                    operation_index: check.operation_index,
                },
            )?;
        }

        self.enforce_policy_operation(
            &plan,
            snapshot,
            PolicyOperationCheck {
                collection: check.collection,
                entity_id: check.entity_id,
                operation: check.operation.clone(),
                data: check.candidate_data,
                operation_index: check.operation_index,
            },
        )?;
        self.enforce_field_write_policy(&plan, snapshot, check.clone())?;
        self.enforce_policy_envelopes(&plan, snapshot, check)?;

        Ok(())
    }

    fn enforce_policy_operation(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyOperationCheck<'_>,
    ) -> Result<(), AxonError> {
        let denial_label =
            self.policy_operation_denial_label(plan, snapshot, check.clone(), None)?;
        let Some(denial_label) = denial_label else {
            return Ok(());
        };

        Err(policy_forbidden(
            "row_write_denied",
            check.collection,
            check.entity_id,
            None,
            Some(denial_label),
            check.operation_index,
        ))
    }

    fn policy_operation_allows(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyOperationCheck<'_>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<bool, AxonError> {
        Ok(self
            .policy_operation_denial_label(plan, snapshot, check, preview)?
            .is_none())
    }

    fn policy_operation_denial_label(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyOperationCheck<'_>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Option<String>, AxonError> {
        let mut allow_rules_present = false;
        let mut allow_matched = false;

        for policy in applicable_operation_policies(plan, &check.operation) {
            for rule in &policy.deny {
                if self.policy_rule_matches(
                    rule,
                    snapshot,
                    &check.operation,
                    check.data,
                    preview,
                )? {
                    return Ok(Some(policy_rule_label(rule)));
                }
            }

            if !policy.allow.is_empty() {
                allow_rules_present = true;
            }
            for rule in &policy.allow {
                if self.policy_rule_matches(
                    rule,
                    snapshot,
                    &check.operation,
                    check.data,
                    preview,
                )? {
                    allow_matched = true;
                }
            }
        }

        if allow_rules_present && !allow_matched {
            return Ok(Some(check.operation.as_str().to_string()));
        }

        Ok(None)
    }

    fn enforce_field_write_policy(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyWriteCheck<'_>,
    ) -> Result<(), AxonError> {
        for (field_path, field_policy) in &plan.fields {
            let Some(write_policy) = field_policy.write.as_ref() else {
                continue;
            };
            if !field_write_scope_touches_path(check.field_scope, field_path) {
                continue;
            }

            for rule in &write_policy.deny {
                if self.field_policy_rule_matches(
                    rule,
                    snapshot,
                    &check.operation,
                    check.candidate_data,
                    None,
                )? {
                    return Err(policy_forbidden(
                        "field_write_denied",
                        check.collection,
                        check.entity_id,
                        Some(field_path),
                        Some(field_policy_rule_label(rule)),
                        check.operation_index,
                    ));
                }
            }

            if !write_policy.allow.is_empty() {
                let mut matched = false;
                for rule in &write_policy.allow {
                    if self.field_policy_rule_matches(
                        rule,
                        snapshot,
                        &check.operation,
                        check.candidate_data,
                        None,
                    )? {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return Err(policy_forbidden(
                        "field_write_denied",
                        check.collection,
                        check.entity_id,
                        Some(field_path),
                        write_policy.allow.first().map(field_policy_rule_label),
                        check.operation_index,
                    ));
                }
            }
        }

        Ok(())
    }

    fn enforce_policy_envelopes(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyWriteCheck<'_>,
    ) -> Result<(), AxonError> {
        let mut needs_approval: Option<String> = None;

        for envelope in applicable_policy_envelopes(plan, &check.operation) {
            if self.policy_predicate_matches(
                envelope.when.as_ref(),
                PolicyPredicateContext {
                    snapshot,
                    operation: &check.operation,
                    data: check.candidate_data,
                    preview: None,
                },
            )? {
                match envelope.decision {
                    PolicyDecision::Deny => {
                        return Err(policy_forbidden(
                            "row_write_denied",
                            check.collection,
                            check.entity_id,
                            None,
                            Some(policy_envelope_label(envelope)),
                            check.operation_index,
                        ))
                    }
                    PolicyDecision::NeedsApproval => {
                        needs_approval = Some(policy_envelope_label(envelope));
                    }
                    PolicyDecision::Allow => {}
                }
            }
        }

        if let Some(policy) = needs_approval {
            return Err(policy_forbidden(
                "needs_approval",
                check.collection,
                check.entity_id,
                None,
                Some(policy),
                check.operation_index,
            ));
        }

        Ok(())
    }

    fn enforce_transition_policy(
        &self,
        schema: &CollectionSchema,
        snapshot: Option<&PolicyRequestSnapshot>,
        check: TransitionPolicyCheck<'_>,
    ) -> Result<(), AxonError> {
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let Some(plan) = self.compile_policy_plan_for_schema(schema)? else {
            return Ok(());
        };
        let Some(transitions) = plan.transitions.get(check.lifecycle_field) else {
            return Ok(());
        };
        let Some(policy) = transitions.get(check.target_state) else {
            return Ok(());
        };

        for rule in &policy.deny {
            if self.policy_rule_matches(
                rule,
                snapshot,
                &PolicyOperation::Update,
                check.data,
                None,
            )? {
                return Err(policy_forbidden(
                    "row_write_denied",
                    check.collection,
                    Some(check.entity_id),
                    Some(check.lifecycle_field),
                    Some(policy_rule_label(rule)),
                    None,
                ));
            }
        }

        if !policy.allow.is_empty() {
            let mut matched = false;
            for rule in &policy.allow {
                if self.policy_rule_matches(
                    rule,
                    snapshot,
                    &PolicyOperation::Update,
                    check.data,
                    None,
                )? {
                    matched = true;
                    break;
                }
            }
            if !matched {
                return Err(policy_forbidden(
                    "row_write_denied",
                    check.collection,
                    Some(check.entity_id),
                    Some(check.lifecycle_field),
                    Some(check.target_state.to_string()),
                    None,
                ));
            }
        }

        Ok(())
    }

    fn policy_rule_matches(
        &self,
        rule: &CompiledPolicyRule,
        snapshot: &PolicyRequestSnapshot,
        operation: &PolicyOperation,
        data: &Value,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<bool, AxonError> {
        let ctx = PolicyPredicateContext {
            snapshot,
            operation,
            data,
            preview,
        };
        Ok(self.policy_predicate_matches(rule.when.as_ref(), ctx)?
            && self.policy_predicate_matches(rule.where_clause.as_ref(), ctx)?)
    }

    fn field_policy_rule_matches(
        &self,
        rule: &CompiledFieldPolicyRule,
        snapshot: &PolicyRequestSnapshot,
        operation: &PolicyOperation,
        data: &Value,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<bool, AxonError> {
        let ctx = PolicyPredicateContext {
            snapshot,
            operation,
            data,
            preview,
        };
        Ok(self.policy_predicate_matches(rule.when.as_ref(), ctx)?
            && self.policy_predicate_matches(rule.where_clause.as_ref(), ctx)?)
    }

    fn policy_predicate_matches(
        &self,
        predicate: Option<&CompiledPredicate>,
        ctx: PolicyPredicateContext<'_>,
    ) -> Result<bool, AxonError> {
        let Some(predicate) = predicate else {
            return Ok(true);
        };

        match predicate {
            CompiledPredicate::All(predicates) => {
                for predicate in predicates {
                    if !self.policy_predicate_matches(Some(predicate), ctx)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            CompiledPredicate::Any(predicates) => {
                for predicate in predicates {
                    if self.policy_predicate_matches(Some(predicate), ctx)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            CompiledPredicate::Not(predicate) => {
                Ok(!self.policy_predicate_matches(Some(predicate), ctx)?)
            }
            CompiledPredicate::Compare(comparison) => {
                Ok(policy_comparison_matches(comparison, ctx))
            }
            CompiledPredicate::Operation(operation) => Ok(operation == ctx.operation),
            CompiledPredicate::Related(related) => self.policy_related_matches(related, ctx),
            CompiledPredicate::SharesRelation(_) => Ok(false),
        }
    }

    fn policy_related_matches(
        &self,
        related: &axon_schema::CompiledRelationshipPredicate,
        ctx: PolicyPredicateContext<'_>,
    ) -> Result<bool, AxonError> {
        let source_collection = &ctx.snapshot.collection;
        let Some(entity_id) = ctx
            .data
            .get("_id")
            .and_then(Value::as_str)
            .map(EntityId::new)
        else {
            return Ok(false);
        };

        let links = self
            .storage
            .range_scan(&Link::links_collection(), None, None, None)?;
        for link in links.iter().filter_map(Link::from_entity) {
            let (matched, target_collection, target_id) = match related.direction {
                LinkDirection::Outgoing => (
                    &link.source_collection == source_collection
                        && link.source_id == entity_id
                        && link.link_type == related.link_type
                        && link.target_collection.as_str() == related.target_collection,
                    &link.target_collection,
                    &link.target_id,
                ),
                LinkDirection::Incoming => (
                    &link.target_collection == source_collection
                        && link.target_id == entity_id
                        && link.link_type == related.link_type
                        && link.source_collection.as_str() == related.target_collection,
                    &link.source_collection,
                    &link.source_id,
                ),
            };
            if !matched {
                continue;
            }
            if let Some(operation) = &related.target_policy {
                let Some(target) = self.storage.get(target_collection, target_id)? else {
                    continue;
                };
                let Some((target_plan, _)) =
                    self.resolve_target_policy_plan(target_collection, ctx.preview)?
                else {
                    continue;
                };
                let mut target_snapshot = ctx.snapshot.clone();
                target_snapshot.collection = target_collection.clone();
                let target_data = entity_policy_data(&target);
                if !self.policy_operation_allows(
                    &target_plan,
                    &target_snapshot,
                    PolicyOperationCheck {
                        collection: target_collection,
                        entity_id: Some(target_id),
                        operation: operation.clone(),
                        data: &target_data,
                        operation_index: None,
                    },
                    ctx.preview,
                )? {
                    continue;
                }
            }
            return Ok(true);
        }

        Ok(false)
    }

    /// Resolve the `(plan, schema)` pair that governs `target_collection` for
    /// a `target_policy` recursion. When a preview catalog is provided and
    /// contains a plan for `target_collection`, the proposed plan wins so
    /// self-referential `target_policy` recursion (target == previewed root)
    /// uses the proposed plan instead of the active stored one.
    fn resolve_target_policy_plan(
        &self,
        target_collection: &CollectionId,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Option<(PolicyPlan, Option<CollectionSchema>)>, AxonError> {
        if let Some(p) = preview {
            if let Some(plan) = p.plans.get(target_collection.as_str()).cloned() {
                let schema = if &p.schema.collection == target_collection {
                    Some(p.schema.clone())
                } else {
                    self.storage.get_schema(target_collection)?
                };
                return Ok(Some((plan, schema)));
            }
        }
        let Some(target_schema) = self.storage.get_schema(target_collection)? else {
            return Ok(None);
        };
        let Some(target_plan) = self.compile_policy_plan_for_schema(&target_schema)? else {
            return Ok(None);
        };
        Ok(Some((target_plan, Some(target_schema))))
    }

    fn read_policy_allows_entity(
        &self,
        collection: &CollectionId,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        entity: &Entity,
    ) -> Result<bool, AxonError> {
        let data = entity_policy_data(entity);
        self.policy_operation_allows(
            plan,
            snapshot,
            PolicyOperationCheck {
                collection,
                entity_id: Some(&entity.id),
                operation: PolicyOperation::Read,
                data: &data,
                operation_index: None,
            },
            None,
        )
    }

    fn field_read_redactions(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        data: &Value,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Vec<(String, Value)>, AxonError> {
        let mut redactions = Vec::new();

        for (field_path, field_policy) in &plan.fields {
            let Some(read_policy) = field_policy.read.as_ref() else {
                continue;
            };

            let mut redaction = None;
            for rule in &read_policy.deny {
                if self.field_policy_rule_matches(
                    rule,
                    snapshot,
                    &PolicyOperation::Read,
                    data,
                    preview,
                )? {
                    redaction = Some(rule.redact_as.clone().unwrap_or(Value::Null));
                    break;
                }
            }

            if redaction.is_none() && !read_policy.allow.is_empty() {
                let mut matched = false;
                for rule in &read_policy.allow {
                    if self.field_policy_rule_matches(
                        rule,
                        snapshot,
                        &PolicyOperation::Read,
                        data,
                        preview,
                    )? {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    redaction = Some(Value::Null);
                }
            }

            if let Some(redaction) = redaction {
                redactions.push((field_path.clone(), redaction));
            }
        }

        Ok(redactions)
    }

    fn redact_entity_fields_for_read(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        entity: &mut Entity,
    ) -> Result<(), AxonError> {
        let policy_data = entity_policy_data(entity);
        let redactions = self.field_read_redactions(plan, snapshot, &policy_data, None)?;
        apply_field_redactions(&mut entity.data, &redactions);
        Ok(())
    }

    fn redact_entity_for_read_with_context(
        &self,
        collection: &CollectionId,
        mut entity: Entity,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<Entity, AxonError> {
        let schema = self.storage.get_schema(collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            collection,
            schema.as_ref(),
            None,
            caller,
            attribution,
        )?;
        let compiled_policy = match schema.as_ref() {
            Some(schema) if policy_snapshot.is_some() => {
                self.compile_policy_plan_for_schema(schema)?
            }
            _ => None,
        };

        if let (Some(plan), Some(snapshot)) = (&compiled_policy, &policy_snapshot) {
            self.redact_entity_fields_for_read(plan, snapshot, &mut entity)?;
        }

        Ok(entity)
    }

    fn entity_visible_for_read_with_context(
        &self,
        collection: &CollectionId,
        entity: &Entity,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<bool, AxonError> {
        let schema = self.storage.get_schema(collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            collection,
            schema.as_ref(),
            None,
            caller,
            attribution,
        )?;
        let compiled_policy = match schema.as_ref() {
            Some(schema) if policy_snapshot.is_some() => {
                self.compile_policy_plan_for_schema(schema)?
            }
            _ => None,
        };

        match (&compiled_policy, &policy_snapshot) {
            (Some(plan), Some(snapshot)) => {
                self.read_policy_allows_entity(collection, plan, snapshot, entity)
            }
            _ => Ok(true),
        }
    }

    pub fn effective_policy_with_caller(
        &self,
        collection: CollectionId,
        entity_id: Option<EntityId>,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<EffectivePolicyResponse, AxonError> {
        self.effective_policy_inner(collection, entity_id, caller, attribution.as_ref(), None)
    }

    /// Resolve effective capabilities against an in-memory policy plan instead
    /// of the active stored access_control for `schema.collection`.
    #[allow(clippy::too_many_arguments)]
    pub fn effective_policy_with_plan(
        &self,
        collection: CollectionId,
        entity_id: Option<EntityId>,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        schema: &CollectionSchema,
        plan: &PolicyPlan,
        plans: &HashMap<String, PolicyPlan>,
    ) -> Result<EffectivePolicyResponse, AxonError> {
        let preview = PreviewedSchemaPlan {
            schema,
            plan,
            plans,
            actor_override: None,
        };
        self.effective_policy_inner(collection, entity_id, caller, attribution, Some(&preview))
    }

    fn effective_policy_inner(
        &self,
        collection: CollectionId,
        entity_id: Option<EntityId>,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<EffectivePolicyResponse, AxonError> {
        let Some(schema) = self.storage.get_schema(&collection)? else {
            return Err(AxonError::NotFound(collection.to_string()));
        };
        let policy_version = schema.version;
        let Some(snapshot) = self.policy_snapshot_for_request(
            &collection,
            preview.map_or(Some(&schema), |p| Some(p.schema)),
            None,
            Some(caller),
            attribution,
        )?
        else {
            return Ok(EffectivePolicyResponse {
                collection: collection.to_string(),
                can_read: true,
                can_create: true,
                can_update: true,
                can_delete: true,
                redacted_fields: Vec::new(),
                denied_fields: Vec::new(),
                policy_version,
            });
        };
        let plan = if let Some(p) = preview {
            Some(p.plan.clone())
        } else {
            self.compile_policy_plan_for_schema(&schema)?
        };
        let Some(plan) = plan else {
            return Ok(EffectivePolicyResponse {
                collection: collection.to_string(),
                can_read: true,
                can_create: true,
                can_update: true,
                can_delete: true,
                redacted_fields: Vec::new(),
                denied_fields: Vec::new(),
                policy_version,
            });
        };

        let entity = match entity_id {
            Some(id) => Some(
                self.storage
                    .get(&collection, &id)?
                    .ok_or_else(|| AxonError::NotFound(id.to_string()))?,
            ),
            None => None,
        };
        let entity_data = entity.as_ref().map(entity_policy_data);
        let can_read = match entity_data.as_ref() {
            Some(data) => self.policy_operation_allows(
                &plan,
                &snapshot,
                PolicyOperationCheck {
                    collection: &collection,
                    entity_id: entity.as_ref().map(|entity| &entity.id),
                    operation: PolicyOperation::Read,
                    data,
                    operation_index: None,
                },
                preview,
            )?,
            None => effective_operation_allows_static(&plan, &snapshot, PolicyOperation::Read),
        };
        let can_create = self.effective_operation_allows(
            &collection,
            &plan,
            &snapshot,
            PolicyOperation::Create,
            entity_data.as_ref(),
            preview,
        )?;
        let can_update = can_read
            && self.effective_operation_allows(
                &collection,
                &plan,
                &snapshot,
                PolicyOperation::Update,
                entity_data.as_ref(),
                preview,
            )?;
        let can_delete = can_read
            && self.effective_operation_allows(
                &collection,
                &plan,
                &snapshot,
                PolicyOperation::Delete,
                entity_data.as_ref(),
                preview,
            )?;
        let redacted_fields =
            self.effective_redacted_fields(&plan, &snapshot, entity_data.as_ref(), preview)?;
        let denied_fields =
            self.effective_denied_fields(&plan, &snapshot, entity_data.as_ref(), preview)?;

        Ok(EffectivePolicyResponse {
            collection: collection.to_string(),
            can_read,
            can_create,
            can_update,
            can_delete,
            redacted_fields,
            denied_fields,
            policy_version,
        })
    }

    /// Resolve the current policy subject for a caller against a collection.
    ///
    /// Mutation-intent review authorization uses this to verify approver roles
    /// from the same request-time identity mappings that FEAT-029 policies use.
    pub fn policy_subject_snapshot_with_caller(
        &self,
        collection: &CollectionId,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<Option<PolicySubjectSnapshot>, AxonError> {
        let Some(schema) = self.storage.get_schema(collection)? else {
            return Ok(None);
        };
        let Some(snapshot) = self.policy_snapshot_for_request(
            collection,
            Some(&schema),
            None,
            Some(caller),
            attribution.as_ref(),
        )?
        else {
            return Ok(None);
        };
        Ok(Some(snapshot.subject))
    }

    fn effective_operation_allows(
        &self,
        collection: &CollectionId,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        operation: PolicyOperation,
        data: Option<&Value>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<bool, AxonError> {
        match data {
            Some(data) => self.policy_operation_allows(
                plan,
                snapshot,
                PolicyOperationCheck {
                    collection,
                    entity_id: None,
                    operation,
                    data,
                    operation_index: None,
                },
                preview,
            ),
            None => Ok(effective_operation_allows_static(plan, snapshot, operation)),
        }
    }

    fn effective_redacted_fields(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        data: Option<&Value>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Vec<String>, AxonError> {
        if let Some(data) = data {
            let mut fields: Vec<String> = self
                .field_read_redactions(plan, snapshot, data, preview)?
                .into_iter()
                .map(|(field, _)| field)
                .collect();
            fields.sort();
            fields.dedup();
            return Ok(fields);
        }

        Ok(plan
            .fields
            .iter()
            .filter_map(|(field_path, field_policy)| {
                field_policy.read.as_ref().and_then(|read_policy| {
                    effective_field_read_redacted_static(read_policy, snapshot)
                        .then(|| field_path.clone())
                })
            })
            .collect())
    }

    fn effective_denied_fields(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        data: Option<&Value>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Vec<String>, AxonError> {
        let mut fields = BTreeSet::new();
        for (field_path, field_policy) in &plan.fields {
            let Some(write_policy) = field_policy.write.as_ref() else {
                continue;
            };
            for operation in [
                PolicyOperation::Create,
                PolicyOperation::Update,
                PolicyOperation::Delete,
            ] {
                let denied = match data {
                    Some(data) => effective_field_write_denied_for_data(
                        self,
                        write_policy,
                        snapshot,
                        &operation,
                        data,
                        preview,
                    )?,
                    None => effective_field_write_denied_static(write_policy, snapshot, &operation),
                };
                if denied {
                    fields.insert(field_path.clone());
                    break;
                }
            }
        }
        Ok(fields.into_iter().collect())
    }

    pub fn explain_policy_with_caller(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        self.explain_policy_inner(req, caller, attribution.as_ref(), None, None)
    }

    /// Run an explain against a proposed (in-memory) schema and catalog plans
    /// instead of the active stored ones.
    ///
    /// Used by the `putSchema` dry-run fixture path so the admin UI can
    /// preview decisions for the policy version that *would* be activated.
    /// `plans` carries the full proposed catalog so a self-referential
    /// `target_policy` recursion (target collection equals the previewed root)
    /// resolves through the proposed plan instead of the active stored one.
    #[allow(clippy::too_many_arguments)]
    pub fn explain_policy_with_plan(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        schema: &CollectionSchema,
        plan: &PolicyPlan,
        plans: &HashMap<String, PolicyPlan>,
        actor_override: Option<&ExplainActorOverride>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let preview = PreviewedSchemaPlan {
            schema,
            plan,
            plans,
            actor_override,
        };
        self.explain_policy_inner(req, caller, attribution, None, Some(&preview))
    }

    fn explain_policy_inner(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let operation = req.operation.trim().to_ascii_lowercase();
        match operation.as_str() {
            "read" => self.explain_read_policy(req, caller, attribution, operation_index, preview),
            "create" => {
                self.explain_create_policy(req, caller, attribution, operation_index, preview)
            }
            "update" => {
                self.explain_update_policy(req, caller, attribution, operation_index, preview)
            }
            "patch" => {
                self.explain_patch_policy(req, caller, attribution, operation_index, preview)
            }
            "delete" => {
                self.explain_delete_policy(req, caller, attribution, operation_index, preview)
            }
            "transition" => {
                self.explain_transition_policy(req, caller, attribution, operation_index, preview)
            }
            "rollback" => {
                self.explain_rollback_policy(req, caller, attribution, operation_index, preview)
            }
            "transaction" => {
                self.explain_transaction_policy(req, caller, attribution, operation_index, preview)
            }
            "create_link" | "delete_link" => Ok(policy_explanation(
                operation,
                None,
                None,
                operation_index,
                "allow",
                "link_policy_not_configured",
                0,
            )),
            other => Err(AxonError::InvalidArgument(format!(
                "unsupported policy explanation operation '{other}'"
            ))),
        }
    }

    fn explain_policy_plan_for_request(
        &self,
        collection: &CollectionId,
        schema: &CollectionSchema,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Option<(PolicyRequestSnapshot, PolicyPlan)>, AxonError> {
        let Some(mut snapshot) = self.policy_snapshot_for_request(
            collection,
            Some(schema),
            None,
            Some(caller),
            attribution,
        )?
        else {
            return Ok(None);
        };
        if let Some(p) = preview {
            if let Some(override_) = p.actor_override {
                apply_actor_override_subject(&mut snapshot, override_);
            }
        }
        // Use the proposed plan when its collection matches the request.
        // Self-referential `target_policy` recursion picks up the proposed
        // plan via `PreviewedSchemaPlan::plans` (see `policy_related_matches`).
        if let Some(p) = preview {
            if &p.schema.collection == collection {
                return Ok(Some((snapshot, p.plan.clone())));
            }
        }
        let Some(plan) = self.compile_policy_plan_for_schema(schema)? else {
            return Ok(None);
        };
        Ok(Some((snapshot, plan)))
    }

    fn explain_collection_schema(
        &self,
        req: &ExplainPolicyRequest,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<(CollectionId, CollectionSchema), AxonError> {
        let collection = req
            .collection
            .clone()
            .ok_or_else(|| AxonError::InvalidArgument("collection is required".into()))?;
        if let Some(p) = preview {
            if p.schema.collection == collection {
                return Ok((collection, p.schema.clone()));
            }
        }
        let schema = self
            .storage
            .get_schema(&collection)?
            .ok_or_else(|| AxonError::NotFound(collection.to_string()))?;
        Ok((collection, schema))
    }

    fn explain_read_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let (collection, schema) = self.explain_collection_schema(&req, preview)?;
        let policy_version = schema.version;
        let Some((snapshot, plan)) = self.explain_policy_plan_for_request(
            &collection,
            &schema,
            caller,
            attribution,
            preview,
        )?
        else {
            return Ok(policy_explanation(
                "read",
                Some(&collection),
                req.entity_id.as_ref(),
                operation_index,
                "allow",
                "no_policy",
                policy_version,
            ));
        };
        let (entity_id, data) = if let Some(entity_id) = req.entity_id.as_ref() {
            let entity = self
                .storage
                .get(&collection, entity_id)?
                .ok_or_else(|| AxonError::NotFound(entity_id.to_string()))?;
            (Some(entity_id), entity_policy_data(&entity))
        } else {
            let data = req.data.ok_or_else(|| {
                AxonError::InvalidArgument("read explanation requires entityId or data".into())
            })?;
            (None, data)
        };

        self.explain_operation_policy_response(
            "read",
            &collection,
            entity_id,
            &plan,
            &snapshot,
            PolicyOperation::Read,
            &data,
            "row_read_denied",
            policy_version,
            operation_index,
            preview,
        )
    }

    fn explain_create_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let (collection, schema) = self.explain_collection_schema(&req, preview)?;
        let policy_version = schema.version;
        let mut data = req
            .data
            .ok_or_else(|| AxonError::InvalidArgument("create explanation requires data".into()))?;
        enforce_lifecycle_initial_state(&schema, &mut data, LifecycleEnforcementMode::Create)?;
        let entity_id = req.entity_id.as_ref();
        let Some((snapshot, plan)) = self.explain_policy_plan_for_request(
            &collection,
            &schema,
            caller,
            attribution,
            preview,
        )?
        else {
            return Ok(policy_explanation(
                "create",
                Some(&collection),
                entity_id,
                operation_index,
                "allow",
                "no_policy",
                policy_version,
            ));
        };

        self.explain_write_policy_response(
            "create",
            &collection,
            entity_id,
            &plan,
            &snapshot,
            PolicyWriteCheck {
                collection: &collection,
                entity_id,
                operation: PolicyOperation::Create,
                current_data: None,
                candidate_data: &data,
                field_scope: FieldWriteScope::PresentFields(&data),
                operation_index,
            },
            policy_version,
            preview,
        )
    }

    fn explain_update_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let (collection, schema) = self.explain_collection_schema(&req, preview)?;
        let policy_version = schema.version;
        let entity_id = req.entity_id.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument("update explanation requires entityId".into())
        })?;
        let mut data = req
            .data
            .ok_or_else(|| AxonError::InvalidArgument("update explanation requires data".into()))?;
        enforce_lifecycle_initial_state(&schema, &mut data, LifecycleEnforcementMode::Update)?;
        let current = self
            .storage
            .get(&collection, entity_id)?
            .ok_or_else(|| AxonError::NotFound(entity_id.to_string()))?;
        let Some((snapshot, plan)) = self.explain_policy_plan_for_request(
            &collection,
            &schema,
            caller,
            attribution,
            preview,
        )?
        else {
            return Ok(policy_explanation(
                "update",
                Some(&collection),
                Some(entity_id),
                operation_index,
                "allow",
                "no_policy",
                policy_version,
            ));
        };

        self.explain_write_policy_response(
            "update",
            &collection,
            Some(entity_id),
            &plan,
            &snapshot,
            PolicyWriteCheck {
                collection: &collection,
                entity_id: Some(entity_id),
                operation: PolicyOperation::Update,
                current_data: Some(&current.data),
                candidate_data: &data,
                field_scope: FieldWriteScope::PresentFields(&data),
                operation_index,
            },
            policy_version,
            preview,
        )
    }

    fn explain_patch_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let (collection, schema) = self.explain_collection_schema(&req, preview)?;
        let policy_version = schema.version;
        let entity_id = req.entity_id.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument("patch explanation requires entityId".into())
        })?;
        let patch = req
            .patch
            .ok_or_else(|| AxonError::InvalidArgument("patch explanation requires patch".into()))?;
        let current = self
            .storage
            .get(&collection, entity_id)?
            .ok_or_else(|| AxonError::NotFound(entity_id.to_string()))?;
        let mut merged = current.data.clone();
        json_merge_patch(&mut merged, &patch);
        enforce_lifecycle_initial_state(&schema, &mut merged, LifecycleEnforcementMode::Update)?;
        let Some((snapshot, plan)) = self.explain_policy_plan_for_request(
            &collection,
            &schema,
            caller,
            attribution,
            preview,
        )?
        else {
            return Ok(policy_explanation(
                "patch",
                Some(&collection),
                Some(entity_id),
                operation_index,
                "allow",
                "no_policy",
                policy_version,
            ));
        };

        self.explain_write_policy_response(
            "patch",
            &collection,
            Some(entity_id),
            &plan,
            &snapshot,
            PolicyWriteCheck {
                collection: &collection,
                entity_id: Some(entity_id),
                operation: PolicyOperation::Update,
                current_data: Some(&current.data),
                candidate_data: &merged,
                field_scope: FieldWriteScope::Patch(&patch),
                operation_index,
            },
            policy_version,
            preview,
        )
    }

    fn explain_delete_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let (collection, schema) = self.explain_collection_schema(&req, preview)?;
        let policy_version = schema.version;
        let entity_id = req.entity_id.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument("delete explanation requires entityId".into())
        })?;
        let current = self
            .storage
            .get(&collection, entity_id)?
            .ok_or_else(|| AxonError::NotFound(entity_id.to_string()))?;
        let Some((snapshot, plan)) = self.explain_policy_plan_for_request(
            &collection,
            &schema,
            caller,
            attribution,
            preview,
        )?
        else {
            return Ok(policy_explanation(
                "delete",
                Some(&collection),
                Some(entity_id),
                operation_index,
                "allow",
                "no_policy",
                policy_version,
            ));
        };

        self.explain_write_policy_response(
            "delete",
            &collection,
            Some(entity_id),
            &plan,
            &snapshot,
            PolicyWriteCheck {
                collection: &collection,
                entity_id: Some(entity_id),
                operation: PolicyOperation::Delete,
                current_data: Some(&current.data),
                candidate_data: &current.data,
                field_scope: FieldWriteScope::PresentFields(&current.data),
                operation_index,
            },
            policy_version,
            preview,
        )
    }

    fn explain_transition_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let (collection, schema) = self.explain_collection_schema(&req, preview)?;
        let policy_version = schema.version;
        let entity_id = req.entity_id.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument("transition explanation requires entityId".into())
        })?;
        let lifecycle_name = req.lifecycle_name.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument("transition explanation requires lifecycleName".into())
        })?;
        let target_state = req.target_state.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument("transition explanation requires targetState".into())
        })?;
        let lifecycle =
            schema
                .lifecycles
                .get(lifecycle_name)
                .ok_or_else(|| AxonError::LifecycleNotFound {
                    lifecycle_name: lifecycle_name.clone(),
                })?;
        let entity = self
            .storage
            .get(&collection, entity_id)?
            .ok_or_else(|| AxonError::NotFound(entity_id.to_string()))?;
        let current_state = entity
            .data
            .get(&lifecycle.field)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let allowed = lifecycle
            .transitions
            .get(&current_state)
            .cloned()
            .unwrap_or_default();
        if !allowed.contains(target_state) {
            return Err(AxonError::InvalidTransition {
                lifecycle_name: lifecycle_name.clone(),
                current_state,
                target_state: target_state.clone(),
                valid_transitions: allowed,
            });
        }
        let mut data = entity.data.clone();
        data[&lifecycle.field] = Value::String(target_state.clone());
        let Some((snapshot, plan)) = self.explain_policy_plan_for_request(
            &collection,
            &schema,
            caller,
            attribution,
            preview,
        )?
        else {
            return Ok(policy_explanation(
                "transition",
                Some(&collection),
                Some(entity_id),
                operation_index,
                "allow",
                "no_policy",
                policy_version,
            ));
        };

        let transition = self.explain_transition_policy_response(
            &collection,
            entity_id,
            &plan,
            &snapshot,
            &lifecycle.field,
            target_state,
            &data,
            policy_version,
            operation_index,
            preview,
        )?;
        if transition.decision == "deny" {
            return Ok(transition);
        }

        let mut write = self.explain_write_policy_response(
            "transition",
            &collection,
            Some(entity_id),
            &plan,
            &snapshot,
            PolicyWriteCheck {
                collection: &collection,
                entity_id: Some(entity_id),
                operation: PolicyOperation::Update,
                current_data: Some(&entity.data),
                candidate_data: &data,
                field_scope: FieldWriteScope::PresentFields(&data),
                operation_index,
            },
            policy_version,
            preview,
        )?;
        merge_policy_explanation(&mut write, transition);
        Ok(finalize_policy_explanation(write))
    }

    fn explain_rollback_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let (collection, schema) = self.explain_collection_schema(&req, preview)?;
        let policy_version = schema.version;
        let entity_id = req.entity_id.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument("rollback explanation requires entityId".into())
        })?;
        let to_version = req.to_version.ok_or_else(|| {
            AxonError::InvalidArgument("rollback explanation requires toVersion".into())
        })?;
        let source = self.resolve_rollback_source_entry(
            &collection,
            entity_id,
            &RollbackEntityTarget::Version(to_version),
        )?;
        let target_data = source.data_after.clone().ok_or_else(|| {
            AxonError::NotFound(format!(
                "entity version {} not found in audit log for {}",
                to_version, entity_id
            ))
        })?;
        let current = self.storage.get(&collection, entity_id)?;
        let operation = if current.is_some() {
            PolicyOperation::Update
        } else {
            PolicyOperation::Create
        };
        let Some((snapshot, plan)) = self.explain_policy_plan_for_request(
            &collection,
            &schema,
            caller,
            attribution,
            preview,
        )?
        else {
            return Ok(policy_explanation(
                "rollback",
                Some(&collection),
                Some(entity_id),
                operation_index,
                "allow",
                "no_policy",
                policy_version,
            ));
        };

        self.explain_write_policy_response(
            "rollback",
            &collection,
            Some(entity_id),
            &plan,
            &snapshot,
            PolicyWriteCheck {
                collection: &collection,
                entity_id: Some(entity_id),
                operation,
                current_data: current.as_ref().map(|entity| &entity.data),
                candidate_data: &target_data,
                field_scope: FieldWriteScope::PresentFields(&target_data),
                operation_index,
            },
            policy_version,
            preview,
        )
    }

    fn explain_transaction_policy(
        &self,
        req: ExplainPolicyRequest,
        caller: &CallerIdentity,
        attribution: Option<&AuditAttribution>,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let mut response = policy_explanation(
            "transaction",
            None,
            None,
            operation_index,
            "allow",
            "allowed",
            0,
        );
        let mut saw_needs_approval = false;
        for (index, operation) in req.operations.into_iter().enumerate() {
            let child =
                self.explain_policy_inner(operation, caller, attribution, Some(index), preview)?;
            response.policy_version = response.policy_version.max(child.policy_version);
            if child.decision == "deny" {
                response.decision = "deny".into();
                response.reason = "transaction_denied".into();
            } else if response.decision != "deny" && child.decision == "needs_approval" {
                saw_needs_approval = true;
                response.decision = "needs_approval".into();
                response.reason = "needs_approval".into();
                if response.approval.is_none() {
                    response.approval = child.approval.clone();
                }
            }
            merge_policy_explanation(&mut response, child.clone());
            response.operations.push(child);
        }
        if response.decision == "allow" && !saw_needs_approval {
            response.reason = "allowed".into();
        }
        Ok(finalize_policy_explanation(response))
    }

    #[allow(clippy::too_many_arguments)]
    fn explain_operation_policy_response(
        &self,
        operation_name: impl Into<String>,
        collection: &CollectionId,
        entity_id: Option<&EntityId>,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        operation: PolicyOperation,
        data: &Value,
        denied_reason: &str,
        policy_version: u32,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let mut response = policy_explanation(
            operation_name,
            Some(collection),
            entity_id,
            operation_index,
            "allow",
            "allowed",
            policy_version,
        );
        let mut allow_rules_present = false;
        let mut allow_matched = false;

        for policy in applicable_operation_policies(plan, &operation) {
            for rule in &policy.deny {
                if self.policy_rule_matches(rule, snapshot, &operation, data, preview)? {
                    response.decision = "deny".into();
                    response.reason = denied_reason.into();
                    response
                        .rules
                        .push(operation_rule_match(rule, "operation_deny"));
                    return Ok(finalize_policy_explanation(response));
                }
            }

            if !policy.allow.is_empty() {
                allow_rules_present = true;
            }
            for rule in &policy.allow {
                if self.policy_rule_matches(rule, snapshot, &operation, data, preview)? {
                    allow_matched = true;
                    response
                        .rules
                        .push(operation_rule_match(rule, "operation_allow"));
                }
            }
        }

        if allow_rules_present && !allow_matched {
            response.decision = "deny".into();
            response.reason = denied_reason.into();
            response.policy_ids.push(operation.as_str().to_string());
        }

        Ok(finalize_policy_explanation(response))
    }

    #[allow(clippy::too_many_arguments)]
    fn explain_write_policy_response(
        &self,
        operation_name: impl Into<String>,
        collection: &CollectionId,
        entity_id: Option<&EntityId>,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyWriteCheck<'_>,
        policy_version: u32,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let operation_name = operation_name.into();
        let mut response = policy_explanation(
            operation_name.clone(),
            Some(collection),
            entity_id,
            check.operation_index,
            "allow",
            "allowed",
            policy_version,
        );

        if let Some(current_data) = check.current_data {
            let read = self.explain_operation_policy_response(
                operation_name.clone(),
                collection,
                entity_id,
                plan,
                snapshot,
                PolicyOperation::Read,
                current_data,
                "row_read_denied",
                policy_version,
                check.operation_index,
                preview,
            )?;
            merge_policy_explanation(&mut response, read.clone());
            if read.decision == "deny" {
                response.decision = "deny".into();
                response.reason = read.reason;
                return Ok(finalize_policy_explanation(response));
            }
        }

        let operation = self.explain_operation_policy_response(
            operation_name,
            collection,
            entity_id,
            plan,
            snapshot,
            check.operation.clone(),
            check.candidate_data,
            "row_write_denied",
            policy_version,
            check.operation_index,
            preview,
        )?;
        merge_policy_explanation(&mut response, operation.clone());
        if operation.decision == "deny" {
            response.decision = "deny".into();
            response.reason = operation.reason;
            return Ok(finalize_policy_explanation(response));
        }

        if let Some(field_denial) =
            self.explain_field_write_policy(plan, snapshot, check.clone(), policy_version, preview)?
        {
            merge_policy_explanation(&mut response, field_denial.clone());
            response.decision = "deny".into();
            response.reason = field_denial.reason;
            return Ok(finalize_policy_explanation(response));
        }

        if let Some(envelope) =
            self.explain_policy_envelopes(plan, snapshot, check.clone(), policy_version, preview)?
        {
            merge_policy_explanation(&mut response, envelope.clone());
            response.decision = envelope.decision;
            response.reason = envelope.reason;
            response.approval = envelope.approval;
        }

        Ok(finalize_policy_explanation(response))
    }

    fn explain_field_write_policy(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyWriteCheck<'_>,
        policy_version: u32,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Option<PolicyExplanationResponse>, AxonError> {
        for (field_path, field_policy) in &plan.fields {
            let Some(write_policy) = field_policy.write.as_ref() else {
                continue;
            };
            if !field_write_scope_touches_path(check.field_scope, field_path) {
                continue;
            }

            for rule in &write_policy.deny {
                if self.field_policy_rule_matches(
                    rule,
                    snapshot,
                    &check.operation,
                    check.candidate_data,
                    preview,
                )? {
                    let mut response = policy_explanation(
                        check.operation.as_str(),
                        Some(check.collection),
                        check.entity_id,
                        check.operation_index,
                        "deny",
                        "field_write_denied",
                        policy_version,
                    );
                    response.denied_fields.push(field_path.clone());
                    response.field_paths.push(field_path.clone());
                    response
                        .rules
                        .push(field_rule_match(rule, field_path, "field_write_deny"));
                    return Ok(Some(finalize_policy_explanation(response)));
                }
            }

            if !write_policy.allow.is_empty() {
                let mut matched = false;
                for rule in &write_policy.allow {
                    if self.field_policy_rule_matches(
                        rule,
                        snapshot,
                        &check.operation,
                        check.candidate_data,
                        preview,
                    )? {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    let mut response = policy_explanation(
                        check.operation.as_str(),
                        Some(check.collection),
                        check.entity_id,
                        check.operation_index,
                        "deny",
                        "field_write_denied",
                        policy_version,
                    );
                    response.denied_fields.push(field_path.clone());
                    response.field_paths.push(field_path.clone());
                    if let Some(rule) = write_policy.allow.first() {
                        response.rules.push(field_rule_match(
                            rule,
                            field_path,
                            "field_write_allow",
                        ));
                    }
                    return Ok(Some(finalize_policy_explanation(response)));
                }
            }
        }

        Ok(None)
    }

    fn explain_policy_envelopes(
        &self,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        check: PolicyWriteCheck<'_>,
        policy_version: u32,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<Option<PolicyExplanationResponse>, AxonError> {
        let mut needs_approval = None;

        for envelope in applicable_policy_envelopes(plan, &check.operation) {
            if self.policy_predicate_matches(
                envelope.when.as_ref(),
                PolicyPredicateContext {
                    snapshot,
                    operation: &check.operation,
                    data: check.candidate_data,
                    preview,
                },
            )? {
                match envelope.decision {
                    PolicyDecision::Deny => {
                        let mut response = policy_explanation(
                            check.operation.as_str(),
                            Some(check.collection),
                            check.entity_id,
                            check.operation_index,
                            "deny",
                            "row_write_denied",
                            policy_version,
                        );
                        response.policy_ids.push(envelope.envelope_id.clone());
                        return Ok(Some(finalize_policy_explanation(response)));
                    }
                    PolicyDecision::NeedsApproval => {
                        needs_approval = Some(envelope);
                    }
                    PolicyDecision::Allow => {}
                }
            }
        }

        Ok(needs_approval.map(|envelope| {
            let mut response = policy_explanation(
                check.operation.as_str(),
                Some(check.collection),
                check.entity_id,
                check.operation_index,
                "needs_approval",
                "needs_approval",
                policy_version,
            );
            response.policy_ids.push(envelope.envelope_id.clone());
            response.approval = Some(approval_envelope_summary(envelope));
            finalize_policy_explanation(response)
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn explain_transition_policy_response(
        &self,
        collection: &CollectionId,
        entity_id: &EntityId,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
        lifecycle_field: &str,
        target_state: &str,
        data: &Value,
        policy_version: u32,
        operation_index: Option<usize>,
        preview: Option<&PreviewedSchemaPlan<'_>>,
    ) -> Result<PolicyExplanationResponse, AxonError> {
        let mut response = policy_explanation(
            "transition",
            Some(collection),
            Some(entity_id),
            operation_index,
            "allow",
            "allowed",
            policy_version,
        );
        let Some(transitions) = plan.transitions.get(lifecycle_field) else {
            return Ok(response);
        };
        let Some(policy) = transitions.get(target_state) else {
            return Ok(response);
        };

        for rule in &policy.deny {
            if self.policy_rule_matches(rule, snapshot, &PolicyOperation::Update, data, preview)? {
                response.decision = "deny".into();
                response.reason = "row_write_denied".into();
                response.field_paths.push(lifecycle_field.to_string());
                response
                    .rules
                    .push(operation_rule_match(rule, "transition_deny"));
                return Ok(finalize_policy_explanation(response));
            }
        }

        if !policy.allow.is_empty() {
            let mut matched = false;
            for rule in &policy.allow {
                if self.policy_rule_matches(
                    rule,
                    snapshot,
                    &PolicyOperation::Update,
                    data,
                    preview,
                )? {
                    matched = true;
                    response
                        .rules
                        .push(operation_rule_match(rule, "transition_allow"));
                }
            }
            if !matched {
                response.decision = "deny".into();
                response.reason = "row_write_denied".into();
                response.field_paths.push(lifecycle_field.to_string());
            }
        }

        Ok(finalize_policy_explanation(response))
    }

    fn get_visible_entity_for_read_with_context(
        &self,
        collection: &CollectionId,
        id: &EntityId,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<Option<Entity>, AxonError> {
        let Some(entity) = self.storage.get(collection, id)? else {
            return Ok(None);
        };
        if self.entity_visible_for_read_with_context(collection, &entity, caller, attribution)? {
            Ok(Some(entity))
        } else {
            Ok(None)
        }
    }

    fn plan_read_policy_storage_filters(
        &self,
        collection: &CollectionId,
        schema: Option<&CollectionSchema>,
        plan: &PolicyPlan,
        snapshot: &PolicyRequestSnapshot,
    ) -> Result<PolicyStoragePlan, AxonError> {
        let Some(schema) = schema else {
            return Ok(PolicyStoragePlan::default());
        };
        let policies = applicable_operation_policies(plan, &PolicyOperation::Read);
        if policies.is_empty() {
            return Ok(PolicyStoragePlan::default());
        }

        let mut output = PolicyStoragePlan {
            explain: vec!["policy.read".into()],
            ..Default::default()
        };

        for policy in policies {
            if !policy.deny.is_empty() {
                output.post_filter = true;
                output.explain.push("deny rules require post_filter".into());
                for rule in &policy.deny {
                    if let PolicyRuleCandidatePlan::Unindexed {
                        missing_index: Some(index),
                    } = self.policy_rule_candidate_plan(collection, schema, rule, snapshot)?
                    {
                        output.missing_index.get_or_insert(index);
                    }
                }
            }

            if policy.allow.is_empty() {
                continue;
            }

            let mut indexed_rule_ids = Vec::new();
            let mut all_rules_indexed = true;
            let mut allow_all = false;

            for rule in &policy.allow {
                match self.policy_rule_candidate_plan(collection, schema, rule, snapshot)? {
                    PolicyRuleCandidatePlan::All => {
                        allow_all = true;
                    }
                    PolicyRuleCandidatePlan::Indexed {
                        entity_ids,
                        storage_filters,
                    } => {
                        indexed_rule_ids.push(entity_ids);
                        output.storage_filters.extend(storage_filters);
                    }
                    PolicyRuleCandidatePlan::Unindexed { missing_index } => {
                        all_rules_indexed = false;
                        output.post_filter = true;
                        if let Some(index) = missing_index {
                            output.missing_index.get_or_insert(index);
                        }
                    }
                }
            }

            if allow_all {
                output
                    .explain
                    .push("allow rule covers all rows; no policy storage filter".into());
                output.candidate_ids = None;
            } else if all_rules_indexed {
                let ids = union_entity_id_sets(indexed_rule_ids);
                output.explain.push(format!(
                    "allow rules covered by {} storage filter(s)",
                    output.storage_filters.len()
                ));
                output.candidate_ids = Some(ids);
                output.post_filter = output.post_filter || !policy.deny.is_empty();
            } else {
                output
                    .explain
                    .push("allow rules require bounded post_filter".into());
                output.candidate_ids = None;
            }
        }

        Ok(output)
    }

    fn policy_rule_candidate_plan(
        &self,
        collection: &CollectionId,
        schema: &CollectionSchema,
        rule: &CompiledPolicyRule,
        snapshot: &PolicyRequestSnapshot,
    ) -> Result<PolicyRuleCandidatePlan, AxonError> {
        match rule.when.as_ref().map(|predicate| {
            policy_static_predicate_matches(predicate, snapshot, &PolicyOperation::Read)
        }) {
            Some(Some(false)) => {
                return Ok(PolicyRuleCandidatePlan::Indexed {
                    entity_ids: Vec::new(),
                    storage_filters: vec![format!("rule:{}:static_false", policy_rule_label(rule))],
                });
            }
            Some(None) => {
                return Ok(PolicyRuleCandidatePlan::Unindexed {
                    missing_index: None,
                });
            }
            Some(Some(true)) | None => {}
        }

        match &rule.where_clause {
            Some(predicate) => {
                self.policy_predicate_candidate_plan(collection, schema, predicate, snapshot)
            }
            None => Ok(PolicyRuleCandidatePlan::All),
        }
    }

    fn policy_predicate_candidate_plan(
        &self,
        collection: &CollectionId,
        schema: &CollectionSchema,
        predicate: &CompiledPredicate,
        snapshot: &PolicyRequestSnapshot,
    ) -> Result<PolicyRuleCandidatePlan, AxonError> {
        match predicate {
            CompiledPredicate::All(predicates) => {
                let mut current: Option<Vec<EntityId>> = None;
                let mut filters = Vec::new();
                for predicate in predicates {
                    match self
                        .policy_predicate_candidate_plan(collection, schema, predicate, snapshot)?
                    {
                        PolicyRuleCandidatePlan::All => {}
                        PolicyRuleCandidatePlan::Indexed {
                            entity_ids,
                            storage_filters,
                        } => {
                            current = Some(match current {
                                Some(existing) => intersect_entity_ids(existing, entity_ids),
                                None => entity_ids,
                            });
                            filters.extend(storage_filters);
                        }
                        PolicyRuleCandidatePlan::Unindexed { missing_index } => {
                            return Ok(PolicyRuleCandidatePlan::Unindexed { missing_index });
                        }
                    }
                }
                Ok(current.map_or(PolicyRuleCandidatePlan::All, |entity_ids| {
                    PolicyRuleCandidatePlan::Indexed {
                        entity_ids,
                        storage_filters: filters,
                    }
                }))
            }
            CompiledPredicate::Any(predicates) => {
                let mut id_sets = Vec::new();
                let mut filters = Vec::new();
                for predicate in predicates {
                    match self
                        .policy_predicate_candidate_plan(collection, schema, predicate, snapshot)?
                    {
                        PolicyRuleCandidatePlan::All => return Ok(PolicyRuleCandidatePlan::All),
                        PolicyRuleCandidatePlan::Indexed {
                            entity_ids,
                            storage_filters,
                        } => {
                            id_sets.push(entity_ids);
                            filters.extend(storage_filters);
                        }
                        PolicyRuleCandidatePlan::Unindexed { missing_index } => {
                            return Ok(PolicyRuleCandidatePlan::Unindexed { missing_index });
                        }
                    }
                }
                Ok(PolicyRuleCandidatePlan::Indexed {
                    entity_ids: union_entity_id_sets(id_sets),
                    storage_filters: filters,
                })
            }
            CompiledPredicate::Compare(comparison) => {
                self.policy_comparison_candidate_plan(collection, schema, comparison, snapshot)
            }
            CompiledPredicate::Related(related) => {
                self.policy_related_candidate_plan(collection, related, snapshot)
            }
            CompiledPredicate::Operation(operation) if operation == &PolicyOperation::Read => {
                Ok(PolicyRuleCandidatePlan::All)
            }
            _ => Ok(PolicyRuleCandidatePlan::Unindexed {
                missing_index: None,
            }),
        }
    }

    fn policy_comparison_candidate_plan(
        &self,
        collection: &CollectionId,
        schema: &CollectionSchema,
        comparison: &CompiledComparison,
        snapshot: &PolicyRequestSnapshot,
    ) -> Result<PolicyRuleCandidatePlan, AxonError> {
        let PredicateTarget::Field(field) = &comparison.target else {
            return Ok(PolicyRuleCandidatePlan::Unindexed {
                missing_index: None,
            });
        };
        let Some(index) = schema.indexes.iter().find(|index| index.field == *field) else {
            return Ok(PolicyRuleCandidatePlan::Unindexed {
                missing_index: Some(field.clone()),
            });
        };

        let lookup = |value: &Value| -> Result<Option<Vec<EntityId>>, AxonError> {
            let Some(index_value) = extract_index_value(value, &index.index_type) else {
                return Ok(Some(Vec::new()));
            };
            Ok(self
                .storage
                .index_lookup(collection, field, &index_value)
                .ok())
        };

        let (entity_ids, label) = match &comparison.op {
            CompiledCompareOp::Eq(value) => match lookup(value)? {
                Some(ids) => (ids, format!("index:{field}:eq")),
                None => {
                    return Ok(PolicyRuleCandidatePlan::Unindexed {
                        missing_index: Some(field.clone()),
                    })
                }
            },
            CompiledCompareOp::EqSubject(subject) | CompiledCompareOp::ContainsSubject(subject) => {
                let Some(value) = policy_subject_value(snapshot, subject) else {
                    return Ok(PolicyRuleCandidatePlan::Indexed {
                        entity_ids: Vec::new(),
                        storage_filters: vec![format!("index:{field}:subject_missing")],
                    });
                };
                match lookup(value)? {
                    Some(ids) => (ids, format!("index:{field}:subject:{subject}")),
                    None => {
                        return Ok(PolicyRuleCandidatePlan::Unindexed {
                            missing_index: Some(field.clone()),
                        })
                    }
                }
            }
            CompiledCompareOp::In(values) => {
                let mut sets = Vec::new();
                for value in values {
                    match lookup(value)? {
                        Some(ids) => sets.push(ids),
                        None => {
                            return Ok(PolicyRuleCandidatePlan::Unindexed {
                                missing_index: Some(field.clone()),
                            })
                        }
                    }
                }
                (union_entity_id_sets(sets), format!("index:{field}:in"))
            }
            CompiledCompareOp::Gt(value)
            | CompiledCompareOp::Gte(value)
            | CompiledCompareOp::Lt(value)
            | CompiledCompareOp::Lte(value) => {
                let Some(index_value) = extract_index_value(value, &index.index_type) else {
                    return Ok(PolicyRuleCandidatePlan::Indexed {
                        entity_ids: Vec::new(),
                        storage_filters: vec![format!("index:{field}:type_mismatch")],
                    });
                };
                let (lower, upper, op_label) = match &comparison.op {
                    CompiledCompareOp::Gt(_) => (
                        std::ops::Bound::Excluded(&index_value),
                        std::ops::Bound::Unbounded,
                        "gt",
                    ),
                    CompiledCompareOp::Gte(_) => (
                        std::ops::Bound::Included(&index_value),
                        std::ops::Bound::Unbounded,
                        "gte",
                    ),
                    CompiledCompareOp::Lt(_) => (
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Excluded(&index_value),
                        "lt",
                    ),
                    CompiledCompareOp::Lte(_) => (
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Included(&index_value),
                        "lte",
                    ),
                    _ => unreachable!(),
                };
                let Some(ids) = self
                    .storage
                    .index_range(collection, field, lower, upper)
                    .ok()
                else {
                    return Ok(PolicyRuleCandidatePlan::Unindexed {
                        missing_index: Some(field.clone()),
                    });
                };
                (ids, format!("index:{field}:{op_label}"))
            }
            _ => {
                return Ok(PolicyRuleCandidatePlan::Unindexed {
                    missing_index: Some(field.clone()),
                })
            }
        };

        Ok(PolicyRuleCandidatePlan::Indexed {
            entity_ids,
            storage_filters: vec![label],
        })
    }

    fn policy_related_candidate_plan(
        &self,
        collection: &CollectionId,
        related: &axon_schema::CompiledRelationshipPredicate,
        snapshot: &PolicyRequestSnapshot,
    ) -> Result<PolicyRuleCandidatePlan, AxonError> {
        let links = self.load_all_links()?;
        let mut entity_ids = std::collections::BTreeSet::new();

        for link in links {
            let (candidate_id, target_collection, target_id) = match related.direction {
                LinkDirection::Outgoing => {
                    if link.source_collection != *collection
                        || link.link_type != related.link_type
                        || link.target_collection.as_str() != related.target_collection
                    {
                        continue;
                    }
                    (&link.source_id, &link.target_collection, &link.target_id)
                }
                LinkDirection::Incoming => {
                    if link.target_collection != *collection
                        || link.link_type != related.link_type
                        || link.source_collection.as_str() != related.target_collection
                    {
                        continue;
                    }
                    (&link.target_id, &link.source_collection, &link.source_id)
                }
            };

            if let Some(operation) = &related.target_policy {
                let Some(target) = self.storage.get(target_collection, target_id)? else {
                    continue;
                };
                let Some(target_schema) = self.storage.get_schema(target_collection)? else {
                    continue;
                };
                let Some(target_plan) = self.compile_policy_plan_for_schema(&target_schema)? else {
                    continue;
                };
                let mut target_snapshot = snapshot.clone();
                target_snapshot.collection = target_collection.clone();
                let target_data = entity_policy_data(&target);
                if !self.policy_operation_allows(
                    &target_plan,
                    &target_snapshot,
                    PolicyOperationCheck {
                        collection: target_collection,
                        entity_id: Some(target_id),
                        operation: operation.clone(),
                        data: &target_data,
                        operation_index: None,
                    },
                    None,
                )? {
                    continue;
                }
            }

            entity_ids.insert(candidate_id.clone());
        }

        Ok(PolicyRuleCandidatePlan::Indexed {
            entity_ids: entity_ids.into_iter().collect(),
            storage_filters: vec![format!(
                "link_index:{:?}:{}",
                related.direction, related.link_type
            )
            .to_lowercase()],
        })
    }

    fn policy_subject_for_request(
        &self,
        identity: Option<&AccessControlIdentity>,
        actor: Option<&str>,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
        database_id: &str,
    ) -> Result<PolicySubjectSnapshot, AxonError> {
        let actor = actor
            .or_else(|| caller.map(|caller| caller.actor.as_str()))
            .unwrap_or("anonymous")
            .to_string();
        let mut bindings =
            default_policy_subject_bindings(actor.as_str(), caller, attribution, database_id);

        if let Some(identity) = identity {
            for (name, source) in &identity.subject {
                if let Some(value) =
                    resolve_policy_subject_expression(source, &bindings, &HashMap::new())
                {
                    bindings.insert(name.clone(), value);
                }
            }
        }

        let mut attributes = HashMap::new();
        if let Some(identity) = identity {
            for (name, source) in &identity.attributes {
                if let Some(value) =
                    self.resolve_policy_identity_attribute(name, source, &bindings, database_id)?
                {
                    attributes.insert(name.clone(), value.clone());
                    bindings.insert(name.clone(), value);
                }
            }

            for (name, source) in &identity.aliases {
                if let Some(value) =
                    resolve_policy_subject_expression(source, &bindings, &attributes)
                {
                    bindings.insert(name.clone(), value);
                }
            }
        }

        Ok(PolicySubjectSnapshot {
            actor,
            bindings,
            attributes,
        })
    }

    fn resolve_policy_identity_attribute(
        &self,
        name: &str,
        source: &IdentityAttributeSource,
        bindings: &HashMap<String, Value>,
        database_id: &str,
    ) -> Result<Option<Value>, AxonError> {
        if source.from != "collection" {
            return Err(AxonError::SchemaValidation(format!(
                "unsupported access_control identity attribute source '{}' for '{}'",
                source.from, name
            )));
        }

        let collection =
            required_identity_attribute_field(name, "collection", source.collection.as_deref())?;
        let key_field =
            required_identity_attribute_field(name, "key_field", source.key_field.as_deref())?;
        let key_subject =
            required_identity_attribute_field(name, "key_subject", source.key_subject.as_deref())?;
        let value_field =
            required_identity_attribute_field(name, "value_field", source.value_field.as_deref())?;

        let Some(key_value) = bindings.get(key_subject) else {
            return Ok(None);
        };

        let collection = request_scoped_collection_id(collection, database_id);
        for entity in self.storage.range_scan(&collection, None, None, None)? {
            let Some(candidate) = resolve_field_path(&entity.data, key_field) else {
                continue;
            };
            if candidate == key_value {
                return Ok(resolve_field_path(&entity.data, value_field).cloned());
            }
        }

        Ok(None)
    }

    /// Consume this handler, returning the underlying storage adapter.
    ///
    /// Useful in tests that need to reconstruct a handler from the same storage
    /// to verify that persisted state (e.g. collection registrations) survives.
    pub fn into_storage(self) -> S {
        self.storage
    }

    /// Commits a [`Transaction`] through this handler's storage and audit log.
    pub fn commit_transaction(
        &mut self,
        tx: crate::transaction::Transaction,
        actor: Option<String>,
        attribution: Option<AuditAttribution>,
    ) -> Result<Vec<axon_core::types::Entity>, AxonError> {
        self.commit_transaction_inner(tx, actor, attribution, None)
    }

    fn commit_transaction_inner(
        &mut self,
        tx: crate::transaction::Transaction,
        actor: Option<String>,
        attribution: Option<AuditAttribution>,
        caller: Option<&CallerIdentity>,
    ) -> Result<Vec<axon_core::types::Entity>, AxonError> {
        self.enforce_transaction_policy(&tx, actor.as_deref(), attribution.as_ref(), caller)?;
        tx.commit(&mut self.storage, &mut self.audit, actor, attribution)
    }

    fn enforce_transaction_policy(
        &self,
        tx: &crate::transaction::Transaction,
        actor: Option<&str>,
        attribution: Option<&AuditAttribution>,
        caller: Option<&CallerIdentity>,
    ) -> Result<(), AxonError> {
        for (operation_index, op) in tx.staged_ops().iter().enumerate() {
            match op {
                crate::transaction::StagedOp::Entity(op) => {
                    let schema = self.storage.get_schema(&op.entity.collection)?;
                    let policy_snapshot = self.policy_snapshot_for_request(
                        &op.entity.collection,
                        schema.as_ref(),
                        actor,
                        caller,
                        attribution,
                    )?;
                    let current = self.storage.get(&op.entity.collection, &op.entity.id)?;
                    let (operation, current_data, candidate_data, field_scope) = match op.mutation {
                        MutationType::EntityCreate => (
                            PolicyOperation::Create,
                            None,
                            &op.entity.data,
                            FieldWriteScope::PresentFields(&op.entity.data),
                        ),
                        MutationType::EntityUpdate => (
                            PolicyOperation::Update,
                            current.as_ref().map(|entity| &entity.data),
                            &op.entity.data,
                            FieldWriteScope::PresentFields(&op.entity.data),
                        ),
                        MutationType::EntityDelete => {
                            let data = current
                                .as_ref()
                                .map(|entity| &entity.data)
                                .or(op.data_before.as_ref())
                                .unwrap_or(&op.entity.data);
                            (
                                PolicyOperation::Delete,
                                current.as_ref().map(|entity| &entity.data),
                                data,
                                FieldWriteScope::PresentFields(data),
                            )
                        }
                        _ => continue,
                    };

                    self.enforce_write_policy(
                        schema.as_ref(),
                        policy_snapshot.as_ref(),
                        PolicyWriteCheck {
                            collection: &op.entity.collection,
                            entity_id: Some(&op.entity.id),
                            operation,
                            current_data,
                            candidate_data,
                            field_scope,
                            operation_index: Some(operation_index),
                        },
                    )?;
                }
                crate::transaction::StagedOp::LinkCreate(_)
                | crate::transaction::StagedOp::LinkDelete(_) => {}
            }
        }

        Ok(())
    }

    // ── Identity-propagating wrappers (FEAT-012) ────────────────────────────
    //
    // These variants accept a [`CallerIdentity`] extracted from a transport
    // layer (gRPC metadata, HTTP header). They override any actor field set
    // on the incoming request with `caller.actor`, so audit entries always
    // record the authenticated caller rather than whatever string the
    // client put in the JSON body. The non-`_with_caller` methods remain for
    // internal callers (tests, CLI, MCP) that construct requests directly
    // without an HTTP/gRPC transport origin.

    /// Create or replace an entity, overriding `req.actor` with the caller identity.
    ///
    /// This is the transport-level create path used by HTTP `/entities` POST
    /// and gRPC `CreateEntity`. It delegates to [`StorageAdapter::put`], so an
    /// existing entity with the same collection/id is overwritten instead of
    /// being treated as a duplicate. Strict create semantics live in
    /// [`Self::create_entity_strict_with_caller`] and transaction `create`
    /// operations.
    pub fn create_entity_with_caller(
        &mut self,
        mut req: CreateEntityRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<CreateEntityResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.create_entity_inner(req, Some(caller))
    }

    /// Strictly create an entity, rejecting duplicate collection/id pairs.
    ///
    /// Typed GraphQL `createXxx` mutations use this contract so they behave
    /// like transaction `op:create`: if an entity already exists, the call
    /// returns [`AxonError::ConflictingVersion`] with `expected = 0` and the
    /// current entity attached. HTTP and gRPC creates intentionally do not use
    /// this wrapper; they retain create-or-replace/upsert behavior.
    pub fn create_entity_strict_with_caller(
        &mut self,
        mut req: CreateEntityRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<CreateEntityResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.create_entity_strict_inner(req, Some(caller))
    }

    /// Update an entity, overriding `req.actor` with the caller identity.
    pub fn update_entity_with_caller(
        &mut self,
        mut req: UpdateEntityRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<UpdateEntityResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.update_entity_inner(req, Some(caller))
    }

    /// Merge-patch an entity, overriding `req.actor` with the caller identity.
    pub fn patch_entity_with_caller(
        &mut self,
        mut req: PatchEntityRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<PatchEntityResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.patch_entity_inner(req, Some(caller))
    }

    /// Delete an entity, overriding `req.actor` with the caller identity.
    pub fn delete_entity_with_caller(
        &mut self,
        mut req: DeleteEntityRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<DeleteEntityResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.delete_entity_inner(req, Some(caller))
    }

    /// Commit a transaction attributing the audit entries to the caller.
    ///
    /// Ignores any actor string the client may have supplied on the request
    /// body and uses `caller.actor` as the authoritative identity.
    pub fn commit_transaction_with_caller(
        &mut self,
        tx: crate::transaction::Transaction,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<Vec<axon_core::types::Entity>, AxonError> {
        self.commit_transaction_inner(tx, Some(caller.actor.clone()), attribution, Some(caller))
    }

    /// Create a link, overriding `req.actor` with the caller identity.
    pub fn create_link_with_caller(
        &mut self,
        mut req: CreateLinkRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<CreateLinkResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.create_link(req)
    }

    /// Delete a link, overriding `req.actor` with the caller identity.
    pub fn delete_link_with_caller(
        &mut self,
        mut req: DeleteLinkRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<DeleteLinkResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.delete_link(req)
    }

    /// Drive a lifecycle transition, overriding `req.actor` with the caller.
    pub fn transition_lifecycle_with_caller(
        &mut self,
        mut req: TransitionLifecycleRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<TransitionLifecycleResponse, AxonError> {
        req.actor = Some(caller.actor.clone());
        req.attribution = attribution;
        self.transition_lifecycle(req)
    }

    fn ensure_collection_exists(&self, collection: &CollectionId) -> Result<(), AxonError> {
        let qualified = self.storage.resolve_collection_key(collection)?;
        if self
            .storage
            .collection_registered_in_namespace(&qualified.collection, &qualified.namespace)?
        {
            Ok(())
        } else {
            Err(AxonError::NotFound(collection.to_string()))
        }
    }

    // ── Entity operations ────────────────────────────────────────────────────

    /// Create or replace an entity.
    ///
    /// This method preserves Pattern B create semantics from
    /// `create-semantics.md`: direct handler/HTTP/gRPC creates are upserts
    /// backed by [`StorageAdapter::put`]. Use
    /// [`Self::create_entity_strict_with_caller`] or transaction `create`
    /// operations when duplicate IDs must be rejected.
    pub fn create_entity(
        &mut self,
        req: CreateEntityRequest,
    ) -> Result<CreateEntityResponse, AxonError> {
        self.create_entity_inner(req, None)
    }

    fn create_entity_strict_inner(
        &mut self,
        req: CreateEntityRequest,
        caller: Option<&CallerIdentity>,
    ) -> Result<CreateEntityResponse, AxonError> {
        if let Some(current) = self.storage.get(&req.collection, &req.id)? {
            return Err(AxonError::ConflictingVersion {
                expected: 0,
                actual: current.version,
                current_entity: Some(Box::new(current)),
            });
        }
        self.create_entity_inner(req, caller)
    }

    fn create_entity_inner(
        &mut self,
        req: CreateEntityRequest,
        caller: Option<&CallerIdentity>,
    ) -> Result<CreateEntityResponse, AxonError> {
        let mut req = req;
        // Lifecycle initial-state enforcement (FEAT-015).
        // Auto-populates the lifecycle field with `initial` on create and
        // rejects non-string/unknown-state values so downstream schema
        // validation and audit records see the canonical state.
        let schema = self.storage.get_schema(&req.collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            &req.collection,
            schema.as_ref(),
            req.actor.as_deref(),
            caller,
            req.attribution.as_ref(),
        )?;
        if let Some(schema) = &schema {
            enforce_lifecycle_initial_state(
                schema,
                &mut req.data,
                LifecycleEnforcementMode::Create,
            )?;
        }

        self.enforce_write_policy(
            schema.as_ref(),
            policy_snapshot.as_ref(),
            PolicyWriteCheck {
                collection: &req.collection,
                entity_id: Some(&req.id),
                operation: PolicyOperation::Create,
                current_data: None,
                candidate_data: &req.data,
                field_scope: FieldWriteScope::PresentFields(&req.data),
                operation_index: None,
            },
        )?;

        // Schema validation.
        if let Some(schema) = &schema {
            validate(schema, &req.data)?;
        }

        // Gate evaluation (ESF Layer 5).
        let gate_eval = if let Some(schema) = &schema {
            if schema.validation_rules.is_empty() {
                None
            } else {
                let eval = evaluate_gates(&schema.validation_rules, &schema.gates, &req.data);
                // Save gate blocks persistence.
                if !eval.save_passes() {
                    return Err(AxonError::SchemaValidation(format!(
                        "save gate failed: {}",
                        eval.save_violations
                            .iter()
                            .map(|v| v.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
                Some(eval)
            }
        } else {
            None
        };

        // Unique index constraint check (FEAT-013, US-032).
        if let Some(ref s) = schema {
            check_unique_constraints(&self.storage, &req.collection, &req.id, &req.data, s)?;
        }

        let now = now_ns();
        let mut entity = Entity::new(req.collection, req.id, req.data);
        entity.created_at_ns = Some(now);
        entity.updated_at_ns = Some(now);
        entity.created_by = req.actor.clone();
        entity.updated_by = req.actor.clone();
        entity.schema_version = schema.as_ref().map(|s| s.version);
        // Materialize gate results onto the entity itself before storage
        // write so the persisted blob carries its gate verdicts (FEAT-019).
        if let Some(eval) = gate_eval.as_ref() {
            entity.gate_results = eval.gate_results.clone();
        }
        self.storage.put(entity.clone())?;

        // Index maintenance (FEAT-013).
        if let Some(ref s) = schema {
            if !s.indexes.is_empty() {
                self.storage.update_indexes(
                    &entity.collection,
                    &entity.id,
                    None,
                    &entity.data,
                    &s.indexes,
                )?;
            }
            if !s.compound_indexes.is_empty() {
                self.storage.update_compound_indexes(
                    &entity.collection,
                    &entity.id,
                    None,
                    &entity.data,
                    &s.compound_indexes,
                )?;
            }
        }

        // Audit.
        let mut audit_entry = AuditEntry::new(
            entity.collection.clone(),
            entity.id.clone(),
            entity.version,
            MutationType::EntityCreate,
            None,
            Some(entity.data.clone()),
            req.actor,
        );
        if let Some(meta) = req.audit_metadata {
            audit_entry = audit_entry.with_metadata(meta);
        }
        if let Some(attr) = req.attribution.clone() {
            audit_entry = audit_entry.with_attribution(attr);
        }
        let appended = self.audit.append(audit_entry)?;
        let audit_id = Some(appended.id);

        // Gate results were materialized onto the entity blob above; the
        // response mirrors them (plus advisories) for the caller (FEAT-019).
        let (gates, advisories) = match gate_eval {
            Some(eval) => (eval.gate_results, eval.advisories),
            None => (Default::default(), Vec::new()),
        };

        Ok(CreateEntityResponse {
            entity,
            policy_snapshot,
            gates,
            advisories,
            audit_id,
        })
    }

    pub fn get_entity(&self, req: GetEntityRequest) -> Result<GetEntityResponse, AxonError> {
        self.get_entity_with_read_context(req, None, None)
    }

    pub fn get_entity_with_caller(
        &self,
        req: GetEntityRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<GetEntityResponse, AxonError> {
        self.get_entity_with_read_context(req, Some(caller), attribution.as_ref())
    }

    fn get_entity_with_read_context(
        &self,
        req: GetEntityRequest,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<GetEntityResponse, AxonError> {
        match self.get_visible_entity_for_read_with_context(
            &req.collection,
            &req.id,
            caller,
            attribution,
        )? {
            Some(entity) => {
                let entity = self.redact_entity_for_read_with_context(
                    &req.collection,
                    entity,
                    caller,
                    attribution,
                )?;
                Ok(GetEntityResponse {
                    entity: Self::present_entity(&req.collection, entity),
                })
            }
            None => Err(AxonError::NotFound(req.id.to_string())),
        }
    }

    /// Render a single entity using the collection's stored markdown template.
    ///
    /// Returns [`AxonError::InvalidArgument`] when the collection has no view,
    /// keeping the HTTP surface aligned with FEAT-026's `400 Bad Request`
    /// contract for `format=markdown` without a template.
    pub fn get_entity_markdown(
        &self,
        collection: &CollectionId,
        id: &EntityId,
    ) -> Result<GetEntityMarkdownResponse, AxonError> {
        self.get_entity_markdown_with_read_context(collection, id, None, None)
    }

    pub fn get_entity_markdown_with_caller(
        &self,
        collection: &CollectionId,
        id: &EntityId,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<GetEntityMarkdownResponse, AxonError> {
        self.get_entity_markdown_with_read_context(
            collection,
            id,
            Some(caller),
            attribution.as_ref(),
        )
    }

    fn get_entity_markdown_with_read_context(
        &self,
        collection: &CollectionId,
        id: &EntityId,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<GetEntityMarkdownResponse, AxonError> {
        let entity = self
            .get_visible_entity_for_read_with_context(collection, id, caller, attribution)?
            .ok_or_else(|| AxonError::NotFound(id.to_string()))?;
        let entity =
            self.redact_entity_for_read_with_context(collection, entity, caller, attribution)?;
        let view = self
            .storage
            .get_collection_view(collection)?
            .ok_or_else(|| {
                AxonError::InvalidArgument(format!(
                    "collection '{}' has no markdown template defined",
                    collection
                ))
            })?;

        let render_result = self
            .compiled_markdown_template(collection, &view)
            .map(|template| axon_render::render_compiled(&entity, template.as_ref()));

        Ok(match render_result {
            Ok(rendered_markdown) => GetEntityMarkdownResponse::Rendered {
                entity: Self::present_entity(collection, entity),
                rendered_markdown,
            },
            Err(error) => GetEntityMarkdownResponse::RenderFailed {
                entity: Self::present_entity(collection, entity),
                detail: format!(
                    "failed to render markdown for collection '{}': {error}",
                    collection
                ),
            },
        })
    }

    /// Update an entity using optimistic concurrency control (OCC).
    ///
    /// Fails with [`AxonError::ConflictingVersion`] if `expected_version`
    /// does not match the current stored version.
    pub fn update_entity(
        &mut self,
        req: UpdateEntityRequest,
    ) -> Result<UpdateEntityResponse, AxonError> {
        self.update_entity_inner(req, None)
    }

    fn update_entity_inner(
        &mut self,
        req: UpdateEntityRequest,
        caller: Option<&CallerIdentity>,
    ) -> Result<UpdateEntityResponse, AxonError> {
        let mut req = req;
        // Lifecycle state enforcement (FEAT-015).
        // Updates must already carry a known state at the lifecycle field;
        // unlike create we do not auto-populate so the caller cannot silently
        // elide a state change.
        let schema = self.storage.get_schema(&req.collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            &req.collection,
            schema.as_ref(),
            req.actor.as_deref(),
            caller,
            req.attribution.as_ref(),
        )?;
        if let Some(schema) = &schema {
            enforce_lifecycle_initial_state(
                schema,
                &mut req.data,
                LifecycleEnforcementMode::Update,
            )?;
        }

        // Schema validation.
        if let Some(schema) = &schema {
            validate(schema, &req.data)?;
        }

        // Gate evaluation (ESF Layer 5).
        let gate_eval = if let Some(schema) = &schema {
            if schema.validation_rules.is_empty() {
                None
            } else {
                let eval = evaluate_gates(&schema.validation_rules, &schema.gates, &req.data);
                if !eval.save_passes() {
                    return Err(AxonError::SchemaValidation(format!(
                        "save gate failed: {}",
                        eval.save_violations
                            .iter()
                            .map(|v| v.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
                Some(eval)
            }
        } else {
            None
        };

        // Unique index constraint check (FEAT-013, US-032).
        if let Some(ref s) = schema {
            check_unique_constraints(&self.storage, &req.collection, &req.id, &req.data, s)?;
        }

        // Read current state for the audit `before` snapshot and metadata preservation.
        let existing = self.storage.get(&req.collection, &req.id)?;
        let before = existing.as_ref().map(|e| e.data.clone());

        self.enforce_write_policy(
            schema.as_ref(),
            policy_snapshot.as_ref(),
            PolicyWriteCheck {
                collection: &req.collection,
                entity_id: Some(&req.id),
                operation: PolicyOperation::Update,
                current_data: existing.as_ref().map(|entity| &entity.data),
                candidate_data: &req.data,
                field_scope: FieldWriteScope::PresentFields(&req.data),
                operation_index: None,
            },
        )?;

        // Materialize gate results on the entity itself (FEAT-019).
        let materialized_gates = gate_eval
            .as_ref()
            .map(|eval| eval.gate_results.clone())
            .unwrap_or_default();

        // OCC write: preserve created_at/created_by, update updated_at/updated_by.
        let candidate = Entity {
            collection: req.collection.clone(),
            id: req.id,
            version: req.expected_version, // compare_and_swap bumps this to +1
            data: req.data,
            created_at_ns: existing.as_ref().and_then(|e| e.created_at_ns),
            updated_at_ns: Some(now_ns()),
            created_by: existing.as_ref().and_then(|e| e.created_by.clone()),
            updated_by: req.actor.clone(),
            schema_version: schema.as_ref().map(|s| s.version),
            gate_results: materialized_gates,
        };
        let stored = self
            .storage
            .compare_and_swap(candidate, req.expected_version)?;

        // Index maintenance (FEAT-013).
        if let Some(ref s) = schema {
            if !s.indexes.is_empty() {
                self.storage.update_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.indexes,
                )?;
            }
            if !s.compound_indexes.is_empty() {
                self.storage.update_compound_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.compound_indexes,
                )?;
            }
        }

        let updated = Self::present_entity(&req.collection, stored);

        // Audit.
        let mut audit_entry = AuditEntry::new(
            updated.collection.clone(),
            updated.id.clone(),
            updated.version,
            MutationType::EntityUpdate,
            before,
            Some(updated.data.clone()),
            req.actor,
        );
        if let Some(meta) = req.audit_metadata {
            audit_entry = audit_entry.with_metadata(meta);
        }
        if let Some(attr) = req.attribution.clone() {
            audit_entry = audit_entry.with_attribution(attr);
        }
        let appended = self.audit.append(audit_entry)?;
        let audit_id = Some(appended.id);

        // Gate results were materialized on the entity itself before the
        // storage write (FEAT-019); we only need to surface them (plus
        // advisories) in the response here.
        let (gates, advisories) = match gate_eval {
            Some(eval) => (eval.gate_results, eval.advisories),
            None => (Default::default(), Vec::new()),
        };

        Ok(UpdateEntityResponse {
            entity: updated,
            policy_snapshot,
            gates,
            advisories,
            audit_id,
        })
    }

    /// Partially update an entity using RFC 7396 JSON Merge Patch.
    ///
    /// Reads the current entity, applies the merge patch, validates the result
    /// against the schema, and writes via OCC (`compare_and_swap`).
    pub fn patch_entity(
        &mut self,
        req: PatchEntityRequest,
    ) -> Result<PatchEntityResponse, AxonError> {
        self.patch_entity_inner(req, None)
    }

    fn patch_entity_inner(
        &mut self,
        req: PatchEntityRequest,
        caller: Option<&CallerIdentity>,
    ) -> Result<PatchEntityResponse, AxonError> {
        // Read current entity.
        let existing = self
            .storage
            .get(&req.collection, &req.id)?
            .ok_or_else(|| AxonError::NotFound(req.id.to_string()))?;

        // Apply RFC 7396 merge patch.
        let mut merged = existing.data.clone();
        json_merge_patch(&mut merged, &req.patch);

        // Lifecycle state enforcement (FEAT-015).
        // Patches run the update-mode check on the post-merge payload so
        // callers cannot null-out or corrupt the lifecycle field via patch.
        let schema = self.storage.get_schema(&req.collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            &req.collection,
            schema.as_ref(),
            req.actor.as_deref(),
            caller,
            req.attribution.as_ref(),
        )?;
        if let Some(schema) = &schema {
            enforce_lifecycle_initial_state(schema, &mut merged, LifecycleEnforcementMode::Update)?;
        }

        // Schema validation on the merged result.
        if let Some(schema) = &schema {
            validate(schema, &merged)?;
        }

        // Gate evaluation (ESF Layer 5).
        let gate_eval = if let Some(schema) = &schema {
            if schema.validation_rules.is_empty() {
                None
            } else {
                let eval = evaluate_gates(&schema.validation_rules, &schema.gates, &merged);
                if !eval.save_passes() {
                    return Err(AxonError::SchemaValidation(format!(
                        "save gate failed: {}",
                        eval.save_violations
                            .iter()
                            .map(|v| v.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
                Some(eval)
            }
        } else {
            None
        };

        // Unique index constraint check (FEAT-013, US-032).
        if let Some(ref s) = schema {
            check_unique_constraints(&self.storage, &req.collection, &req.id, &merged, s)?;
        }

        self.enforce_write_policy(
            schema.as_ref(),
            policy_snapshot.as_ref(),
            PolicyWriteCheck {
                collection: &req.collection,
                entity_id: Some(&req.id),
                operation: PolicyOperation::Update,
                current_data: Some(&existing.data),
                candidate_data: &merged,
                field_scope: FieldWriteScope::Patch(&req.patch),
                operation_index: None,
            },
        )?;

        let before = Some(existing.data.clone());

        // Materialize gate results on the entity itself (FEAT-019).
        let materialized_gates = gate_eval
            .as_ref()
            .map(|eval| eval.gate_results.clone())
            .unwrap_or_default();

        // OCC write.
        let candidate = Entity {
            collection: req.collection.clone(),
            id: req.id,
            version: req.expected_version,
            data: merged,
            created_at_ns: existing.created_at_ns,
            updated_at_ns: Some(now_ns()),
            created_by: existing.created_by,
            updated_by: req.actor.clone(),
            schema_version: schema.as_ref().map(|s| s.version),
            gate_results: materialized_gates,
        };
        let stored = self
            .storage
            .compare_and_swap(candidate, req.expected_version)?;

        // Index maintenance (FEAT-013).
        if let Some(ref s) = schema {
            if !s.indexes.is_empty() {
                self.storage.update_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.indexes,
                )?;
            }
            if !s.compound_indexes.is_empty() {
                self.storage.update_compound_indexes(
                    &req.collection,
                    &stored.id,
                    before.as_ref(),
                    &stored.data,
                    &s.compound_indexes,
                )?;
            }
        }

        let updated = Self::present_entity(&req.collection, stored);

        // Audit.
        let mut audit_entry = AuditEntry::new(
            updated.collection.clone(),
            updated.id.clone(),
            updated.version,
            MutationType::EntityUpdate,
            before,
            Some(updated.data.clone()),
            req.actor,
        );
        if let Some(meta) = req.audit_metadata {
            audit_entry = audit_entry.with_metadata(meta);
        }
        if let Some(attr) = req.attribution.clone() {
            audit_entry = audit_entry.with_attribution(attr);
        }
        let appended = self.audit.append(audit_entry)?;
        let audit_id = Some(appended.id);

        // Gate results were materialized on the entity itself before the
        // storage write (FEAT-019); the response mirrors them here.
        let (gates, advisories) = match gate_eval {
            Some(eval) => (eval.gate_results, eval.advisories),
            None => (Default::default(), Vec::new()),
        };

        Ok(PatchEntityResponse {
            entity: updated,
            policy_snapshot,
            gates,
            advisories,
            audit_id,
        })
    }

    pub fn delete_entity(
        &mut self,
        req: DeleteEntityRequest,
    ) -> Result<DeleteEntityResponse, AxonError> {
        self.delete_entity_inner(req, None)
    }

    fn delete_entity_inner(
        &mut self,
        req: DeleteEntityRequest,
        caller: Option<&CallerIdentity>,
    ) -> Result<DeleteEntityResponse, AxonError> {
        let schema = self.storage.get_schema(&req.collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            &req.collection,
            schema.as_ref(),
            req.actor.as_deref(),
            caller,
            req.attribution.as_ref(),
        )?;

        // Referential integrity: reject delete when inbound links exist
        // (unless `force` is set).
        if !req.force {
            let links_rev_col = Link::links_rev_collection();
            let rev_prefix = format!("{}/{}/", req.collection, req.id);
            let rev_start = EntityId::new(&rev_prefix);
            let rev_candidates =
                self.storage
                    .range_scan(&links_rev_col, Some(&rev_start), None, Some(1))?;
            let has_inbound = rev_candidates
                .iter()
                .any(|e| e.id.as_str().starts_with(&rev_prefix));
            if has_inbound {
                return Err(AxonError::InvalidOperation(format!(
                    "entity {}/{} has inbound link(s); delete or re-target those links first, or use force=true",
                    req.collection, req.id
                )));
            }
        }

        // Read current state for the audit `before` snapshot.
        let existing = self.storage.get(&req.collection, &req.id)?;
        let before = existing.as_ref().map(|e| e.data.clone());
        let version = existing.as_ref().map(|e| e.version).unwrap_or(0);

        if let Some(current) = existing.as_ref() {
            self.enforce_write_policy(
                schema.as_ref(),
                policy_snapshot.as_ref(),
                PolicyWriteCheck {
                    collection: &req.collection,
                    entity_id: Some(&req.id),
                    operation: PolicyOperation::Delete,
                    current_data: Some(&current.data),
                    candidate_data: &current.data,
                    field_scope: FieldWriteScope::PresentFields(&current.data),
                    operation_index: None,
                },
            )?;
        }

        // Remove index entries before deleting (FEAT-013).
        if let Some(ref data) = before {
            if let Some(schema) = &schema {
                if !schema.indexes.is_empty() {
                    self.storage.remove_index_entries(
                        &req.collection,
                        &req.id,
                        data,
                        &schema.indexes,
                    )?;
                }
                if !schema.compound_indexes.is_empty() {
                    self.storage.remove_compound_index_entries(
                        &req.collection,
                        &req.id,
                        data,
                        &schema.compound_indexes,
                    )?;
                }
            }
        }

        self.storage.delete(&req.collection, &req.id)?;
        // Gate results live on the entity blob (FEAT-019); deleting the
        // entity removes them implicitly.

        // Audit (only if the entity actually existed).
        let audit_id = if before.is_some() {
            let mut audit_entry = AuditEntry::new(
                req.collection.clone(),
                req.id.clone(),
                version,
                MutationType::EntityDelete,
                before,
                None,
                req.actor,
            );
            if let Some(meta) = req.audit_metadata {
                audit_entry = audit_entry.with_metadata(meta);
            }
            if let Some(attr) = req.attribution.clone() {
                audit_entry = audit_entry.with_attribution(attr);
            }
            let appended = self.audit.append(audit_entry)?;
            Some(appended.id)
        } else {
            None
        };

        Ok(DeleteEntityResponse {
            collection: req.collection.to_string(),
            id: req.id.to_string(),
            policy_snapshot,
            audit_id,
        })
    }

    // ── Entity query ─────────────────────────────────────────────────────────

    /// Query entities in a collection with optional filtering, sorting, and
    /// cursor-based pagination (US-011, FEAT-004).
    ///
    /// V1 uses a full sequential scan; secondary indexes are P1.
    pub fn query_entities(
        &self,
        req: QueryEntitiesRequest,
    ) -> Result<QueryEntitiesResponse, AxonError> {
        self.query_entities_with_read_context(req, None, None)
    }

    pub fn query_entities_with_caller(
        &self,
        req: QueryEntitiesRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<QueryEntitiesResponse, AxonError> {
        self.query_entities_with_read_context(req, Some(caller), attribution.as_ref())
    }

    fn query_entities_with_read_context(
        &self,
        req: QueryEntitiesRequest,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<QueryEntitiesResponse, AxonError> {
        // Reject excessively deep filter trees before any evaluation to prevent
        // stack overflows from client-controlled recursion.
        if let Some(ref f) = req.filter {
            let depth = filter_depth(f);
            if depth > MAX_FILTER_DEPTH {
                return Err(AxonError::InvalidArgument(format!(
                    "filter tree depth {depth} exceeds maximum allowed depth {MAX_FILTER_DEPTH}"
                )));
            }
        }

        // Try index-accelerated lookup (FEAT-013) before falling back to scan.
        let schema = self.storage.get_schema(&req.collection)?;
        let request_index_candidates = try_index_lookup(
            &self.storage,
            &req.collection,
            req.filter.as_ref(),
            schema.as_ref(),
        );
        let policy_snapshot = self.policy_snapshot_for_request(
            &req.collection,
            schema.as_ref(),
            None,
            caller,
            attribution,
        )?;
        let compiled_policy = match schema.as_ref() {
            Some(schema) if policy_snapshot.is_some() => {
                self.compile_policy_plan_for_schema(schema)?
            }
            _ => None,
        };
        let policy_storage_plan = match (&compiled_policy, &policy_snapshot) {
            (Some(plan), Some(snapshot)) => self.plan_read_policy_storage_filters(
                &req.collection,
                schema.as_ref(),
                plan,
                snapshot,
            )?,
            _ => PolicyStoragePlan::default(),
        };

        let index_candidates = combine_candidate_ids(
            request_index_candidates,
            policy_storage_plan.candidate_ids.clone(),
        );

        let post_filter_scope = index_candidates
            .as_ref()
            .map_or_else(|| self.storage.count(&req.collection), |ids| Ok(ids.len()))?;
        if policy_storage_plan.post_filter
            && policy_storage_plan.missing_index.is_some()
            && post_filter_scope > POLICY_POST_FILTER_COST_LIMIT
        {
            let missing_index = policy_storage_plan
                .missing_index
                .clone()
                .unwrap_or_else(|| "unknown".into());
            let mut denial =
                PolicyDenial::new("policy_filter_unindexed", req.collection.to_string());
            denial.missing_index = Some(missing_index);
            denial.cost_limit = Some(POLICY_POST_FILTER_COST_LIMIT);
            denial.candidate_count = Some(post_filter_scope);
            return Err(AxonError::PolicyDenied(Box::new(denial)));
        }

        let all = if let Some(entity_ids) = index_candidates {
            // Fetch entities by ID from the index results.
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&req.collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            // Fallback: full scan.
            self.storage.range_scan(&req.collection, None, None, None)?
        };

        // Pre-compute gate evaluations if any gate filters are present.
        let needs_gates = req.filter.as_ref().is_some_and(has_gate_filter);

        // Apply filter (even if we used an index, there may be additional
        // filter predicates or gate filters that need post-filtering).
        let mut matched = Vec::new();
        for entity in all {
            let request_matches = req.filter.as_ref().map_or(true, |filter| {
                if needs_gates {
                    if let Some(ref schema) = schema {
                        let eval =
                            evaluate_gates(&schema.validation_rules, &schema.gates, &entity.data);
                        apply_filter_with_gates(filter, &entity.data, Some(&eval))
                    } else {
                        apply_filter(filter, &entity.data)
                    }
                } else {
                    apply_filter(filter, &entity.data)
                }
            });
            if !request_matches {
                continue;
            }

            let policy_matches = match (&compiled_policy, &policy_snapshot) {
                (Some(plan), Some(snapshot)) => {
                    self.read_policy_allows_entity(&req.collection, plan, snapshot, &entity)?
                }
                _ => true,
            };
            if policy_matches {
                matched.push(entity);
            }
        }

        // Sort before pagination so cursors are stable.
        if !req.sort.is_empty() {
            matched.sort_by(|a, b| {
                for sf in &req.sort {
                    let va = get_field_value(&a.data, &sf.field);
                    let vb = get_field_value(&b.data, &sf.field);
                    let cmp = compare_values(va, vb);
                    if cmp != std::cmp::Ordering::Equal {
                        return if sf.direction == SortDirection::Asc {
                            cmp
                        } else {
                            cmp.reverse()
                        };
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        let total_count = matched.len();

        // Cursor-based pagination: skip everything up to and including after_id.
        if let Some(ref cursor_id) = req.after_id {
            let pos = matched
                .iter()
                .position(|e| &e.id == cursor_id)
                .ok_or_else(|| {
                    AxonError::InvalidArgument(format!(
                        "cursor entity '{}' not found in result set",
                        cursor_id
                    ))
                })?;
            matched = matched.split_off(pos + 1);
        }

        // Apply limit.
        let limit = req.limit.unwrap_or(usize::MAX);
        let has_more = matched.len() > limit;
        if has_more {
            matched.truncate(limit);
        }

        let next_cursor = if has_more {
            matched.last().map(|e| e.id.to_string())
        } else {
            None
        };

        let entities = if req.count_only {
            vec![]
        } else {
            let mut entities = matched;
            if let (Some(plan), Some(snapshot)) = (&compiled_policy, &policy_snapshot) {
                for entity in &mut entities {
                    self.redact_entity_fields_for_read(plan, snapshot, entity)?;
                }
            }
            Self::present_entities(&req.collection, entities)
        };

        Ok(QueryEntitiesResponse {
            entities,
            total_count,
            next_cursor,
            policy_plan: compiled_policy
                .as_ref()
                .map(|_| policy_storage_plan.diagnostics()),
        })
    }

    // ── Snapshot (US-080, FEAT-004) ──────────────────────────────────────────

    /// Take a consistent point-in-time snapshot of one or more collections.
    ///
    /// Returns matching entities along with an `audit_cursor` that represents
    /// the audit log high-water mark at the moment of the snapshot. Callers can
    /// tail the audit log from `audit_cursor` to discover mutations that
    /// occurred after the snapshot.
    ///
    /// The audit cursor is captured **under the same `&self` reference** as
    /// the entity scan so there is no race window between reading the cursor
    /// and reading entities: no write can interleave between the two reads.
    ///
    /// When `req.collections` is `None`, all collections visible to this
    /// handler are included. Results are ordered by `(collection, entity_id)`
    /// for stable cursor-based pagination.
    ///
    /// # V1 caveat — multi-page consistency
    ///
    /// Multi-page snapshot consistency is only guaranteed for single-threaded,
    /// in-memory use. Concurrent writes between paginated requests can cause a
    /// multi-page snapshot to reflect mixed state (some entities from before a
    /// write, others from after). This is acceptable for V1. Production-grade
    /// multi-page consistency requires storage-level snapshot support and is
    /// deferred to a later release.
    pub fn snapshot_entities(&self, req: SnapshotRequest) -> Result<SnapshotResponse, AxonError> {
        // Capture the audit high-water mark *before* reading entities so the
        // cursor correctly represents "no changes newer than this snapshot".
        // This happens under the same `&self` reference as the entity scan, so
        // no writer can interleave between the cursor read and the data read.
        let audit_cursor = self.audit.entries().last().map(|e| e.id).unwrap_or(0);

        // Resolve the list of collections to scan.
        let collections: Vec<CollectionId> = match req.collections {
            Some(list) => list,
            None => self.storage.list_collections()?,
        };

        // Collect entities from all requested collections into a single
        // ordered stream. Sorting by (collection, id) guarantees deterministic
        // cursor-based pagination across pages and across repeated calls.
        let mut all: Vec<Entity> = Vec::new();
        for collection in &collections {
            let page = self.storage.range_scan(collection, None, None, None)?;
            all.extend(Self::present_entities(collection, page));
        }
        all.sort_by(|a, b| {
            a.collection
                .as_str()
                .cmp(b.collection.as_str())
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });

        // Cursor-based pagination: skip everything up to and including the
        // token from a previous page.
        if let Some(ref token) = req.after_page_token {
            let (cursor_collection, cursor_id) = decode_snapshot_page_token(token)?;
            let pos = all
                .iter()
                .position(|e| {
                    e.collection.as_str() == cursor_collection && e.id.as_str() == cursor_id
                })
                .ok_or_else(|| {
                    AxonError::InvalidArgument(format!(
                        "snapshot page token '{token}' does not match any entity in the snapshot"
                    ))
                })?;
            all = all.split_off(pos + 1);
        }

        // Apply limit.
        let limit = req.limit.unwrap_or(usize::MAX);
        let has_more = all.len() > limit;
        if has_more {
            all.truncate(limit);
        }

        let next_page_token = if has_more {
            all.last()
                .map(|e| encode_snapshot_page_token(e.collection.as_str(), e.id.as_str()))
        } else {
            None
        };

        Ok(SnapshotResponse {
            entities: all,
            audit_cursor,
            next_page_token,
        })
    }

    // ── Aggregation operations (US-062) ────────────────────────────────────────

    /// Count entities with optional filter and GROUP BY.
    pub fn count_entities(
        &self,
        req: CountEntitiesRequest,
    ) -> Result<CountEntitiesResponse, AxonError> {
        // Try index-accelerated lookup (FEAT-013).
        let schema = self.storage.get_schema(&req.collection)?;
        let index_candidates = try_index_lookup(
            &self.storage,
            &req.collection,
            req.filter.as_ref(),
            schema.as_ref(),
        );

        let all = if let Some(entity_ids) = index_candidates {
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&req.collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            self.storage.range_scan(&req.collection, None, None, None)?
        };

        // Apply filter (post-filter for remaining predicates).
        let matched: Vec<&Entity> = all
            .iter()
            .filter(|e| {
                req.filter
                    .as_ref()
                    .map_or(true, |f| apply_filter(f, &e.data))
            })
            .collect();

        let total_count = matched.len();

        // Group by field, if requested.
        let groups = if let Some(ref field) = req.group_by {
            let mut group_map: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for entity in &matched {
                let key = get_field_value(&entity.data, field)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let key_str = match &key {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => "null".into(),
                    other => other.to_string(),
                };
                *group_map.entry(key_str).or_insert(0) += 1;
            }
            group_map
                .into_iter()
                .map(|(key_str, count)| {
                    let key = if key_str == "null" {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(key_str)
                    };
                    CountGroup { key, count }
                })
                .collect()
        } else {
            vec![]
        };

        Ok(CountEntitiesResponse {
            total_count,
            groups,
        })
    }

    /// Compute a numeric aggregation (SUM, AVG, MIN, MAX) over entities.
    pub fn aggregate(&self, req: AggregateRequest) -> Result<AggregateResponse, AxonError> {
        // Try index-accelerated lookup (FEAT-013).
        let schema = self.storage.get_schema(&req.collection)?;
        let index_candidates = try_index_lookup(
            &self.storage,
            &req.collection,
            req.filter.as_ref(),
            schema.as_ref(),
        );

        let all = if let Some(entity_ids) = index_candidates {
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&req.collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            self.storage.range_scan(&req.collection, None, None, None)?
        };

        // Apply filter (post-filter for remaining predicates).
        let matched: Vec<&Entity> = all
            .iter()
            .filter(|e| {
                req.filter
                    .as_ref()
                    .map_or(true, |f| apply_filter(f, &e.data))
            })
            .collect();

        if let Some(ref group_by) = req.group_by {
            // Group by field, then aggregate per group.
            let mut groups: std::collections::BTreeMap<String, Vec<f64>> =
                std::collections::BTreeMap::new();
            for entity in &matched {
                let group_key = get_field_value(&entity.data, group_by)
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Null => "null".into(),
                        other => other.to_string(),
                    })
                    .unwrap_or_else(|| "null".into());
                let val = get_field_value(&entity.data, &req.field).and_then(|v| v.as_f64());
                if let Some(n) = val {
                    groups.entry(group_key).or_default().push(n);
                } else {
                    // Ensure the group exists even if this entity has null for the agg field.
                    groups.entry(group_key).or_default();
                }
            }

            let results = groups
                .into_iter()
                .filter(|(_, vals)| !vals.is_empty())
                .map(|(key_str, vals)| {
                    let value = compute_aggregate(&req.function, &vals);
                    let key = if key_str == "null" {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(key_str)
                    };
                    AggregateGroup {
                        key,
                        value,
                        count: vals.len(),
                    }
                })
                .collect();

            Ok(AggregateResponse { results })
        } else {
            // No GROUP BY — aggregate all matching.
            let values: Vec<f64> = matched
                .iter()
                .filter_map(|e| get_field_value(&e.data, &req.field).and_then(|v| v.as_f64()))
                .collect();

            // Check if we tried to aggregate but found no numeric values and entities exist.
            if values.is_empty() && !matched.is_empty() {
                // Check if the field exists but is non-numeric.
                let has_non_numeric = matched.iter().any(|e| {
                    get_field_value(&e.data, &req.field)
                        .is_some_and(|v| !v.is_number() && !v.is_null())
                });
                if has_non_numeric {
                    return Err(AxonError::InvalidArgument(format!(
                        "field '{}' is not numeric",
                        req.field
                    )));
                }
            }

            if values.is_empty() {
                return Ok(AggregateResponse { results: vec![] });
            }

            let value = compute_aggregate(&req.function, &values);
            Ok(AggregateResponse {
                results: vec![AggregateGroup {
                    key: serde_json::Value::Null,
                    value,
                    count: values.len(),
                }],
            })
        }
    }

    // ── Audit operations ─────────────────────────────────────────────────────

    /// Query the audit log with optional filters and cursor-based pagination.
    pub fn query_audit(&self, req: QueryAuditRequest) -> Result<QueryAuditResponse, AxonError> {
        self.query_audit_with_read_context(req, None, None)
    }

    pub fn query_audit_with_caller(
        &self,
        req: QueryAuditRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<QueryAuditResponse, AxonError> {
        self.query_audit_with_read_context(req, Some(caller), attribution.as_ref())
    }

    fn query_audit_with_read_context(
        &self,
        req: QueryAuditRequest,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<QueryAuditResponse, AxonError> {
        use axon_audit::entry::MutationType as MT;

        let operation: Option<MT> = match req.operation.as_deref() {
            None => None,
            Some("entity.create") => Some(MT::EntityCreate),
            Some("entity.update") => Some(MT::EntityUpdate),
            Some("entity.delete") => Some(MT::EntityDelete),
            Some("entity.revert") => Some(MT::EntityRevert),
            Some("link.create") => Some(MT::LinkCreate),
            Some("link.delete") => Some(MT::LinkDelete),
            Some("collection.create") => Some(MT::CollectionCreate),
            Some("collection.drop") => Some(MT::CollectionDrop),
            Some("template.create") => Some(MT::TemplateCreate),
            Some("template.update") => Some(MT::TemplateUpdate),
            Some("template.delete") => Some(MT::TemplateDelete),
            Some("schema.update") => Some(MT::SchemaUpdate),
            Some("mutation_intent.preview" | "intent.preview") => Some(MT::IntentPreview),
            Some("intent.approve") => Some(MT::IntentApprove),
            Some("intent.reject") => Some(MT::IntentReject),
            Some("intent.expire") => Some(MT::IntentExpire),
            Some("intent.commit") => Some(MT::IntentCommit),
            Some(unknown) => {
                return Err(AxonError::InvalidOperation(format!(
                    "unknown operation type: {unknown}"
                )))
            }
        };

        let query = AuditQuery {
            database: req.database,
            collection: req.collection,
            collection_ids: req.collection_ids,
            entity_id: req.entity_id,
            actor: req.actor,
            operation,
            intent_id: req.intent_id,
            approval_id: req.approval_id,
            since_ns: req.since_ns,
            until_ns: req.until_ns,
            after_id: req.after_id,
            limit: req.limit,
        };

        let page: AuditPage = self.audit.query_paginated(query)?;
        let mut entries = page.entries;
        for entry in &mut entries {
            self.redact_audit_entry_for_read_with_context(entry, caller, attribution)?;
        }
        Ok(QueryAuditResponse {
            entries,
            next_cursor: page.next_cursor,
        })
    }

    pub fn redact_mutation_intent_for_read(
        &self,
        intent: &mut MutationIntent,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<(), AxonError> {
        self.redact_mutation_intent_for_read_with_context(
            intent,
            Some(caller),
            attribution.as_ref(),
        )
    }

    fn redact_mutation_intent_for_read_with_context(
        &self,
        intent: &mut MutationIntent,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<(), AxonError> {
        let operation_kind = intent.operation.operation_kind.clone();
        if let Some(operation) = intent.operation.canonical_operation.as_mut() {
            self.redact_intent_operation_for_read(
                &operation_kind,
                operation,
                Some(&mut intent.review_summary.diff),
                caller,
                attribution,
            )?;
        }
        Ok(())
    }

    fn redact_intent_audit_payload_for_read(
        &self,
        entry: &mut AuditEntry,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<(), AxonError> {
        if !matches!(
            entry.mutation,
            MutationType::IntentPreview
                | MutationType::IntentApprove
                | MutationType::IntentReject
                | MutationType::IntentExpire
                | MutationType::IntentCommit
        ) {
            return Ok(());
        }

        if entry.mutation == MutationType::IntentPreview {
            self.redact_preview_audit_review_summary_for_read(entry, caller, attribution)?;
        }

        for payload in [&mut entry.data_before, &mut entry.data_after]
            .into_iter()
            .flatten()
        {
            let Ok(mut intent) = serde_json::from_value::<MutationIntent>(payload.clone()) else {
                continue;
            };
            self.redact_mutation_intent_for_read_with_context(&mut intent, caller, attribution)?;
            *payload = serde_json::to_value(intent).map_err(|err| {
                AxonError::InvalidOperation(format!("failed to serialize redacted intent: {err}"))
            })?;
        }
        Ok(())
    }

    fn redact_preview_audit_review_summary_for_read(
        &self,
        entry: &mut AuditEntry,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<(), AxonError> {
        let Some(data_after) = entry.data_after.as_mut() else {
            return Ok(());
        };
        let Ok(mut summary) = serde_json::from_value::<MutationReviewSummary>(data_after.clone())
        else {
            return Ok(());
        };

        for affected_record in &summary.affected_records {
            let PreImageBinding::Entity { collection, id, .. } = affected_record else {
                continue;
            };
            let Some(entity) = self.storage.get(collection, id)? else {
                continue;
            };
            let redactions = self.field_read_redactions_for_data(
                collection,
                id,
                &entity.data,
                caller,
                attribution,
            )?;
            apply_review_summary_diff_redactions(&mut summary.diff, &redactions);
        }

        *data_after = serde_json::to_value(summary).map_err(|err| {
            AxonError::InvalidOperation(format!(
                "failed to serialize redacted preview review summary: {err}"
            ))
        })?;
        Ok(())
    }

    fn redact_intent_operation_for_read(
        &self,
        operation_kind: &MutationOperationKind,
        operation: &mut Value,
        diff: Option<&mut Value>,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<(), AxonError> {
        if matches!(operation_kind, MutationOperationKind::Transaction) {
            self.redact_transaction_intent_operation_for_read(
                operation,
                diff,
                caller,
                attribution,
            )?;
            return Ok(());
        }

        let Some((collection, id)) = intent_operation_entity_ref(operation) else {
            return Ok(());
        };
        let read_context = IntentReadContext {
            caller,
            attribution,
        };
        match operation_kind {
            MutationOperationKind::CreateEntity => {
                let after = operation.get("data").cloned();
                self.redact_entity_intent_values_for_read(
                    IntentEntityRedaction {
                        collection: &collection,
                        id: &id,
                        before: None,
                        after: after.as_ref(),
                        operation_data: operation.get_mut("data"),
                        diff,
                    },
                    read_context,
                )?;
            }
            MutationOperationKind::UpdateEntity => {
                let before = self
                    .storage
                    .get(&collection, &id)?
                    .map(|entity| entity.data);
                let after = operation.get("data").cloned();
                self.redact_entity_intent_values_for_read(
                    IntentEntityRedaction {
                        collection: &collection,
                        id: &id,
                        before: before.as_ref(),
                        after: after.as_ref(),
                        operation_data: operation.get_mut("data"),
                        diff,
                    },
                    read_context,
                )?;
            }
            MutationOperationKind::PatchEntity => {
                let before = self
                    .storage
                    .get(&collection, &id)?
                    .map(|entity| entity.data);
                let after = before.as_ref().map(|before| {
                    let mut after = before.clone();
                    if let Some(patch) = operation.get("patch") {
                        json_merge_patch(&mut after, patch);
                    }
                    after
                });
                self.redact_entity_intent_values_for_read(
                    IntentEntityRedaction {
                        collection: &collection,
                        id: &id,
                        before: before.as_ref(),
                        after: after.as_ref(),
                        operation_data: operation.get_mut("patch"),
                        diff,
                    },
                    read_context,
                )?;
            }
            MutationOperationKind::DeleteEntity => {
                let before = self
                    .storage
                    .get(&collection, &id)?
                    .map(|entity| entity.data);
                self.redact_entity_intent_values_for_read(
                    IntentEntityRedaction {
                        collection: &collection,
                        id: &id,
                        before: before.as_ref(),
                        after: None,
                        operation_data: None,
                        diff,
                    },
                    read_context,
                )?;
            }
            MutationOperationKind::Transition => {
                let before = self
                    .storage
                    .get(&collection, &id)?
                    .map(|entity| entity.data);
                let after = before.as_ref().map(|before| {
                    let mut after = before.clone();
                    if let (Some(lifecycle_name), Some(target_state)) = (
                        operation.get("lifecycle_name").and_then(Value::as_str),
                        operation.get("target_state").cloned(),
                    ) {
                        if let Ok(Some(schema)) = self.storage.get_schema(&collection) {
                            if let Some(lifecycle) = schema.lifecycles.get(lifecycle_name) {
                                after[&lifecycle.field] = target_state;
                            }
                        }
                    }
                    after
                });
                self.redact_entity_intent_values_for_read(
                    IntentEntityRedaction {
                        collection: &collection,
                        id: &id,
                        before: before.as_ref(),
                        after: after.as_ref(),
                        operation_data: None,
                        diff,
                    },
                    read_context,
                )?;
            }
            MutationOperationKind::Rollback | MutationOperationKind::Revert => {
                self.redact_entity_intent_values_for_read(
                    IntentEntityRedaction {
                        collection: &collection,
                        id: &id,
                        before: None,
                        after: None,
                        operation_data: operation.get_mut("data"),
                        diff,
                    },
                    read_context,
                )?;
            }
            MutationOperationKind::CreateLink | MutationOperationKind::DeleteLink => {}
            MutationOperationKind::Transaction => {}
        }
        Ok(())
    }

    fn redact_transaction_intent_operation_for_read(
        &self,
        operation: &mut Value,
        diff: Option<&mut Value>,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<(), AxonError> {
        let Some(operations) = operation
            .get_mut("operations")
            .and_then(Value::as_array_mut)
        else {
            return Ok(());
        };
        let mut diff = diff;
        for (index, child) in operations.iter_mut().enumerate() {
            let Some(child_kind) = transaction_child_operation_kind(child) else {
                continue;
            };
            let child_diff = diff
                .as_deref_mut()
                .and_then(|diff| transaction_child_diff_mut(diff, index));
            self.redact_intent_operation_for_read(
                &child_kind,
                child,
                child_diff,
                caller,
                attribution,
            )?;
        }
        Ok(())
    }

    fn redact_entity_intent_values_for_read(
        &self,
        input: IntentEntityRedaction<'_>,
        read_context: IntentReadContext<'_>,
    ) -> Result<(), AxonError> {
        let before_redactions = input
            .before
            .map(|data| {
                self.field_read_redactions_for_data(
                    input.collection,
                    input.id,
                    data,
                    read_context.caller,
                    read_context.attribution,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let after_redactions = input
            .after
            .map(|data| {
                self.field_read_redactions_for_data(
                    input.collection,
                    input.id,
                    data,
                    read_context.caller,
                    read_context.attribution,
                )
            })
            .transpose()?
            .unwrap_or_default();

        if let Some(operation_data) = input.operation_data {
            apply_existing_field_redactions(operation_data, &after_redactions);
        }
        if let Some(diff) = input.diff {
            apply_diff_value_redactions(diff, &before_redactions);
            apply_diff_value_redactions(diff, &after_redactions);
        }
        Ok(())
    }

    fn field_read_redactions_for_data(
        &self,
        collection: &CollectionId,
        id: &EntityId,
        data: &Value,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<Vec<(String, Value)>, AxonError> {
        let schema = self.storage.get_schema(collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            collection,
            schema.as_ref(),
            None,
            caller,
            attribution,
        )?;
        let compiled_policy = match schema.as_ref() {
            Some(schema) if policy_snapshot.is_some() => {
                self.compile_policy_plan_for_schema(schema)?
            }
            _ => None,
        };
        let (Some(plan), Some(snapshot)) = (&compiled_policy, &policy_snapshot) else {
            return Ok(Vec::new());
        };
        let policy_data = policy_data_with_entity_id(id, data);
        self.field_read_redactions(plan, snapshot, &policy_data, None)
    }

    fn redact_audit_entry_for_read_with_context(
        &self,
        entry: &mut AuditEntry,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<(), AxonError> {
        self.redact_intent_audit_payload_for_read(entry, caller, attribution)?;
        let schema = self.storage.get_schema(&entry.collection)?;
        let policy_snapshot = self.policy_snapshot_for_request(
            &entry.collection,
            schema.as_ref(),
            None,
            caller,
            attribution,
        )?;
        let compiled_policy = match schema.as_ref() {
            Some(schema) if policy_snapshot.is_some() => {
                self.compile_policy_plan_for_schema(schema)?
            }
            _ => None,
        };
        let (Some(plan), Some(snapshot)) = (&compiled_policy, &policy_snapshot) else {
            return Ok(());
        };

        let before_redactions = entry
            .data_before
            .as_ref()
            .map(|data| {
                let policy_data = policy_data_with_entity_id(&entry.entity_id, data);
                self.field_read_redactions(plan, snapshot, &policy_data, None)
            })
            .transpose()?
            .unwrap_or_default();
        let after_redactions = entry
            .data_after
            .as_ref()
            .map(|data| {
                let policy_data = policy_data_with_entity_id(&entry.entity_id, data);
                self.field_read_redactions(plan, snapshot, &policy_data, None)
            })
            .transpose()?
            .unwrap_or_default();

        if let Some(data_before) = entry.data_before.as_mut() {
            apply_field_redactions(data_before, &before_redactions);
        }
        if let Some(data_after) = entry.data_after.as_mut() {
            apply_field_redactions(data_after, &after_redactions);
        }
        if let Some(diff) = entry.diff.as_mut() {
            apply_diff_redactions(diff, &before_redactions);
            apply_diff_redactions(diff, &after_redactions);
        }

        Ok(())
    }

    /// Revert an entity to the `before` state recorded in the given audit entry.
    ///
    /// The revert itself produces a new audit entry tagged `EntityRevert` so
    /// the audit log never loses information.
    pub fn revert_entity_to_audit_entry(
        &mut self,
        req: RevertEntityRequest,
    ) -> Result<RevertEntityResponse, AxonError> {
        let source = self
            .audit
            .find_by_id(req.audit_entry_id)?
            .ok_or_else(|| AxonError::NotFound(format!("audit entry {}", req.audit_entry_id)))?;

        let before_data = source.data_before.clone().ok_or_else(|| {
            AxonError::InvalidOperation(format!(
                "audit entry {} has no before state (cannot revert a create)",
                req.audit_entry_id
            ))
        })?;

        // Validate against current schema unless force=true.
        let schema = self.storage.get_schema(&source.collection)?;
        if !req.force {
            if let Some(schema) = &schema {
                validate(schema, &before_data).map_err(|e| {
                    AxonError::SchemaValidation(format!(
                        "before state from audit entry {} does not validate against current schema: {}",
                        req.audit_entry_id, e
                    ))
                })?;
            }
        }

        // Apply the revert: update if entity still exists, recreate if deleted.
        let current = self.storage.get(&source.collection, &source.entity_id)?;
        let data_before_revert = current.as_ref().map(|e| e.data.clone());
        let policy_snapshot = self.policy_snapshot_for_request(
            &source.collection,
            schema.as_ref(),
            req.actor.as_deref(),
            None,
            req.attribution.as_ref(),
        )?;
        let operation = if current.is_some() {
            PolicyOperation::Update
        } else {
            PolicyOperation::Create
        };
        self.enforce_write_policy(
            schema.as_ref(),
            policy_snapshot.as_ref(),
            PolicyWriteCheck {
                collection: &source.collection,
                entity_id: Some(&source.entity_id),
                operation,
                current_data: current.as_ref().map(|entity| &entity.data),
                candidate_data: &before_data,
                field_scope: FieldWriteScope::PresentFields(&before_data),
                operation_index: None,
            },
        )?;
        let restored = match current {
            Some(existing) => {
                let candidate = Entity {
                    collection: source.collection.clone(),
                    id: source.entity_id.clone(),
                    version: existing.version,
                    data: before_data.clone(),
                    created_at_ns: existing.created_at_ns,
                    updated_at_ns: Some(now_ns()),
                    created_by: existing.created_by.clone(),
                    updated_by: req.actor.clone(),
                    schema_version: schema.as_ref().map(|s| s.version),
                    gate_results: Default::default(),
                };
                self.storage.compare_and_swap(candidate, existing.version)?
            }
            None => {
                let mut entity = Entity::new(
                    source.collection.clone(),
                    source.entity_id.clone(),
                    before_data.clone(),
                );
                entity.schema_version = schema.as_ref().map(|s| s.version);
                self.storage.put(entity.clone())?;
                entity
            }
        };

        // Audit the revert.
        let mut revert_entry = AuditEntry::new(
            restored.collection.clone(),
            restored.id.clone(),
            restored.version,
            MutationType::EntityRevert,
            data_before_revert,
            Some(before_data),
            req.actor,
        );
        revert_entry.metadata.insert(
            "reverted_from_entry_id".into(),
            req.audit_entry_id.to_string(),
        );
        if let Some(attr) = req.attribution.clone() {
            revert_entry = revert_entry.with_attribution(attr);
        }

        let appended = self.audit.append(revert_entry)?;

        Ok(RevertEntityResponse {
            entity: restored,
            audit_entry: appended,
        })
    }

    /// Roll an entity back to a prior version or audit entry state.
    ///
    /// The rollback uses the target entry's `data_after` snapshot, validates it
    /// against the current schema and save gate, and writes it as a new
    /// `entity.revert` revision unless `dry_run=true`.
    pub fn rollback_entity(
        &mut self,
        req: RollbackEntityRequest,
    ) -> Result<RollbackEntityResponse, AxonError> {
        struct DeletedEntityContext {
            deleted_version: u64,
            created_at_ns: Option<u64>,
            created_by: Option<String>,
        }

        let source = self.resolve_rollback_source_entry(&req.collection, &req.id, &req.target)?;
        let target_data = source.data_after.clone().ok_or_else(|| match &req.target {
            RollbackEntityTarget::Version(version) => AxonError::NotFound(format!(
                "entity version {} not found in audit log for {}",
                version, req.id
            )),
            RollbackEntityTarget::AuditEntryId(audit_entry_id) => AxonError::NotFound(format!(
                "audit entry {} has no stored entity state",
                audit_entry_id
            )),
        })?;
        let audit_history = self.audit.query_by_entity(&req.collection, &req.id)?;
        let latest_entry = audit_history
            .last()
            .cloned()
            .ok_or_else(|| AxonError::NotFound(req.id.to_string()))?;
        let current = self.storage.get(&req.collection, &req.id)?;
        let deleted_entity_context = if current.is_none() {
            if latest_entry.mutation != MutationType::EntityDelete {
                return Err(AxonError::NotFound(req.id.to_string()));
            }
            let created_entry = audit_history
                .iter()
                .find(|entry| entry.data_after.is_some())
                .cloned()
                .unwrap_or_else(|| latest_entry.clone());
            Some(DeletedEntityContext {
                deleted_version: latest_entry.version,
                created_at_ns: Some(created_entry.timestamp_ns),
                created_by: (created_entry.actor != "anonymous")
                    .then(|| created_entry.actor.clone()),
            })
        } else {
            None
        };
        let (actual_version, created_at_ns, created_by) =
            match (current.as_ref(), deleted_entity_context.as_ref()) {
                (Some(entity), _) => (
                    entity.version,
                    entity.created_at_ns,
                    entity.created_by.clone(),
                ),
                (None, Some(context)) => (
                    context.deleted_version,
                    context.created_at_ns,
                    context.created_by.clone(),
                ),
                (None, None) => return Err(AxonError::NotFound(req.id.to_string())),
            };
        let expected_version = req.expected_version.unwrap_or(actual_version);
        if expected_version != actual_version {
            return Err(AxonError::ConflictingVersion {
                expected: expected_version,
                actual: actual_version,
                current_entity: current
                    .clone()
                    .map(|entity| Box::new(Self::present_entity(&req.collection, entity))),
            });
        }

        let schema = self.storage.get_schema(&req.collection)?;
        if let Some(schema) = &schema {
            validate(schema, &target_data)?;
        }

        // Materialize gate results onto the rollback candidate so the
        // persisted entity blob carries its gate verdicts (FEAT-019).
        let materialized_gates = if let Some(schema) = &schema {
            if schema.validation_rules.is_empty() {
                Default::default()
            } else {
                let eval = evaluate_gates(&schema.validation_rules, &schema.gates, &target_data);
                if !eval.save_passes() {
                    return Err(AxonError::SchemaValidation(format!(
                        "save gate failed: {}",
                        eval.save_violations
                            .iter()
                            .map(|v| v.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; ")
                    )));
                }
                eval.gate_results
            }
        } else {
            Default::default()
        };

        if let Some(ref s) = schema {
            check_unique_constraints(&self.storage, &req.collection, &req.id, &target_data, s)?;
        }

        let target = Entity {
            collection: req.collection.clone(),
            id: req.id.clone(),
            version: actual_version + 1,
            data: target_data.clone(),
            created_at_ns,
            updated_at_ns: current.as_ref().and_then(|entity| entity.updated_at_ns),
            created_by: created_by.clone(),
            updated_by: req.actor.clone(),
            schema_version: schema.as_ref().map(|s| s.version),
            gate_results: materialized_gates.clone(),
        };

        if !req.dry_run {
            let policy_snapshot = self.policy_snapshot_for_request(
                &req.collection,
                schema.as_ref(),
                req.actor.as_deref(),
                None,
                None,
            )?;
            let operation = if current.is_some() {
                PolicyOperation::Update
            } else {
                PolicyOperation::Create
            };
            self.enforce_write_policy(
                schema.as_ref(),
                policy_snapshot.as_ref(),
                PolicyWriteCheck {
                    collection: &req.collection,
                    entity_id: Some(&req.id),
                    operation,
                    current_data: current.as_ref().map(|entity| &entity.data),
                    candidate_data: &target_data,
                    field_scope: FieldWriteScope::PresentFields(&target_data),
                    operation_index: None,
                },
            )?;
        }

        if req.dry_run {
            return Ok(RollbackEntityResponse::DryRun {
                current: current
                    .clone()
                    .map(|entity| Self::present_entity(&req.collection, entity)),
                target,
                diff: current.as_ref().map_or_else(
                    || compute_diff(&json!({}), &target_data),
                    |entity| compute_diff(&entity.data, &target_data),
                ),
            });
        }

        let stored = if let Some(current) = current.as_ref() {
            self.storage.compare_and_swap(
                Entity {
                    collection: req.collection.clone(),
                    id: req.id.clone(),
                    version: expected_version,
                    data: target_data.clone(),
                    created_at_ns: current.created_at_ns,
                    updated_at_ns: Some(now_ns()),
                    created_by: current.created_by.clone(),
                    updated_by: req.actor.clone(),
                    schema_version: schema.as_ref().map(|s| s.version),
                    gate_results: materialized_gates.clone(),
                },
                expected_version,
            )?
        } else {
            let recreated = Entity {
                collection: req.collection.clone(),
                id: req.id.clone(),
                version: expected_version + 1,
                data: target_data.clone(),
                created_at_ns,
                updated_at_ns: Some(now_ns()),
                created_by,
                updated_by: req.actor.clone(),
                schema_version: schema.as_ref().map(|s| s.version),
                gate_results: materialized_gates,
            };
            self.storage.create_if_absent(recreated, expected_version)?
        };

        if let Some(ref s) = schema {
            if !s.indexes.is_empty() {
                self.storage.update_indexes(
                    &req.collection,
                    &stored.id,
                    current.as_ref().map(|entity| &entity.data),
                    &stored.data,
                    &s.indexes,
                )?;
            }
            if !s.compound_indexes.is_empty() {
                self.storage.update_compound_indexes(
                    &req.collection,
                    &stored.id,
                    current.as_ref().map(|entity| &entity.data),
                    &stored.data,
                    &s.compound_indexes,
                )?;
            }
        }

        // Gate results were materialized onto the entity blob above
        // (FEAT-019); no separate side-table write is needed.

        let entity = Self::present_entity(&req.collection, stored);
        let mut audit_entry = AuditEntry::new(
            entity.collection.clone(),
            entity.id.clone(),
            entity.version,
            MutationType::EntityRevert,
            current.as_ref().map(|entity| entity.data.clone()),
            Some(entity.data.clone()),
            req.actor,
        );
        audit_entry
            .metadata
            .insert("reverted_from_entry_id".into(), source.id.to_string());
        let audit_entry = self.audit.append(audit_entry)?;

        Ok(RollbackEntityResponse::Applied {
            entity,
            audit_entry,
        })
    }

    fn resolve_rollback_source_entry(
        &self,
        collection: &CollectionId,
        id: &EntityId,
        target: &RollbackEntityTarget,
    ) -> Result<AuditEntry, AxonError> {
        match target {
            RollbackEntityTarget::Version(version) => self
                .audit
                .query_by_entity(collection, id)?
                .into_iter()
                .find(|entry| entry.version == *version && entry.data_after.is_some())
                .ok_or_else(|| {
                    AxonError::NotFound(format!(
                        "entity version {} not found in audit log for {}",
                        version, id
                    ))
                }),
            RollbackEntityTarget::AuditEntryId(audit_entry_id) => {
                let entry = self.audit.find_by_id(*audit_entry_id)?.ok_or_else(|| {
                    AxonError::NotFound(format!("audit entry {}", audit_entry_id))
                })?;
                if &entry.collection != collection || &entry.entity_id != id {
                    return Err(AxonError::NotFound(format!(
                        "audit entry {} not found for {}/{}",
                        audit_entry_id, collection, id
                    )));
                }
                Ok(entry)
            }
        }
    }

    // ── Collection-level rollback ─────────────────────────────────────────────

    /// Roll back every entity in a collection to its state at a given point in
    /// time. Mutations recorded after `timestamp_ns` are reverted by replaying
    /// each affected entity to its last known state at-or-before the timestamp.
    ///
    /// When `dry_run` is `true`, the method returns a preview without writing.
    pub fn rollback_collection(
        &mut self,
        req: RollbackCollectionRequest,
    ) -> Result<RollbackCollectionResponse, AxonError> {
        // 1. Query the audit log for all entity-level mutations in this
        //    collection that occurred strictly after the target timestamp.
        let page = self.audit.query_paginated(AuditQuery {
            collection: Some(req.collection.clone()),
            since_ns: Some(req.timestamp_ns + 1),
            ..AuditQuery::default()
        })?;

        let entity_mutations: Vec<&AuditEntry> = page
            .entries
            .iter()
            .filter(|e| {
                matches!(
                    e.mutation,
                    MutationType::EntityCreate
                        | MutationType::EntityUpdate
                        | MutationType::EntityDelete
                        | MutationType::EntityRevert
                )
            })
            .collect();

        // 2. Collect the distinct entity IDs that were mutated.
        let mut seen = HashSet::new();
        let affected_entity_ids: Vec<EntityId> = entity_mutations
            .iter()
            .filter_map(|e| {
                if e.entity_id.as_str().is_empty() {
                    None
                } else if seen.insert(e.entity_id.clone()) {
                    Some(e.entity_id.clone())
                } else {
                    None
                }
            })
            .collect();

        let entities_affected = affected_entity_ids.len();

        // 3. For each affected entity, find its state at the target timestamp
        //    and roll it back.
        let mut details = Vec::with_capacity(entities_affected);
        let mut entities_rolled_back: usize = 0;
        let mut errors: usize = 0;

        for entity_id in &affected_entity_ids {
            let result = self.rollback_single_entity_to_timestamp(
                &req.collection,
                entity_id,
                req.timestamp_ns,
                req.actor.as_deref(),
                req.dry_run,
            );

            match result {
                Ok(()) => {
                    entities_rolled_back += 1;
                    details.push(RollbackCollectionEntityResult {
                        id: entity_id.to_string(),
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    errors += 1;
                    details.push(RollbackCollectionEntityResult {
                        id: entity_id.to_string(),
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        Ok(RollbackCollectionResponse {
            entities_affected,
            entities_rolled_back,
            errors,
            dry_run: req.dry_run,
            details,
        })
    }

    /// Roll a single entity back to its state at the given timestamp.
    ///
    /// If the entity did not exist at the timestamp (i.e., it was created after
    /// the timestamp), it is deleted. If it existed at the timestamp, it is
    /// restored to the snapshot recorded in the last audit entry at-or-before
    /// the timestamp.
    fn rollback_single_entity_to_timestamp(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        timestamp_ns: u64,
        actor: Option<&str>,
        dry_run: bool,
    ) -> Result<(), AxonError> {
        let history = self.audit.query_by_entity(collection, entity_id)?;

        // Find the last audit entry whose timestamp is <= the target timestamp.
        // That entry's `data_after` is the entity state at that point in time.
        let state_at_timestamp: Option<&AuditEntry> = history
            .iter()
            .rev()
            .find(|entry| entry.timestamp_ns <= timestamp_ns);

        let current = self.storage.get(collection, entity_id)?;

        match state_at_timestamp {
            Some(target_entry) => {
                // Entity existed at the timestamp — restore its data_after snapshot.
                let target_data = match &target_entry.data_after {
                    Some(data) => data.clone(),
                    None => {
                        // The entry is a delete — entity should not exist at this point.
                        // If it currently exists, delete it.
                        if current.is_some() && !dry_run {
                            self.storage.delete(collection, entity_id)?;
                            let mut audit_entry = AuditEntry::new(
                                collection.clone(),
                                entity_id.clone(),
                                current.as_ref().map_or(0, |e| e.version) + 1,
                                MutationType::EntityDelete,
                                current.map(|e| e.data),
                                None,
                                actor.map(String::from),
                            );
                            audit_entry
                                .metadata
                                .insert("collection_rollback".into(), "true".into());
                            audit_entry
                                .metadata
                                .insert("rollback_timestamp_ns".into(), timestamp_ns.to_string());
                            self.audit.append(audit_entry)?;
                        }
                        return Ok(());
                    }
                };

                if let Some(ref cur) = current {
                    // Entity exists — check if it already matches.
                    if cur.data == target_data {
                        return Ok(());
                    }
                }

                if dry_run {
                    return Ok(());
                }

                // Write the restored state.
                let schema = self.storage.get_schema(collection)?;
                match current {
                    Some(existing) => {
                        let restored = Entity {
                            collection: collection.clone(),
                            id: entity_id.clone(),
                            version: existing.version,
                            data: target_data.clone(),
                            created_at_ns: existing.created_at_ns,
                            updated_at_ns: Some(now_ns()),
                            created_by: existing.created_by.clone(),
                            updated_by: actor.map(String::from),
                            schema_version: schema.as_ref().map(|s| s.version),
                            gate_results: Default::default(),
                        };
                        let stored = self.storage.compare_and_swap(restored, existing.version)?;
                        let mut audit_entry = AuditEntry::new(
                            collection.clone(),
                            entity_id.clone(),
                            stored.version,
                            MutationType::EntityRevert,
                            Some(existing.data),
                            Some(target_data),
                            actor.map(String::from),
                        );
                        audit_entry
                            .metadata
                            .insert("collection_rollback".into(), "true".into());
                        audit_entry
                            .metadata
                            .insert("rollback_timestamp_ns".into(), timestamp_ns.to_string());
                        self.audit.append(audit_entry)?;
                    }
                    None => {
                        // Entity was deleted — recreate it.
                        let mut entity =
                            Entity::new(collection.clone(), entity_id.clone(), target_data.clone());
                        entity.schema_version = schema.as_ref().map(|s| s.version);
                        entity.updated_by = actor.map(String::from);
                        self.storage.put(entity.clone())?;
                        let mut audit_entry = AuditEntry::new(
                            collection.clone(),
                            entity_id.clone(),
                            entity.version,
                            MutationType::EntityRevert,
                            None,
                            Some(target_data),
                            actor.map(String::from),
                        );
                        audit_entry
                            .metadata
                            .insert("collection_rollback".into(), "true".into());
                        audit_entry
                            .metadata
                            .insert("rollback_timestamp_ns".into(), timestamp_ns.to_string());
                        self.audit.append(audit_entry)?;
                    }
                }
            }
            None => {
                // Entity did not exist at the target timestamp — it was created after.
                // If it currently exists, delete it.
                if let Some(existing) = current {
                    if dry_run {
                        return Ok(());
                    }
                    self.storage.delete(collection, entity_id)?;
                    let mut audit_entry = AuditEntry::new(
                        collection.clone(),
                        entity_id.clone(),
                        existing.version + 1,
                        MutationType::EntityDelete,
                        Some(existing.data),
                        None,
                        actor.map(String::from),
                    );
                    audit_entry
                        .metadata
                        .insert("collection_rollback".into(), "true".into());
                    audit_entry
                        .metadata
                        .insert("rollback_timestamp_ns".into(), timestamp_ns.to_string());
                    self.audit.append(audit_entry)?;
                }
            }
        }

        Ok(())
    }

    // ── Transaction-level rollback ──────────────────────────────────────────

    /// Roll back all mutations from a specific transaction.
    ///
    /// Finds all audit entries sharing the given `transaction_id` and, for each
    /// affected entity, reverts it to its pre-transaction state (the
    /// `data_before` snapshot recorded in the audit entry).
    ///
    /// - Entities that were *created* by the transaction are deleted.
    /// - Entities that were *updated* by the transaction are restored to their
    ///   `data_before` snapshot.
    /// - Entities that were *deleted* by the transaction are recreated from
    ///   their `data_before` snapshot.
    ///
    /// When `dry_run` is `true`, a preview of the affected entities is returned
    /// without modifying data.
    pub fn rollback_transaction(
        &mut self,
        req: RollbackTransactionRequest,
    ) -> Result<RollbackTransactionResponse, AxonError> {
        // 1. Find all audit entries from the target transaction.
        let tx_entries = self.audit.query_by_transaction_id(&req.transaction_id)?;

        if tx_entries.is_empty() {
            return Err(AxonError::NotFound(format!(
                "transaction '{}'",
                req.transaction_id
            )));
        }

        // 2. Collect the distinct (collection, entity_id) pairs.
        //    Process in reverse order so we see the *last* mutation per entity
        //    in the transaction (a transaction might create then update the same entity).
        let mut seen = HashSet::new();
        let mut entity_entries: Vec<&AuditEntry> = Vec::new();
        for entry in tx_entries.iter().rev() {
            let key = (entry.collection.clone(), entry.entity_id.clone());
            if seen.insert(key) {
                entity_entries.push(entry);
            }
        }
        // Reverse back to original order for consistent processing.
        entity_entries.reverse();

        let entities_affected = entity_entries.len();
        let mut details = Vec::with_capacity(entities_affected);
        let mut entities_rolled_back: usize = 0;
        let mut errors: usize = 0;

        // 3. For each affected entity, find its first mutation in the transaction
        //    (to get the pre-transaction data_before) and revert.
        for last_entry in &entity_entries {
            let collection = &last_entry.collection;
            let entity_id = &last_entry.entity_id;

            // Find the *first* entry for this entity in the transaction —
            // its `data_before` is the pre-transaction state.
            let first_entry = tx_entries
                .iter()
                .find(|e| &e.collection == collection && &e.entity_id == entity_id);

            let first_entry = match first_entry {
                Some(e) => e,
                None => {
                    errors += 1;
                    details.push(RollbackTransactionEntityResult {
                        collection: collection.to_string(),
                        id: entity_id.to_string(),
                        success: false,
                        error: Some("no audit entry found for entity in transaction".into()),
                    });
                    continue;
                }
            };

            let result = self.rollback_single_entity_from_transaction(
                collection,
                entity_id,
                first_entry,
                &req.transaction_id,
                req.actor.as_deref(),
                req.dry_run,
            );

            match result {
                Ok(()) => {
                    entities_rolled_back += 1;
                    details.push(RollbackTransactionEntityResult {
                        collection: collection.to_string(),
                        id: entity_id.to_string(),
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    errors += 1;
                    details.push(RollbackTransactionEntityResult {
                        collection: collection.to_string(),
                        id: entity_id.to_string(),
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        Ok(RollbackTransactionResponse {
            transaction_id: req.transaction_id,
            entities_affected,
            entities_rolled_back,
            errors,
            dry_run: req.dry_run,
            details,
        })
    }

    /// Roll a single entity back to its pre-transaction state.
    ///
    /// `first_tx_entry` is the first audit entry for this entity in the transaction.
    /// Its `data_before` represents the entity's state before the transaction began.
    fn rollback_single_entity_from_transaction(
        &mut self,
        collection: &CollectionId,
        entity_id: &EntityId,
        first_tx_entry: &AuditEntry,
        transaction_id: &str,
        actor: Option<&str>,
        dry_run: bool,
    ) -> Result<(), AxonError> {
        let current = self.storage.get(collection, entity_id)?;

        match first_tx_entry.mutation {
            MutationType::EntityCreate => {
                // The transaction created this entity — delete it to revert.
                if current.is_none() {
                    // Already absent — nothing to do.
                    return Ok(());
                }
                if dry_run {
                    return Ok(());
                }
                self.storage.delete(collection, entity_id)?;
                let mut audit_entry = AuditEntry::new(
                    collection.clone(),
                    entity_id.clone(),
                    current.as_ref().map_or(0, |e| e.version) + 1,
                    MutationType::EntityDelete,
                    current.map(|e| e.data),
                    None,
                    actor.map(String::from),
                );
                audit_entry
                    .metadata
                    .insert("transaction_rollback".into(), "true".into());
                audit_entry
                    .metadata
                    .insert("rolled_back_transaction_id".into(), transaction_id.into());
                self.audit.append(audit_entry)?;
            }
            MutationType::EntityUpdate | MutationType::EntityRevert => {
                // The transaction updated this entity — restore data_before.
                let target_data = match &first_tx_entry.data_before {
                    Some(data) => data.clone(),
                    None => {
                        return Err(AxonError::InvalidOperation(format!(
                            "audit entry {} has no data_before snapshot to restore",
                            first_tx_entry.id
                        )));
                    }
                };

                if let Some(ref cur) = current {
                    if cur.data == target_data {
                        return Ok(());
                    }
                }

                if dry_run {
                    return Ok(());
                }

                let schema = self.storage.get_schema(collection)?;
                match current {
                    Some(existing) => {
                        let restored = Entity {
                            collection: collection.clone(),
                            id: entity_id.clone(),
                            version: existing.version,
                            data: target_data.clone(),
                            created_at_ns: existing.created_at_ns,
                            updated_at_ns: Some(now_ns()),
                            created_by: existing.created_by.clone(),
                            updated_by: actor.map(String::from),
                            schema_version: schema.as_ref().map(|s| s.version),
                            gate_results: Default::default(),
                        };
                        let stored = self.storage.compare_and_swap(restored, existing.version)?;
                        let mut audit_entry = AuditEntry::new(
                            collection.clone(),
                            entity_id.clone(),
                            stored.version,
                            MutationType::EntityRevert,
                            Some(existing.data),
                            Some(target_data),
                            actor.map(String::from),
                        );
                        audit_entry
                            .metadata
                            .insert("transaction_rollback".into(), "true".into());
                        audit_entry
                            .metadata
                            .insert("rolled_back_transaction_id".into(), transaction_id.into());
                        self.audit.append(audit_entry)?;
                    }
                    None => {
                        // Entity was deleted after the transaction (or is otherwise
                        // absent) — recreate it from the pre-transaction snapshot.
                        let mut entity =
                            Entity::new(collection.clone(), entity_id.clone(), target_data.clone());
                        entity.schema_version = schema.as_ref().map(|s| s.version);
                        entity.updated_by = actor.map(String::from);
                        self.storage.put(entity.clone())?;
                        let mut audit_entry = AuditEntry::new(
                            collection.clone(),
                            entity_id.clone(),
                            entity.version,
                            MutationType::EntityRevert,
                            None,
                            Some(target_data),
                            actor.map(String::from),
                        );
                        audit_entry
                            .metadata
                            .insert("transaction_rollback".into(), "true".into());
                        audit_entry
                            .metadata
                            .insert("rolled_back_transaction_id".into(), transaction_id.into());
                        self.audit.append(audit_entry)?;
                    }
                }
            }
            MutationType::EntityDelete => {
                // The transaction deleted this entity — recreate from data_before.
                let target_data = match &first_tx_entry.data_before {
                    Some(data) => data.clone(),
                    None => {
                        return Err(AxonError::InvalidOperation(format!(
                            "audit entry {} has no data_before snapshot to restore",
                            first_tx_entry.id
                        )));
                    }
                };

                if current.is_some() {
                    // Entity already exists (perhaps recreated by another operation).
                    return Ok(());
                }

                if dry_run {
                    return Ok(());
                }

                let schema = self.storage.get_schema(collection)?;
                let mut entity =
                    Entity::new(collection.clone(), entity_id.clone(), target_data.clone());
                entity.schema_version = schema.as_ref().map(|s| s.version);
                entity.updated_by = actor.map(String::from);
                self.storage.put(entity.clone())?;
                let mut audit_entry = AuditEntry::new(
                    collection.clone(),
                    entity_id.clone(),
                    entity.version,
                    MutationType::EntityRevert,
                    None,
                    Some(target_data),
                    actor.map(String::from),
                );
                audit_entry
                    .metadata
                    .insert("transaction_rollback".into(), "true".into());
                audit_entry
                    .metadata
                    .insert("rolled_back_transaction_id".into(), transaction_id.into());
                self.audit.append(audit_entry)?;
            }
            _ => {
                // Non-entity mutations (collection create/drop, schema, etc.)
                // are not handled by transaction rollback.
            }
        }

        Ok(())
    }

    // ── Collection lifecycle ─────────────────────────────────────────────────

    /// Validate a collection name against naming rules.
    ///
    /// Names must be 1-128 characters, start with a lowercase letter, and
    /// contain only lowercase letters, digits, hyphens, and underscores.
    /// Internal pseudo-collections beginning with `__` are exempt.
    fn validate_collection_name(name: &CollectionId) -> Result<(), AxonError> {
        let s = name.as_str();

        // Internal pseudo-collections are exempt from user-facing naming rules.
        if s.starts_with("__") {
            return Ok(());
        }

        if s.is_empty() || s.len() > 128 {
            return Err(AxonError::InvalidArgument(format!(
                "collection name '{}' must be 1-128 characters",
                s
            )));
        }

        let mut chars = s.chars();
        let Some(first) = chars.next() else {
            return Err(AxonError::InvalidArgument(format!(
                "collection name '{}' must be 1-128 characters",
                s
            )));
        };
        if !first.is_ascii_lowercase() {
            return Err(AxonError::InvalidArgument(format!(
                "collection name '{}' must start with a lowercase letter",
                s
            )));
        }

        for c in chars {
            if !matches!(c, 'a'..='z' | '0'..='9' | '-' | '_') {
                return Err(AxonError::InvalidArgument(format!(
                    "collection name '{}' contains invalid character '{}'; \
                     only lowercase letters, digits, hyphens, and underscores are allowed",
                    s, c
                )));
            }
        }

        Ok(())
    }

    /// Explicitly register a named collection and record the event in the audit log.
    ///
    /// A schema must be provided at creation time; schemaless collections are not supported.
    ///
    /// Returns [`AxonError::InvalidArgument`] if the name violates naming rules or the schema's
    /// `collection` field does not match `req.name`.
    /// Returns [`AxonError::AlreadyExists`] if the collection has already been created.
    pub fn create_collection(
        &mut self,
        req: CreateCollectionRequest,
    ) -> Result<CreateCollectionResponse, AxonError> {
        let (namespace, bare_name) = Namespace::parse(req.name.as_str());
        let bare_collection = CollectionId::new(&bare_name);

        Self::validate_collection_name(&bare_collection)?;

        if req.schema.collection != req.name {
            return Err(AxonError::InvalidArgument(format!(
                "schema.collection '{}' does not match collection name '{}'",
                req.schema.collection, req.name
            )));
        }

        // Validate entity_schema before any mutations so a bad schema never
        // leaves an orphan (schemaless) collection registration.
        if let Some(entity_schema) = &req.schema.entity_schema {
            compile_entity_schema(entity_schema)?;
        }

        if self
            .storage
            .collection_registered_in_namespace(&bare_collection, &namespace)?
        {
            return Err(AxonError::AlreadyExists(req.name.to_string()));
        }
        self.storage
            .register_collection_in_namespace(&bare_collection, &namespace)?;
        self.put_schema(req.schema)?;

        self.audit.append(AuditEntry::new(
            req.name.clone(),
            EntityId::new(""),
            0,
            MutationType::CollectionCreate,
            None,
            None,
            req.actor,
        ))?;

        Ok(CreateCollectionResponse {
            name: req.name.to_string(),
        })
    }

    /// Drop a collection, removing all its entities, and record the event in the audit log.
    ///
    /// Returns [`AxonError::NotFound`] if the collection was never created via
    /// [`create_collection`].
    pub fn drop_collection(
        &mut self,
        req: DropCollectionRequest,
    ) -> Result<DropCollectionResponse, AxonError> {
        if !req.confirm {
            return Err(AxonError::InvalidArgument(
                "drop_collection requires confirm=true to acknowledge the destructive operation"
                    .into(),
            ));
        }
        let qualified = self.storage.resolve_collection_key(&req.name)?;
        if !self
            .storage
            .collection_registered_in_namespace(&qualified.collection, &qualified.namespace)?
        {
            return Err(AxonError::NotFound(req.name.to_string()));
        }

        // Remove all entities in the collection.
        let entities = self.storage.range_scan(&req.name, None, None, None)?;
        let count = entities.len();
        for entity in &entities {
            self.storage.delete(&req.name, &entity.id)?;
        }
        self.storage.delete_schema(&req.name)?;
        self.storage.delete_collection_view(&req.name)?;
        self.invalidate_markdown_template(&req.name)?;
        self.storage.unregister_collection(&req.name)?;

        let mut drop_meta = std::collections::HashMap::new();
        drop_meta.insert("entities_removed".into(), count.to_string());
        self.audit.append(
            AuditEntry::new(
                req.name.clone(),
                EntityId::new(""),
                0,
                MutationType::CollectionDrop,
                None,
                None,
                req.actor,
            )
            .with_metadata(drop_meta),
        )?;

        Ok(DropCollectionResponse {
            name: req.name.to_string(),
            entities_removed: count,
        })
    }

    fn append_collection_drop_audit_entries(
        &mut self,
        collections: &[CollectionId],
    ) -> Result<(), AxonError> {
        for collection in collections {
            self.audit.append(AuditEntry::new(
                collection.clone(),
                EntityId::new(""),
                0,
                MutationType::CollectionDrop,
                None,
                None,
                None,
            ))?;
        }
        Ok(())
    }

    /// List all explicitly created collections with summary metadata.
    pub fn list_collections(
        &self,
        _req: ListCollectionsRequest,
    ) -> Result<ListCollectionsResponse, AxonError> {
        // Storage returns names already sorted ascending.
        let names = self.storage.list_collections()?;
        let collections: Vec<CollectionMetadata> = names
            .iter()
            .map(|name| {
                let entity_count = self.storage.count(name).unwrap_or(0);
                let schema_version = self
                    .storage
                    .get_schema(name)
                    .ok()
                    .flatten()
                    .map(|s| s.version);
                let (created_at_ns, updated_at_ns) =
                    self.collection_timestamps(name).unwrap_or((None, None));
                CollectionMetadata {
                    name: name.to_string(),
                    entity_count,
                    schema_version,
                    created_at_ns,
                    updated_at_ns,
                }
            })
            .collect();

        Ok(ListCollectionsResponse { collections })
    }

    /// Describe a single collection (entity count + full schema + timestamps).
    ///
    /// Returns [`AxonError::NotFound`] if the collection was not explicitly created.
    pub fn describe_collection(
        &self,
        req: DescribeCollectionRequest,
    ) -> Result<DescribeCollectionResponse, AxonError> {
        self.ensure_collection_exists(&req.name)?;

        let entity_count = self.storage.count(&req.name)?;
        let schema = self.storage.get_schema(&req.name)?;
        let (created_at_ns, updated_at_ns) = self
            .collection_timestamps(&req.name)
            .unwrap_or((None, None));

        Ok(DescribeCollectionResponse {
            name: req.name.to_string(),
            entity_count,
            schema,
            created_at_ns,
            updated_at_ns,
        })
    }

    /// Store or replace the markdown template for a collection.
    pub fn put_collection_template(
        &mut self,
        req: PutCollectionTemplateRequest,
    ) -> Result<PutCollectionTemplateResponse, AxonError> {
        self.ensure_collection_exists(&req.collection)?;
        let before_view = self.storage.get_collection_view(&req.collection)?;
        axon_render::compile(req.template.clone())?;

        let schema = self.storage.get_schema(&req.collection)?;
        let validation = axon_render::validate_template(
            &req.template,
            schema
                .as_ref()
                .and_then(|schema| schema.entity_schema.as_ref()),
        );
        if !validation.is_valid() {
            let detail = validation
                .errors
                .iter()
                .map(|error| error.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(AxonError::SchemaValidation(detail));
        }

        let mut view = CollectionView::new(req.collection.clone(), req.template);
        view.updated_by = req.actor.clone();

        let view = Self::present_collection_view(
            &req.collection,
            self.storage.put_collection_view(&view)?,
        );
        self.invalidate_markdown_template(&req.collection)?;
        self.audit.append(AuditEntry::new(
            req.collection.clone(),
            EntityId::new(""),
            u64::from(view.version),
            if before_view.is_some() {
                MutationType::TemplateUpdate
            } else {
                MutationType::TemplateCreate
            },
            before_view
                .map(|view| Self::collection_view_audit_state(&req.collection, view))
                .transpose()?,
            Some(serde_json::to_value(view.clone())?),
            req.actor,
        ))?;

        Ok(PutCollectionTemplateResponse {
            view,
            warnings: validation
                .warnings
                .into_iter()
                .map(|warning| warning.message)
                .collect(),
        })
    }

    /// Retrieve the current markdown template for a collection.
    pub fn get_collection_template(
        &self,
        req: GetCollectionTemplateRequest,
    ) -> Result<GetCollectionTemplateResponse, AxonError> {
        self.ensure_collection_exists(&req.collection)?;
        let view = self
            .storage
            .get_collection_view(&req.collection)?
            .ok_or_else(|| {
                AxonError::NotFound(format!(
                    "collection '{}' has no markdown template defined",
                    req.collection
                ))
            })?;
        Ok(GetCollectionTemplateResponse {
            view: Self::present_collection_view(&req.collection, view),
        })
    }

    /// Delete the current markdown template for a collection.
    pub fn delete_collection_template(
        &mut self,
        req: DeleteCollectionTemplateRequest,
    ) -> Result<DeleteCollectionTemplateResponse, AxonError> {
        self.ensure_collection_exists(&req.collection)?;
        let before_view = self.storage.get_collection_view(&req.collection)?;
        self.storage.delete_collection_view(&req.collection)?;
        self.invalidate_markdown_template(&req.collection)?;
        if let Some(before_view) = before_view {
            let version = before_view.version;
            self.audit.append(AuditEntry::new(
                req.collection.clone(),
                EntityId::new(""),
                u64::from(version),
                MutationType::TemplateDelete,
                Some(Self::collection_view_audit_state(
                    &req.collection,
                    before_view,
                )?),
                None,
                req.actor,
            ))?;
        }
        Ok(DeleteCollectionTemplateResponse {
            collection: req.collection.to_string(),
        })
    }

    /// Derive created_at and updated_at timestamps for a collection from the
    /// audit log. Returns `(created_at_ns, updated_at_ns)`.
    fn collection_timestamps(
        &self,
        collection: &CollectionId,
    ) -> Result<(Option<u64>, Option<u64>), AxonError> {
        let page = self.audit.query_paginated(AuditQuery {
            collection: Some(collection.clone()),
            ..Default::default()
        })?;
        let created_at_ns = page.entries.first().map(|e| e.timestamp_ns);
        let updated_at_ns = page.entries.last().map(|e| e.timestamp_ns);
        Ok((created_at_ns, updated_at_ns))
    }

    // ── Schema operations ────────────────────────────────────────────────────

    /// Persist or replace the schema for a collection.
    ///
    /// The `schema.collection` field must match the collection name in the
    /// request. Subsequent entity creates and updates will be validated against
    /// this schema.
    pub fn handle_put_schema(
        &mut self,
        req: PutSchemaRequest,
    ) -> Result<PutSchemaResponse, AxonError> {
        let collection = req.schema.collection.clone();

        // Compatibility check against existing schema.
        let existing = self.storage.get_schema(&collection)?;
        let old_entity_schema = existing.as_ref().and_then(|s| s.entity_schema.as_ref());
        let new_entity_schema = req.schema.entity_schema.as_ref();
        let diff = axon_schema::diff_schemas(old_entity_schema, new_entity_schema);
        let compatibility = axon_schema::classify(&diff);
        let policy_compile_report = self.policy_compile_report_for_schema(&req.schema)?;
        let compile_report = self.compile_report_for_schema(&req.schema)?;
        let warnings = axon_schema::schema::json_ld_reserved_field_warnings(&req.schema);

        // Dry-run: return classification without applying. Policy compile
        // failures ride the report instead of bubbling so the admin UI can
        // focus the first actionable error. When the caller supplies fixture
        // explain inputs and the proposed policy compiled, we evaluate them
        // against the proposed plan and surface the explanations alongside
        // the compile report.
        if req.dry_run {
            let dry_run_explanations = self.dry_run_explanations_for_proposed(
                &req.schema,
                &policy_compile_report,
                &req.explain_inputs,
                req.actor.as_deref(),
            )?;
            return Ok(PutSchemaResponse {
                schema: req.schema,
                compatibility: Some(compatibility),
                diff: Some(diff),
                policy_compile_report: Some(policy_compile_report),
                compile_report: Some(compile_report),
                warnings,
                dry_run_explanations,
                dry_run: true,
            });
        }

        // Activation gate: refuse to persist when the proposed access_control
        // failed to compile. The active schema/policy version must remain
        // unchanged; no audit entry is appended.
        if !policy_compile_report.errors.is_empty() {
            let summary = policy_compile_report
                .errors
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(AxonError::SchemaValidation(format!(
                "policy_compile_failed: {summary}"
            )));
        }

        if !compile_report.is_success() {
            return Err(AxonError::SchemaValidation(format!(
                "query_compile_failed: {}",
                query_compile_summary(&compile_report)
            )));
        }

        // Breaking changes require force flag.
        if compatibility == axon_schema::Compatibility::Breaking && !req.force {
            return Err(AxonError::InvalidOperation(format!(
                "schema change is breaking ({}). Use force=true to apply. Changes: {}",
                diff.changes.len(),
                diff.changes
                    .iter()
                    .map(|c| c.description.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            )));
        }

        self.put_schema(req.schema.clone())?;

        // Build audit metadata with compatibility classification and impact counts.
        let mut audit_meta = HashMap::new();
        let compat_str = match &compatibility {
            axon_schema::Compatibility::Compatible => "compatible",
            axon_schema::Compatibility::Breaking => "breaking",
            axon_schema::Compatibility::MetadataOnly => "metadata_only",
        };
        audit_meta.insert("compatibility".to_string(), compat_str.to_string());

        let fields_added = diff
            .changes
            .iter()
            .filter(|c| c.kind == axon_schema::FieldChangeKind::Added)
            .count();
        let fields_removed = diff
            .changes
            .iter()
            .filter(|c| c.kind == axon_schema::FieldChangeKind::Removed)
            .count();
        let fields_modified = diff.changes.len() - fields_added - fields_removed;

        audit_meta.insert("fields_added".to_string(), fields_added.to_string());
        audit_meta.insert("fields_removed".to_string(), fields_removed.to_string());
        audit_meta.insert("fields_modified".to_string(), fields_modified.to_string());
        audit_meta.insert("total_changes".to_string(), diff.changes.len().to_string());
        audit_meta.insert(
            "old_schema_version".to_string(),
            existing
                .as_ref()
                .map(|schema| schema.version.to_string())
                .unwrap_or_else(|| "none".into()),
        );
        audit_meta.insert(
            "new_schema_version".to_string(),
            req.schema.version.to_string(),
        );
        audit_meta.insert(
            "old_policy_version".to_string(),
            existing
                .as_ref()
                .and_then(|schema| schema.access_control.as_ref().map(|_| schema.version))
                .map(|version| version.to_string())
                .unwrap_or_else(|| "none".into()),
        );
        audit_meta.insert(
            "new_policy_version".to_string(),
            req.schema
                .access_control
                .as_ref()
                .map(|_| req.schema.version.to_string())
                .unwrap_or_else(|| "none".into()),
        );
        audit_meta.insert(
            "policy_nullable_fields".to_string(),
            policy_compile_report.nullable_fields.len().to_string(),
        );
        audit_meta.insert(
            "policy_denied_write_fields".to_string(),
            policy_compile_report.denied_write_fields.len().to_string(),
        );
        audit_meta.insert(
            "policy_envelopes".to_string(),
            policy_compile_report.envelope_summaries.len().to_string(),
        );

        self.audit.append(
            AuditEntry::new(
                collection,
                EntityId::new(""),
                0,
                MutationType::SchemaUpdate,
                None,
                None,
                req.actor,
            )
            .with_metadata(audit_meta),
        )?;
        Ok(PutSchemaResponse {
            schema: req.schema,
            compatibility: Some(compatibility),
            diff: Some(diff),
            policy_compile_report: Some(policy_compile_report),
            compile_report: Some(compile_report),
            warnings,
            dry_run_explanations: None,
            dry_run: false,
        })
    }

    /// Run fixture explain inputs against the proposed schema/plan during a
    /// putSchema dry-run. Returns:
    ///
    /// - `None` when the proposed policy failed to compile (the report's
    ///   `errors` describe why).
    /// - `Some(vec![])` when no explain inputs were supplied or the proposed
    ///   schema has no `access_control` block.
    /// - `Some(vec![..])` carrying one explanation per input, in order.
    ///
    /// Each explanation is evaluated as a synthetic admin caller derived
    /// from `req.actor` unless the input carries an `actor_override`, in which
    /// case the override drives the synthetic caller and subject bindings.
    /// Transaction explain inputs recurse into child operations against the
    /// same proposed catalog, so the proposed plan governs the root collection
    /// throughout.
    fn dry_run_explanations_for_proposed(
        &self,
        proposed_schema: &CollectionSchema,
        report: &axon_schema::PolicyCompileReport,
        explain_inputs: &[ExplainPolicyRequest],
        actor: Option<&str>,
    ) -> Result<Option<Vec<PolicyExplanationResponse>>, AxonError> {
        if !report.errors.is_empty() {
            return Ok(None);
        }
        if explain_inputs.is_empty() {
            return Ok(Some(Vec::new()));
        }
        // Build the in-memory plan for the proposed schema.
        let schemas = self.policy_catalog_schemas(proposed_schema)?;
        let catalog = match compile_policy_catalog(&schemas) {
            Ok(catalog) => catalog,
            Err(_) => {
                // Compile errors should already be in the report from the
                // caller, but defensively return None instead of panicking.
                return Ok(None);
            }
        };
        let Some(proposed_plan) = catalog
            .plans
            .get(proposed_schema.collection.as_str())
            .cloned()
        else {
            // No access_control on the proposed schema -> empty result.
            return Ok(Some(Vec::new()));
        };
        let proposed_plans = catalog.plans;
        let mut out = Vec::with_capacity(explain_inputs.len());
        for input in explain_inputs {
            validate_dry_run_explain_collection(proposed_schema, input)?;
            let mut input_clone = input.clone();
            input_clone.collection = Some(proposed_schema.collection.clone());
            // Prepare child collections for transactions before stripping the
            // override (the override only applies once, at the top level).
            if input_clone
                .operation
                .trim()
                .eq_ignore_ascii_case("transaction")
            {
                for child in &mut input_clone.operations {
                    validate_dry_run_explain_collection(proposed_schema, child)?;
                    child.collection = Some(proposed_schema.collection.clone());
                }
            }
            let synthetic_caller = synthetic_dry_run_caller(actor, input.actor_override.as_ref())?;
            let response = self.explain_policy_with_plan(
                input_clone,
                &synthetic_caller,
                None,
                proposed_schema,
                &proposed_plan,
                &proposed_plans,
                input.actor_override.as_ref(),
            )?;
            out.push(response);
        }
        Ok(Some(out))
    }
}

fn validate_dry_run_explain_collection(
    proposed_schema: &CollectionSchema,
    input: &ExplainPolicyRequest,
) -> Result<(), AxonError> {
    if let Some(target) = &input.collection {
        if target.as_str() != proposed_schema.collection.as_str() {
            return Err(AxonError::InvalidArgument(format!(
                "putSchema dry-run explain_inputs must target the proposed collection '{}', got '{}'",
                proposed_schema.collection, target
            )));
        }
    }
    Ok(())
}

impl<S: StorageAdapter> AxonHandler<S> {
    /// Retrieve the schema for a collection.
    ///
    /// Returns [`AxonError::NotFound`] if no schema has been stored.
    pub fn handle_get_schema(&self, req: GetSchemaRequest) -> Result<GetSchemaResponse, AxonError> {
        self.storage
            .get_schema(&req.collection)?
            .map(|schema| GetSchemaResponse {
                schema: Self::present_schema(&req.collection, schema),
            })
            .ok_or_else(|| {
                AxonError::NotFound(format!("schema for collection '{}'", req.collection))
            })
    }

    /// Revalidate all entities in a collection against the current schema (US-060).
    ///
    /// Scans all entities and reports which ones fail validation, including
    /// the entity ID, version, and specific errors.
    pub fn revalidate(&self, req: RevalidateRequest) -> Result<RevalidateResponse, AxonError> {
        let schema = self.storage.get_schema(&req.collection)?.ok_or_else(|| {
            AxonError::NotFound(format!("schema for collection '{}'", req.collection))
        })?;

        let all = self.storage.range_scan(&req.collection, None, None, None)?;
        let total_scanned = all.len();
        let mut invalid = Vec::new();

        for entity in &all {
            if let Err(errs) = axon_schema::validate_entity(&schema, &entity.data) {
                invalid.push(InvalidEntity {
                    id: entity.id.to_string(),
                    version: entity.version,
                    errors: errs.0.iter().map(|e| e.to_string()).collect(),
                });
            }
        }

        let valid_count = total_scanned - invalid.len();

        Ok(RevalidateResponse {
            total_scanned,
            valid_count,
            invalid,
        })
    }

    // ── Namespace management (US-036) ───────────────────────────────────────

    /// Create a schema namespace (database.schema).
    pub fn create_namespace(
        &mut self,
        req: CreateNamespaceRequest,
    ) -> Result<CreateNamespaceResponse, AxonError> {
        self.storage
            .create_namespace(&Namespace::new(&req.database, &req.schema))?;
        Ok(CreateNamespaceResponse {
            database: req.database,
            schema: req.schema,
        })
    }

    /// List schemas within a database.
    pub fn list_namespaces(
        &self,
        req: ListNamespacesRequest,
    ) -> Result<ListNamespacesResponse, AxonError> {
        let schemas = self.storage.list_namespaces(&req.database)?;
        Ok(ListNamespacesResponse {
            database: req.database,
            schemas,
        })
    }

    /// List collections within a namespace.
    pub fn list_namespace_collections(
        &self,
        req: ListNamespaceCollectionsRequest,
    ) -> Result<ListNamespaceCollectionsResponse, AxonError> {
        let collections = self
            .storage
            .list_namespace_collections(&Namespace::new(&req.database, &req.schema))?
            .into_iter()
            .map(|collection| collection.to_string())
            .collect();
        Ok(ListNamespaceCollectionsResponse {
            database: req.database,
            schema: req.schema,
            collections,
        })
    }

    /// Drop a namespace. Fails if non-empty unless force is set.
    pub fn drop_namespace(
        &mut self,
        req: DropNamespaceRequest,
    ) -> Result<DropNamespaceResponse, AxonError> {
        if req.schema == DEFAULT_SCHEMA {
            return Err(AxonError::InvalidOperation(format!(
                "schema '{}' cannot be dropped",
                req.schema
            )));
        }

        let namespace = Namespace::new(&req.database, &req.schema);
        let collections = self.storage.list_namespace_collections(&namespace)?;
        let count = collections.len();
        if count > 0 && !req.force {
            return Err(AxonError::InvalidOperation(format!(
                "namespace '{}' contains {} collections: {}. Use force=true to drop",
                namespace,
                count,
                collections
                    .iter()
                    .take(5)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        let doomed_collections: Vec<_> = collections
            .iter()
            .map(|collection| QualifiedCollectionId::from_parts(&namespace, collection))
            .collect();
        self.invalidate_markdown_templates_for_collections(&doomed_collections)?;
        self.storage.drop_namespace(&namespace)?;
        self.append_collection_drop_audit_entries(&collections)?;
        Ok(DropNamespaceResponse {
            database: req.database,
            schema: req.schema,
            collections_removed: count,
        })
    }

    // ── Database isolation (US-035, FEAT-014) ───────────────────────────

    /// Create a new database (isolated data space).
    ///
    /// A database is a namespace prefix that groups collections.
    /// Collections in different databases are invisible to each other.
    pub fn create_database(
        &mut self,
        req: CreateDatabaseRequest,
    ) -> Result<CreateDatabaseResponse, AxonError> {
        self.storage.create_database(&req.name)?;
        Ok(CreateDatabaseResponse { name: req.name })
    }

    /// Drop a database and all its collections.
    ///
    /// Removes all namespaces with the given database prefix and their
    /// collections. Audit entries are retained but the data is purged.
    pub fn drop_database(
        &mut self,
        req: DropDatabaseRequest,
    ) -> Result<DropDatabaseResponse, AxonError> {
        if req.name == DEFAULT_DATABASE {
            return Err(AxonError::InvalidOperation(format!(
                "database '{DEFAULT_DATABASE}' is implicit and cannot be dropped"
            )));
        }

        if !self.storage.list_databases()?.contains(&req.name) {
            return Err(AxonError::NotFound(format!("database '{}'", req.name)));
        }

        let schemas = self.storage.list_namespaces(&req.name)?;
        let mut collections = Vec::new();
        let mut doomed_collections = Vec::new();
        for schema in &schemas {
            let namespace = Namespace::new(&req.name, schema);
            let namespace_collections = self.storage.list_namespace_collections(&namespace)?;
            doomed_collections.extend(
                namespace_collections
                    .iter()
                    .map(|collection| QualifiedCollectionId::from_parts(&namespace, collection)),
            );
            collections.extend(namespace_collections);
        }
        let total_collections = collections.len();

        if total_collections > 0 && !req.force {
            return Err(AxonError::InvalidOperation(format!(
                "database '{}' contains {total_collections} collections. Use force=true to drop",
                req.name
            )));
        }

        self.invalidate_markdown_templates_for_collections(&doomed_collections)?;
        self.storage.drop_database(&req.name)?;
        self.append_collection_drop_audit_entries(&collections)?;

        Ok(DropDatabaseResponse {
            name: req.name,
            collections_removed: total_collections,
        })
    }

    /// List all databases.
    pub fn list_databases(
        &self,
        _req: ListDatabasesRequest,
    ) -> Result<ListDatabasesResponse, AxonError> {
        Ok(ListDatabasesResponse {
            databases: self.storage.list_databases()?,
        })
    }

    /// Diff two schema versions for a collection (US-061).
    ///
    /// Retrieves both versions from storage and produces a field-level diff.
    pub fn diff_schema_versions(
        &self,
        req: DiffSchemaRequest,
    ) -> Result<DiffSchemaResponse, AxonError> {
        let schema_a = self
            .storage
            .get_schema_version(&req.collection, req.version_a)?
            .ok_or_else(|| {
                AxonError::NotFound(format!(
                    "schema version {} for collection '{}'",
                    req.version_a, req.collection
                ))
            })?;
        let schema_b = self
            .storage
            .get_schema_version(&req.collection, req.version_b)?
            .ok_or_else(|| {
                AxonError::NotFound(format!(
                    "schema version {} for collection '{}'",
                    req.version_b, req.collection
                ))
            })?;

        let diff = axon_schema::diff_schemas(
            schema_a.entity_schema.as_ref(),
            schema_b.entity_schema.as_ref(),
        );

        Ok(DiffSchemaResponse {
            version_a: req.version_a,
            version_b: req.version_b,
            diff,
        })
    }

    // ── Link operations ──────────────────────────────────────────────────────

    /// Create a typed link from one entity to another.
    ///
    /// Both source and target must exist in storage; if either is missing,
    /// [`AxonError::NotFound`] is returned.
    pub fn create_link(&mut self, req: CreateLinkRequest) -> Result<CreateLinkResponse, AxonError> {
        // Verify source and target exist.
        if self
            .storage
            .get(&req.source_collection, &req.source_id)?
            .is_none()
        {
            return Err(AxonError::NotFound(format!(
                "source entity {}/{}",
                req.source_collection, req.source_id
            )));
        }
        if self
            .storage
            .get(&req.target_collection, &req.target_id)?
            .is_none()
        {
            return Err(AxonError::NotFound(format!(
                "target entity {}/{}",
                req.target_collection, req.target_id
            )));
        }

        // Enforce link-type definitions from source collection schema (ADR-002).
        if let Some(schema) = self.storage.get_schema(&req.source_collection)? {
            if !schema.link_types.is_empty() {
                let link_def = schema.link_types.get(&req.link_type).ok_or_else(|| {
                    AxonError::SchemaValidation(format!(
                        "link type '{}' is not declared in collection '{}' schema",
                        req.link_type, req.source_collection
                    ))
                })?;

                // Verify target collection matches the declaration.
                if req.target_collection.as_str() != link_def.target_collection {
                    return Err(AxonError::SchemaValidation(format!(
                        "link type '{}' requires target collection '{}', got '{}'",
                        req.link_type, link_def.target_collection, req.target_collection
                    )));
                }

                // Validate link metadata against metadata_schema if declared.
                if let Some(metadata_schema) = &link_def.metadata_schema {
                    validate_link_metadata(metadata_schema, &req.metadata)?;
                }

                // Enforce cardinality constraints.
                use axon_schema::Cardinality;
                match link_def.cardinality {
                    Cardinality::OneToOne | Cardinality::ManyToOne => {
                        // Source can have at most one outgoing link of this type.
                        let prefix = format!(
                            "{}/{}/{}/",
                            req.source_collection, req.source_id, req.link_type
                        );
                        let start = EntityId::new(&prefix);
                        let existing = self.storage.range_scan(
                            &Link::links_collection(),
                            Some(&start),
                            None,
                            Some(1),
                        )?;
                        let has_outgoing =
                            existing.iter().any(|e| e.id.as_str().starts_with(&prefix));
                        if has_outgoing {
                            return Err(AxonError::SchemaValidation(format!(
                                "cardinality violation: source {}/{} already has a '{}' link \
                                 ({:?} allows at most one outgoing)",
                                req.source_collection,
                                req.source_id,
                                req.link_type,
                                link_def.cardinality
                            )));
                        }
                    }
                    Cardinality::OneToMany | Cardinality::ManyToMany => {}
                }
                match link_def.cardinality {
                    Cardinality::OneToOne | Cardinality::OneToMany => {
                        // Target can have at most one inbound link of this type.
                        // Scan the reverse-index: {target_col}/{target_id}/.../{link_type}
                        let rev_col = Link::links_rev_collection();
                        let prefix = format!("{}/{}/", req.target_collection, req.target_id);
                        let start = EntityId::new(&prefix);
                        let candidates =
                            self.storage
                                .range_scan(&rev_col, Some(&start), None, None)?;
                        let has_inbound = candidates.iter().any(|e| {
                            let id = e.id.as_str();
                            id.starts_with(&prefix) && id.ends_with(&format!("/{}", req.link_type))
                        });
                        if has_inbound {
                            return Err(AxonError::SchemaValidation(format!(
                                "cardinality violation: target {}/{} already has an inbound '{}' link \
                                 ({:?} allows at most one inbound)",
                                req.target_collection,
                                req.target_id,
                                req.link_type,
                                link_def.cardinality
                            )));
                        }
                    }
                    Cardinality::ManyToOne | Cardinality::ManyToMany => {}
                }
            }
        }

        // Reject duplicate (source, target, link_type) triples.
        let link_id = Link::storage_id(
            &req.source_collection,
            &req.source_id,
            &req.link_type,
            &req.target_collection,
            &req.target_id,
        );
        if self
            .storage
            .get(&Link::links_collection(), &link_id)?
            .is_some()
        {
            return Err(AxonError::AlreadyExists(format!(
                "link {}/{}/{}/{}/{}",
                req.source_collection,
                req.source_id,
                req.link_type,
                req.target_collection,
                req.target_id
            )));
        }

        let link = Link {
            source_collection: req.source_collection,
            source_id: req.source_id,
            target_collection: req.target_collection,
            target_id: req.target_id,
            link_type: req.link_type,
            metadata: req.metadata,
        };

        // Store the link and its reverse-index entry.
        self.storage.put(link.to_rev_entity())?;
        let link_entity = link.to_entity();
        self.storage.put(link_entity.clone())?;

        // Audit: record the link creation.
        let mut link_audit_entry = AuditEntry::new(
            link_entity.collection,
            link_entity.id,
            link_entity.version,
            MutationType::LinkCreate,
            None,
            Some(link_entity.data),
            req.actor,
        );
        if let Some(attr) = req.attribution.clone() {
            link_audit_entry = link_audit_entry.with_attribution(attr);
        }
        self.audit.append(link_audit_entry)?;

        Ok(CreateLinkResponse { link })
    }

    /// Delete a typed link between two entities.
    ///
    /// Removes both the forward link from `__axon_links__` and the corresponding
    /// reverse-index entry from `__axon_links_rev__`. If the link does not exist,
    /// [`AxonError::NotFound`] is returned.
    pub fn delete_link(&mut self, req: DeleteLinkRequest) -> Result<DeleteLinkResponse, AxonError> {
        let link_id = Link::storage_id(
            &req.source_collection,
            &req.source_id,
            &req.link_type,
            &req.target_collection,
            &req.target_id,
        );

        // Verify the link exists before attempting deletion; capture its data for the audit entry.
        let link_entity = self
            .storage
            .get(&Link::links_collection(), &link_id)?
            .ok_or_else(|| {
                AxonError::NotFound(format!(
                    "link {}/{} --[{}]--> {}/{}",
                    req.source_collection,
                    req.source_id,
                    req.link_type,
                    req.target_collection,
                    req.target_id,
                ))
            })?;

        // Delete the reverse-index entry first, then the forward link.
        let rev_id = Link::rev_storage_id(
            &req.target_collection,
            &req.target_id,
            &req.source_collection,
            &req.source_id,
            &req.link_type,
        );
        self.storage
            .delete(&Link::links_rev_collection(), &rev_id)?;
        self.storage.delete(&Link::links_collection(), &link_id)?;

        // Audit: record the link deletion.
        let mut link_audit_entry = AuditEntry::new(
            link_entity.collection,
            link_entity.id,
            link_entity.version,
            MutationType::LinkDelete,
            Some(link_entity.data),
            None,
            req.actor,
        );
        if let Some(attr) = req.attribution.clone() {
            link_audit_entry = link_audit_entry.with_attribution(attr);
        }
        self.audit.append(link_audit_entry)?;

        Ok(DeleteLinkResponse {
            source_collection: req.source_collection.to_string(),
            source_id: req.source_id.to_string(),
            target_collection: req.target_collection.to_string(),
            target_id: req.target_id.to_string(),
            link_type: req.link_type,
        })
    }

    /// Traverse links from a starting entity using BFS up to `max_depth` hops.
    ///
    /// Returns all reachable entities (excluding the starting entity itself)
    /// in BFS order. Supports forward (outbound) and reverse (inbound) traversal,
    /// per-hop entity filtering, and path/link metadata reporting.
    pub fn traverse(&self, req: TraverseRequest) -> Result<TraverseResponse, AxonError> {
        self.traverse_with_read_context(req, None, None)
    }

    pub fn traverse_with_caller(
        &self,
        req: TraverseRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<TraverseResponse, AxonError> {
        self.traverse_with_read_context(req, Some(caller), attribution.as_ref())
    }

    fn traverse_with_read_context(
        &self,
        req: TraverseRequest,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<TraverseResponse, AxonError> {
        let max_depth = req
            .max_depth
            .unwrap_or(DEFAULT_MAX_DEPTH)
            .min(MAX_DEPTH_CAP);

        if let Some(start) = self.storage.get(&req.collection, &req.id)? {
            if !self.entity_visible_for_read_with_context(
                &req.collection,
                &start,
                caller,
                attribution,
            )? {
                return Err(AxonError::NotFound(req.id.to_string()));
            }
        }

        let all_links = self.load_all_links()?;
        let reverse = req.direction == TraverseDirection::Reverse;

        let mut visited: HashSet<(String, String)> = HashSet::new();
        let start_key = (req.collection.to_string(), req.id.to_string());
        visited.insert(start_key);

        // Queue entries: (collection, id, current_depth, path_so_far)
        let mut queue: VecDeque<(CollectionId, EntityId, usize, Vec<TraverseHop>)> =
            VecDeque::new();
        queue.push_back((req.collection, req.id, 0, Vec::new()));

        let mut entities = Vec::new();
        let mut paths = Vec::new();
        let mut links_traversed = Vec::new();

        while let Some((col, id, depth, path)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let neighbors: Vec<&Link> = if reverse {
                all_links
                    .iter()
                    .filter(|l| {
                        l.target_collection == col
                            && l.target_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            } else {
                all_links
                    .iter()
                    .filter(|l| {
                        l.source_collection == col
                            && l.source_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            };

            for link in neighbors {
                let (next_col, next_id) = if reverse {
                    (&link.source_collection, &link.source_id)
                } else {
                    (&link.target_collection, &link.target_id)
                };

                let neighbor_key = (next_col.to_string(), next_id.to_string());
                if visited.contains(&neighbor_key) {
                    continue;
                }

                let Some(entity) = self.get_visible_entity_for_read_with_context(
                    next_col,
                    next_id,
                    caller,
                    attribution,
                )?
                else {
                    continue;
                };

                // Apply hop filter if present.
                if let Some(ref filter) = req.hop_filter {
                    if !apply_filter(filter, &entity.data) {
                        continue;
                    }
                }

                let entity = self.redact_entity_for_read_with_context(
                    next_col,
                    entity,
                    caller,
                    attribution,
                )?;
                visited.insert(neighbor_key);
                links_traversed.push(link.clone());

                let mut hop_path = path.clone();
                hop_path.push(TraverseHop {
                    link: link.clone(),
                    entity: entity.clone(),
                });

                paths.push(TraversePath {
                    hops: hop_path.clone(),
                });
                entities.push(entity);
                queue.push_back((next_col.clone(), next_id.clone(), depth + 1, hop_path));
            }
        }

        Ok(TraverseResponse {
            entities,
            paths,
            links: links_traversed,
        })
    }

    /// Check whether a target entity is reachable from a source entity.
    ///
    /// Short-circuits BFS as soon as the target is found, returning `true`
    /// and the hop depth. More efficient than a full `traverse()` when only
    /// connectivity matters.
    pub fn reachable(&self, req: ReachableRequest) -> Result<ReachableResponse, AxonError> {
        self.reachable_with_read_context(req, None, None)
    }

    pub fn reachable_with_caller(
        &self,
        req: ReachableRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<ReachableResponse, AxonError> {
        self.reachable_with_read_context(req, Some(caller), attribution.as_ref())
    }

    fn reachable_with_read_context(
        &self,
        req: ReachableRequest,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<ReachableResponse, AxonError> {
        let max_depth = req
            .max_depth
            .unwrap_or(DEFAULT_MAX_DEPTH)
            .min(MAX_DEPTH_CAP);

        let all_links = self.load_all_links()?;
        let reverse = req.direction == TraverseDirection::Reverse;
        let target_key = (req.target_collection.to_string(), req.target_id.to_string());

        let mut visited: HashSet<(String, String)> = HashSet::new();
        let start_key = (req.source_collection.to_string(), req.source_id.to_string());

        if self
            .get_visible_entity_for_read_with_context(
                &req.source_collection,
                &req.source_id,
                caller,
                attribution,
            )?
            .is_none()
        {
            return Ok(ReachableResponse {
                reachable: false,
                depth: None,
            });
        }

        // Check trivial case: source == target.
        if start_key == target_key {
            return Ok(ReachableResponse {
                reachable: true,
                depth: Some(0),
            });
        }

        visited.insert(start_key);

        let mut queue: VecDeque<(CollectionId, EntityId, usize)> = VecDeque::new();
        queue.push_back((req.source_collection, req.source_id, 0));

        while let Some((col, id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let neighbors: Vec<&Link> = if reverse {
                all_links
                    .iter()
                    .filter(|l| {
                        l.target_collection == col
                            && l.target_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            } else {
                all_links
                    .iter()
                    .filter(|l| {
                        l.source_collection == col
                            && l.source_id == id
                            && req
                                .link_type
                                .as_deref()
                                .map_or(true, |lt| l.link_type == lt)
                    })
                    .collect()
            };

            for link in neighbors {
                let (next_col, next_id) = if reverse {
                    (&link.source_collection, &link.source_id)
                } else {
                    (&link.target_collection, &link.target_id)
                };

                let neighbor_key = (next_col.to_string(), next_id.to_string());
                if visited.contains(&neighbor_key) {
                    continue;
                }

                if self
                    .get_visible_entity_for_read_with_context(
                        next_col,
                        next_id,
                        caller,
                        attribution,
                    )?
                    .is_none()
                {
                    continue;
                }

                // Short-circuit: found the target after read-policy visibility.
                if neighbor_key == target_key {
                    return Ok(ReachableResponse {
                        reachable: true,
                        depth: Some(depth + 1),
                    });
                }

                visited.insert(neighbor_key);

                queue.push_back((next_col.clone(), next_id.clone(), depth + 1));
            }
        }

        Ok(ReachableResponse {
            reachable: false,
            depth: None,
        })
    }

    /// Find candidate target entities for a link type (US-070, FEAT-020).
    ///
    /// Returns entities from the target collection with an already-linked
    /// indicator, cardinality info, and existing link count.
    pub fn find_link_candidates(
        &self,
        req: crate::request::FindLinkCandidatesRequest,
    ) -> Result<crate::response::FindLinkCandidatesResponse, AxonError> {
        self.find_link_candidates_with_read_context(req, None, None)
    }

    pub fn find_link_candidates_with_caller(
        &self,
        req: crate::request::FindLinkCandidatesRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<crate::response::FindLinkCandidatesResponse, AxonError> {
        self.find_link_candidates_with_read_context(req, Some(caller), attribution.as_ref())
    }

    fn find_link_candidates_with_read_context(
        &self,
        req: crate::request::FindLinkCandidatesRequest,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<crate::response::FindLinkCandidatesResponse, AxonError> {
        // Verify source entity exists.
        if self
            .get_visible_entity_for_read_with_context(
                &req.source_collection,
                &req.source_id,
                caller,
                attribution,
            )?
            .is_none()
        {
            return Err(AxonError::NotFound(format!(
                "{}/{}",
                req.source_collection, req.source_id
            )));
        }

        // Look up link type definition from source schema.
        let source_schema = self.storage.get_schema(&req.source_collection)?;
        let link_def = source_schema
            .as_ref()
            .and_then(|s| s.link_types.get(&req.link_type));

        let target_collection = link_def
            .map(|d| CollectionId::new(&d.target_collection))
            .unwrap_or_else(|| req.source_collection.clone());

        let cardinality_str = link_def
            .map(|d| {
                format!("{:?}", d.cardinality)
                    .to_lowercase()
                    .replace("to", "-to-")
            })
            .unwrap_or_else(|| "unknown".into());

        // Get all existing links of this type from the source.
        let all_links = self.load_all_links()?;
        let mut existing_targets = HashSet::new();
        for link in all_links.iter().filter(|link| {
            link.source_collection == req.source_collection
                && link.source_id == req.source_id
                && link.target_collection == target_collection
                && link.link_type == req.link_type
        }) {
            if self
                .get_visible_entity_for_read_with_context(
                    &link.target_collection,
                    &link.target_id,
                    caller,
                    attribution,
                )?
                .is_some()
            {
                existing_targets.insert(link.target_id.to_string());
            }
        }
        let existing_link_count = existing_targets.len();

        // Fetch candidate entities from the target collection (FEAT-013 index acceleration).
        let target_schema = self.storage.get_schema(&target_collection)?;
        let index_candidates = try_index_lookup(
            &self.storage,
            &target_collection,
            req.filter.as_ref(),
            target_schema.as_ref(),
        );
        let all_targets = if let Some(entity_ids) = index_candidates {
            let mut entities = Vec::with_capacity(entity_ids.len());
            for eid in &entity_ids {
                if let Some(e) = self.storage.get(&target_collection, eid)? {
                    entities.push(e);
                }
            }
            entities
        } else {
            self.storage
                .range_scan(&target_collection, None, None, None)?
        };

        // Filter, apply read visibility, then collect candidates.
        let limit = req.limit.unwrap_or(50);
        let mut candidates = Vec::new();
        for entity in all_targets {
            if !req
                .filter
                .as_ref()
                .map_or(true, |filter| apply_filter(filter, &entity.data))
            {
                continue;
            }
            if !self.entity_visible_for_read_with_context(
                &target_collection,
                &entity,
                caller,
                attribution,
            )? {
                continue;
            }

            let already_linked = existing_targets.contains(entity.id.as_str());
            let entity = self.redact_entity_for_read_with_context(
                &target_collection,
                entity,
                caller,
                attribution,
            )?;
            candidates.push(crate::response::LinkCandidate {
                entity,
                already_linked,
            });
            if candidates.len() >= limit {
                break;
            }
        }

        Ok(crate::response::FindLinkCandidatesResponse {
            target_collection: target_collection.to_string(),
            link_type: req.link_type,
            cardinality: cardinality_str,
            existing_link_count,
            candidates,
        })
    }

    /// List an entity's neighbors: outbound + inbound linked entities
    /// grouped by link type and direction (US-071, FEAT-020).
    pub fn list_neighbors(
        &self,
        req: crate::request::ListNeighborsRequest,
    ) -> Result<crate::response::ListNeighborsResponse, AxonError> {
        self.list_neighbors_with_read_context(req, None, None)
    }

    pub fn list_neighbors_with_caller(
        &self,
        req: crate::request::ListNeighborsRequest,
        caller: &CallerIdentity,
        attribution: Option<AuditAttribution>,
    ) -> Result<crate::response::ListNeighborsResponse, AxonError> {
        self.list_neighbors_with_read_context(req, Some(caller), attribution.as_ref())
    }

    fn list_neighbors_with_read_context(
        &self,
        req: crate::request::ListNeighborsRequest,
        caller: Option<&CallerIdentity>,
        attribution: Option<&AuditAttribution>,
    ) -> Result<crate::response::ListNeighborsResponse, AxonError> {
        use std::collections::BTreeMap;

        // Verify entity exists.
        if self
            .get_visible_entity_for_read_with_context(
                &req.collection,
                &req.id,
                caller,
                attribution,
            )?
            .is_none()
        {
            return Err(AxonError::NotFound(format!(
                "{}/{}",
                req.collection, req.id
            )));
        }

        let all_links = self.load_all_links()?;

        // group key: (link_type, direction)
        let mut groups: BTreeMap<(String, String), Vec<Entity>> = BTreeMap::new();

        let include_outbound = req
            .direction
            .as_ref()
            .map_or(true, |d| *d == TraverseDirection::Forward);
        let include_inbound = req
            .direction
            .as_ref()
            .map_or(true, |d| *d == TraverseDirection::Reverse);

        for link in &all_links {
            let type_filter_ok = req
                .link_type
                .as_deref()
                .map_or(true, |lt| link.link_type == lt);
            if !type_filter_ok {
                continue;
            }

            // Outbound: this entity is the source.
            if include_outbound
                && link.source_collection == req.collection
                && link.source_id == req.id
            {
                let key = (link.link_type.clone(), "outbound".to_string());
                if let Some(target) = self.get_visible_entity_for_read_with_context(
                    &link.target_collection,
                    &link.target_id,
                    caller,
                    attribution,
                )? {
                    let target = self.redact_entity_for_read_with_context(
                        &link.target_collection,
                        target,
                        caller,
                        attribution,
                    )?;
                    groups.entry(key).or_default().push(target);
                }
            }

            // Inbound: this entity is the target.
            if include_inbound
                && link.target_collection == req.collection
                && link.target_id == req.id
            {
                let key = (link.link_type.clone(), "inbound".to_string());
                if let Some(source) = self.get_visible_entity_for_read_with_context(
                    &link.source_collection,
                    &link.source_id,
                    caller,
                    attribution,
                )? {
                    let source = self.redact_entity_for_read_with_context(
                        &link.source_collection,
                        source,
                        caller,
                        attribution,
                    )?;
                    groups.entry(key).or_default().push(source);
                }
            }
        }

        let mut total_count = 0;
        let result_groups: Vec<crate::response::NeighborGroup> = groups
            .into_iter()
            .map(|((link_type, direction), entities)| {
                total_count += entities.len();
                crate::response::NeighborGroup {
                    link_type,
                    direction,
                    entities,
                }
            })
            .collect();

        Ok(crate::response::ListNeighborsResponse {
            groups: result_groups,
            total_count,
        })
    }

    /// Load all stored links from the internal links collection.
    fn load_all_links(&self) -> Result<Vec<Link>, AxonError> {
        let links_col = Link::links_collection();
        let entities = self.storage.range_scan(&links_col, None, None, None)?;
        Ok(entities.iter().filter_map(Link::from_entity).collect())
    }

    /// Transition an entity through a named lifecycle state machine (FEAT-015).
    ///
    /// Steps:
    /// 1. Load the collection schema and locate the named lifecycle.
    /// 2. Read the current entity.
    /// 3. Determine the current state from `entity.data[lifecycle.field]`.
    /// 4. Validate that `target_state` is reachable from the current state.
    /// 5. Write the updated entity via OCC using `expected_version`.
    pub fn transition_lifecycle(
        &mut self,
        req: TransitionLifecycleRequest,
    ) -> Result<TransitionLifecycleResponse, AxonError> {
        // (1) Load schema and find the lifecycle definition.
        let schema = self
            .storage
            .get_schema(&req.collection_id)?
            .ok_or_else(|| {
                AxonError::NotFound(format!("schema for collection {}", req.collection_id))
            })?;

        let lifecycle = schema.lifecycles.get(&req.lifecycle_name).ok_or_else(|| {
            AxonError::LifecycleNotFound {
                lifecycle_name: req.lifecycle_name.clone(),
            }
        })?;

        // (2) Read current entity.
        let entity = self
            .storage
            .get(&req.collection_id, &req.entity_id)?
            .ok_or_else(|| AxonError::NotFound(req.entity_id.to_string()))?;

        // (3) Get current state from entity data.
        let current_state = entity
            .data
            .get(&lifecycle.field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // (4) Check that the target state is allowed from the current state.
        let allowed: Vec<String> = lifecycle
            .transitions
            .get(&current_state)
            .cloned()
            .unwrap_or_default();

        if !allowed.contains(&req.target_state) {
            return Err(AxonError::InvalidTransition {
                lifecycle_name: req.lifecycle_name.clone(),
                current_state,
                target_state: req.target_state.clone(),
                valid_transitions: allowed,
            });
        }

        // (5) Apply the transition via update_entity (OCC).
        let mut new_data = entity.data.clone();
        new_data[&lifecycle.field] = serde_json::Value::String(req.target_state.clone());
        let policy_snapshot = self.policy_snapshot_for_request(
            &req.collection_id,
            Some(&schema),
            req.actor.as_deref(),
            None,
            req.attribution.as_ref(),
        )?;
        self.enforce_transition_policy(
            &schema,
            policy_snapshot.as_ref(),
            TransitionPolicyCheck {
                collection: &req.collection_id,
                entity_id: &req.entity_id,
                lifecycle_field: &lifecycle.field,
                target_state: &req.target_state,
                data: &new_data,
            },
        )?;

        let update_resp = self.update_entity(crate::request::UpdateEntityRequest {
            collection: req.collection_id,
            id: req.entity_id,
            data: new_data,
            expected_version: req.expected_version,
            actor: req.actor,
            audit_metadata: req.audit_metadata,
            attribution: req.attribution,
        })?;

        Ok(TransitionLifecycleResponse {
            entity: update_resp.entity,
            audit_id: update_resp.audit_id,
        })
    }
}

// ── Snapshot page token codec (US-080) ────────────────────────────────────
//
// Snapshot pagination uses an opaque token that identifies the last entity
// returned on the previous page by its `(collection, id)` key. The wire
// format is `<collection_len>:<collection><id>`, which is length-prefixed so
// it can unambiguously round-trip any byte content in either field without
// depending on an out-of-tree escape/base64 library.
fn encode_snapshot_page_token(collection: &str, id: &str) -> String {
    format!("{}:{collection}{id}", collection.len())
}

fn decode_snapshot_page_token(token: &str) -> Result<(String, String), AxonError> {
    let (len_str, rest) = token.split_once(':').ok_or_else(|| {
        AxonError::InvalidArgument("malformed snapshot page token: missing length prefix".into())
    })?;
    let collection_len: usize = len_str.parse().map_err(|e| {
        AxonError::InvalidArgument(format!(
            "malformed snapshot page token: invalid length prefix: {e}"
        ))
    })?;
    if rest.len() < collection_len {
        return Err(AxonError::InvalidArgument(
            "malformed snapshot page token: payload shorter than declared length".into(),
        ));
    }
    if !rest.is_char_boundary(collection_len) {
        return Err(AxonError::InvalidArgument(
            "malformed snapshot page token: length prefix does not land on utf8 boundary".into(),
        ));
    }
    let (collection, id) = rest.split_at(collection_len);
    Ok((collection.to_string(), id.to_string()))
}

// ── Index-accelerated query planner (FEAT-013) ─────────────────────────────────

/// Attempt to use a secondary index to satisfy a filter.
///
/// Returns `Some(entity_ids)` if the filter can be satisfied by an index lookup.
/// Returns `None` to indicate the caller should fall back to a full scan.
///
/// Currently handles:
/// - Single `FieldFilter` with `Eq` op when the field has a declared index
/// - Single `FieldFilter` with `Gt`/`Gte`/`Lt`/`Lte` op for range queries
/// - `And` of equality filters where any single field has an index (picks first)
fn try_index_lookup<S: StorageAdapter>(
    storage: &S,
    collection: &CollectionId,
    filter: Option<&FilterNode>,
    schema: Option<&CollectionSchema>,
) -> Option<Vec<EntityId>> {
    let filter = filter?;
    let schema = schema?;
    if schema.indexes.is_empty() {
        return None;
    }

    match filter {
        FilterNode::Field(f) => {
            // Find an index matching this field.
            let idx = schema.indexes.iter().find(|i| i.field == f.field)?;
            let val = axon_storage::extract_index_value(&f.value, &idx.index_type)?;

            match f.op {
                FilterOp::Eq => storage.index_lookup(collection, &f.field, &val).ok(),
                FilterOp::Gt => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Excluded(&val),
                        std::ops::Bound::Unbounded,
                    )
                    .ok(),
                FilterOp::Gte => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Included(&val),
                        std::ops::Bound::Unbounded,
                    )
                    .ok(),
                FilterOp::Lt => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Excluded(&val),
                    )
                    .ok(),
                FilterOp::Lte => storage
                    .index_range(
                        collection,
                        &f.field,
                        std::ops::Bound::Unbounded,
                        std::ops::Bound::Included(&val),
                    )
                    .ok(),
                _ => None, // Ne, In, Contains — fall back to scan
            }
        }
        FilterNode::And { filters } => {
            // Try to find at least one equality sub-filter with an index.
            for sub in filters {
                if let FilterNode::Field(f) = sub {
                    if f.op == FilterOp::Eq {
                        if let Some(idx) = schema.indexes.iter().find(|i| i.field == f.field) {
                            if let Some(val) =
                                axon_storage::extract_index_value(&f.value, &idx.index_type)
                            {
                                // Use this index; remaining filters applied post-fetch.
                                return storage.index_lookup(collection, &f.field, &val).ok();
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None, // Or, Gate — fall back to scan
    }
}

// ── Query filter helpers ──────────────────────────────────────────────────────

/// Maximum allowed nesting depth for a [`FilterNode`] tree.
///
/// Prevents stack overflows from deeply nested client-supplied filter trees.
const MAX_FILTER_DEPTH: usize = 32;

/// Return the maximum nesting depth of a [`FilterNode`] tree (1-based).
///
/// Uses an explicit stack-based iterative traversal to avoid stack overflows
/// on deeply nested client-supplied filter trees.
fn filter_depth(root: &FilterNode) -> usize {
    // Stack entries: (node, depth_of_this_node)
    let mut stack: Vec<(&FilterNode, usize)> = vec![(root, 1)];
    let mut max_depth = 0usize;
    while let Some((node, depth)) = stack.pop() {
        if depth > max_depth {
            max_depth = depth;
        }
        if let FilterNode::And { filters } | FilterNode::Or { filters } = node {
            for child in filters {
                stack.push((child, depth + 1));
            }
        }
    }
    max_depth
}

/// Evaluate a [`FilterNode`] against the entity's JSON data.
///
/// `gate_eval` is an optional pre-computed gate evaluation for the entity.
/// When `None`, any `Gate` filter nodes evaluate to `false`.
fn apply_filter(node: &FilterNode, data: &serde_json::Value) -> bool {
    apply_filter_with_gates(node, data, None)
}

fn apply_filter_with_gates(
    node: &FilterNode,
    data: &serde_json::Value,
    gate_eval: Option<&axon_schema::GateEvaluation>,
) -> bool {
    match node {
        FilterNode::Field(f) => apply_field_filter(f, data),
        FilterNode::Gate(g) => gate_eval
            .and_then(|eval| eval.gate_results.get(&g.gate))
            .is_some_and(|result| result.pass == g.pass),
        FilterNode::And { filters } => filters
            .iter()
            .all(|f| apply_filter_with_gates(f, data, gate_eval)),
        FilterNode::Or { filters } => filters
            .iter()
            .any(|f| apply_filter_with_gates(f, data, gate_eval)),
    }
}

/// Check if a filter tree contains any gate filter nodes.
fn has_gate_filter(node: &FilterNode) -> bool {
    match node {
        FilterNode::Gate(_) => true,
        FilterNode::Field(_) => false,
        FilterNode::And { filters } | FilterNode::Or { filters } => {
            filters.iter().any(has_gate_filter)
        }
    }
}

fn apply_field_filter(f: &FieldFilter, data: &serde_json::Value) -> bool {
    let field_val = get_field_value(data, &f.field);
    match &f.op {
        FilterOp::Eq => values_eq(field_val, Some(&f.value)),
        FilterOp::Ne => !values_eq(field_val, Some(&f.value)),
        FilterOp::Gt => compare_values(field_val, Some(&f.value)) == std::cmp::Ordering::Greater,
        FilterOp::Gte => {
            let ord = compare_values(field_val, Some(&f.value));
            ord == std::cmp::Ordering::Greater || ord == std::cmp::Ordering::Equal
        }
        FilterOp::Lt => compare_values(field_val, Some(&f.value)) == std::cmp::Ordering::Less,
        FilterOp::Lte => {
            let ord = compare_values(field_val, Some(&f.value));
            ord == std::cmp::Ordering::Less || ord == std::cmp::Ordering::Equal
        }
        FilterOp::In => {
            if let serde_json::Value::Array(arr) = &f.value {
                arr.iter().any(|v| values_eq(field_val, Some(v)))
            } else {
                false
            }
        }
        FilterOp::Contains => match (field_val, &f.value) {
            (Some(serde_json::Value::String(s)), serde_json::Value::String(sub)) => {
                s.contains(sub.as_str())
            }
            _ => false,
        },
    }
}

/// Resolve a dot-separated field path into a JSON value, returning `None` if missing.
fn get_field_value<'a>(data: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = data;
    for segment in path.split('.') {
        cur = cur.get(segment)?;
    }
    Some(cur)
}

/// Compute an aggregate function over a non-empty slice of f64 values.
#[allow(clippy::cast_precision_loss)]
fn compute_aggregate(func: &AggregateFunction, values: &[f64]) -> f64 {
    match func {
        AggregateFunction::Sum => values.iter().sum(),
        AggregateFunction::Avg => values.iter().sum::<f64>() / values.len() as f64,
        AggregateFunction::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
        AggregateFunction::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

fn values_eq(a: Option<&serde_json::Value>, b: Option<&serde_json::Value>) -> bool {
    match (a, b) {
        (Some(av), Some(bv)) => av == bv,
        (None, None) => true,
        _ => false,
    }
}

/// Total ordering for JSON values (numbers, strings, booleans, null).
/// Incomparable types (e.g. object vs number) are treated as equal.
fn compare_values(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    use serde_json::Value;
    use std::cmp::Ordering;
    match (a, b) {
        (Some(Value::Number(an)), Some(Value::Number(bn))) => {
            let af = an.as_f64().unwrap_or(f64::NAN);
            let bf = bn.as_f64().unwrap_or(f64::NAN);
            af.partial_cmp(&bf).unwrap_or(Ordering::Equal)
        }
        (Some(Value::String(as_)), Some(Value::String(bs))) => as_.cmp(bs),
        (Some(Value::Bool(ab)), Some(Value::Bool(bb))) => ab.cmp(bb),
        (Some(Value::Null), Some(Value::Null)) => Ordering::Equal,
        // Null sorts before everything else.
        (Some(Value::Null), Some(_)) => Ordering::Less,
        (Some(_), Some(Value::Null)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// Apply an RFC 7396 JSON Merge Patch to a target value.
///
/// - Object values are recursively merged.
/// - `null` values remove the key from the target.
/// - Non-object patches replace the target entirely.
fn json_merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    use serde_json::Value;
    if let Value::Object(patch_map) = patch {
        if !target.is_object() {
            *target = Value::Object(serde_json::Map::new());
        }
        if let Value::Object(target_map) = target {
            for (key, value) in patch_map {
                if value.is_null() {
                    target_map.remove(key);
                } else {
                    let entry = target_map.entry(key.clone()).or_insert(Value::Null);
                    json_merge_patch(entry, value);
                }
            }
        }
    } else {
        *target = patch.clone();
    }
}

/// Operation mode for lifecycle field enforcement.
///
/// Only differs on what to do with a missing value: CREATE auto-populates with
/// the lifecycle's `initial` state, UPDATE returns [`AxonError::LifecycleFieldMissing`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum LifecycleEnforcementMode {
    Create,
    Update,
}

/// Enforce lifecycle state invariants on a write (FEAT-015).
///
/// For each lifecycle defined on the collection schema:
///
/// - If the lifecycle field has no value on `data`:
///   - CREATE: auto-populate with `lifecycle.initial`.
///   - UPDATE: return [`AxonError::LifecycleFieldMissing`].
/// - If the lifecycle field has a non-string or unknown-state value:
///   return [`AxonError::LifecycleStateInvalid`].
///
/// A state is "known" if it equals `lifecycle.initial`, appears as a key in
/// `transitions`, or appears in any transitions value list.
fn enforce_lifecycle_initial_state(
    schema: &CollectionSchema,
    data: &mut serde_json::Value,
    mode: LifecycleEnforcementMode,
) -> Result<(), AxonError> {
    if schema.lifecycles.is_empty() {
        return Ok(());
    }

    for lifecycle in schema.lifecycles.values() {
        let current = data.get(&lifecycle.field).cloned();
        match current {
            None | Some(serde_json::Value::Null) => match mode {
                LifecycleEnforcementMode::Create => {
                    if let Some(obj) = data.as_object_mut() {
                        obj.insert(
                            lifecycle.field.clone(),
                            serde_json::Value::String(lifecycle.initial.clone()),
                        );
                    } else {
                        // Entity data must be an object to carry a lifecycle
                        // field; any other shape is a schema violation.
                        return Err(AxonError::LifecycleFieldMissing {
                            field: lifecycle.field.clone(),
                        });
                    }
                }
                LifecycleEnforcementMode::Update => {
                    return Err(AxonError::LifecycleFieldMissing {
                        field: lifecycle.field.clone(),
                    });
                }
            },
            Some(value) => {
                let state = match value.as_str() {
                    Some(s) => s.to_string(),
                    None => {
                        return Err(AxonError::LifecycleStateInvalid {
                            field: lifecycle.field.clone(),
                            actual: value,
                        });
                    }
                };
                if !is_known_lifecycle_state(lifecycle, &state) {
                    return Err(AxonError::LifecycleStateInvalid {
                        field: lifecycle.field.clone(),
                        actual: serde_json::Value::String(state),
                    });
                }
            }
        }
    }

    Ok(())
}

/// Returns `true` when `state` is a recognized state for `lifecycle`.
///
/// A state is recognized if it is the `initial` state, a key in `transitions`,
/// or appears in any transitions value list.
fn is_known_lifecycle_state(lifecycle: &axon_schema::schema::LifecycleDef, state: &str) -> bool {
    if lifecycle.initial == state {
        return true;
    }
    if lifecycle.transitions.contains_key(state) {
        return true;
    }
    lifecycle
        .transitions
        .values()
        .any(|targets| targets.iter().any(|t| t == state))
}

/// Check unique index constraints for an entity's data before write.
///
/// Iterates over all unique indexes in the schema and checks whether any other
/// entity in the collection already has the same indexed value.
fn check_unique_constraints<S: StorageAdapter>(
    storage: &S,
    collection: &CollectionId,
    entity_id: &EntityId,
    data: &serde_json::Value,
    schema: &CollectionSchema,
) -> Result<(), AxonError> {
    for idx in &schema.indexes {
        if !idx.unique {
            continue;
        }
        for val in extract_index_values(data, &idx.field, &idx.index_type) {
            if storage.index_unique_conflict(collection, &idx.field, &val, entity_id)? {
                return Err(AxonError::UniqueViolation {
                    field: idx.field.clone(),
                    value: val.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Build the synthetic caller used by the `putSchema` dry-run fixture path.
///
/// `req_actor` is the schema-writer's actor (from `PutSchemaRequest.actor`)
/// and is used as the default identity. `actor_override` (when present)
/// overrides actor and role so the admin UI can preview decisions as a
/// different subject. The caller always has `Role::Admin` floor: dry-run
/// runs under the schema-writer's admin authority and the override is
/// purely a predicate-evaluation alias.
fn synthetic_dry_run_caller(
    req_actor: Option<&str>,
    actor_override: Option<&ExplainActorOverride>,
) -> Result<CallerIdentity, AxonError> {
    let actor = actor_override
        .and_then(|o| o.actor.as_deref())
        .or(req_actor)
        .unwrap_or("anonymous");
    let role = match actor_override.and_then(|o| o.role.as_deref()) {
        None => axon_core::auth::Role::Admin,
        Some(role) => parse_explain_role(role)?,
    };
    Ok(CallerIdentity::new(actor, role))
}

fn parse_explain_role(role: &str) -> Result<axon_core::auth::Role, AxonError> {
    match role.trim().to_ascii_lowercase().as_str() {
        "admin" => Ok(axon_core::auth::Role::Admin),
        "write" | "writer" => Ok(axon_core::auth::Role::Write),
        "read" | "reader" => Ok(axon_core::auth::Role::Read),
        "none" => Ok(axon_core::auth::Role::None),
        other => Err(AxonError::InvalidArgument(format!(
            "unsupported actor_override role '{other}' (expected admin, write, read, or none)"
        ))),
    }
}

/// Apply an `actor_override` to a freshly built policy snapshot.
///
/// Updates the snapshot's actor and `tenant_role`/`actor`/`user_id` bindings
/// to reflect the override and merges any explicit subject bindings on top
/// of the defaults. Subject keys may be bare (`team`) or prefixed
/// (`subject.team`); the prefix is stripped before insertion.
fn apply_actor_override_subject(
    snapshot: &mut PolicyRequestSnapshot,
    actor_override: &ExplainActorOverride,
) {
    if let Some(actor) = &actor_override.actor {
        snapshot.subject.actor = actor.clone();
        snapshot
            .subject
            .bindings
            .insert("actor".into(), Value::String(actor.clone()));
        snapshot
            .subject
            .bindings
            .insert("user_id".into(), Value::String(actor.clone()));
    }
    if let Some(role) = &actor_override.role {
        snapshot
            .subject
            .bindings
            .insert("tenant_role".into(), Value::String(role.clone()));
    }
    for (key, value) in &actor_override.subject {
        let bare = key.strip_prefix("subject.").unwrap_or(key.as_str());
        snapshot
            .subject
            .bindings
            .insert(bare.to_string(), value.clone());
    }
}

fn default_policy_subject_bindings(
    actor: &str,
    caller: Option<&CallerIdentity>,
    attribution: Option<&AuditAttribution>,
    database_id: &str,
) -> HashMap<String, Value> {
    let mut bindings = HashMap::new();
    bindings.insert("actor".into(), Value::String(actor.to_string()));
    bindings.insert("database_id".into(), Value::String(database_id.to_string()));

    let user_id = attribution
        .map(|attr| attr.user_id.clone())
        .unwrap_or_else(|| actor.to_string());
    bindings.insert("user_id".into(), Value::String(user_id));

    if let Some(attr) = attribution {
        bindings.insert("tenant_id".into(), Value::String(attr.tenant_id.clone()));
        if let Some(jti) = &attr.jti {
            bindings.insert("credential_id".into(), Value::String(jti.clone()));
        }
    }

    if let Some(caller) = caller {
        bindings.insert("tenant_role".into(), Value::String(caller.role.to_string()));
    }

    bindings
}

fn resolve_policy_subject_expression(
    source: &str,
    bindings: &HashMap<String, Value>,
    attributes: &HashMap<String, Value>,
) -> Option<Value> {
    if let Some(attribute) = source.strip_prefix("subject.attributes.") {
        return attributes.get(attribute).cloned();
    }
    if let Some(binding) = source.strip_prefix("subject.") {
        return bindings.get(binding).cloned();
    }
    bindings.get(source).cloned()
}

fn required_identity_attribute_field<'a>(
    attribute_name: &str,
    field_name: &str,
    value: Option<&'a str>,
) -> Result<&'a str, AxonError> {
    value.ok_or_else(|| {
        AxonError::SchemaValidation(format!(
            "access_control identity attribute '{attribute_name}' is missing {field_name}"
        ))
    })
}

fn request_scoped_collection_id(collection: &str, database_id: &str) -> CollectionId {
    let parts = collection.split('.').count();
    if database_id != DEFAULT_DATABASE && parts <= 2 {
        CollectionId::new(Namespace::qualify_with_database(collection, database_id))
    } else {
        CollectionId::new(collection)
    }
}

fn policy_related_target_collections(policy: Option<&AccessControlPolicy>) -> Vec<String> {
    let mut output = HashSet::new();
    let Some(policy) = policy else {
        return Vec::new();
    };

    for operation in [
        policy.read.as_ref(),
        policy.create.as_ref(),
        policy.update.as_ref(),
        policy.delete.as_ref(),
        policy.write.as_ref(),
        policy.admin.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        collect_operation_policy_targets(operation, &mut output);
    }

    for field_policy in policy.fields.values() {
        collect_field_access_policy_targets(field_policy.read.as_ref(), &mut output);
        collect_field_access_policy_targets(field_policy.write.as_ref(), &mut output);
    }

    for transitions in policy.transitions.values() {
        for operation in transitions.values() {
            collect_operation_policy_targets(operation, &mut output);
        }
    }

    for envelopes in policy.envelopes.values() {
        for envelope in envelopes {
            collect_policy_predicate_targets(envelope.when.as_ref(), &mut output);
        }
    }

    output.into_iter().collect()
}

fn collect_operation_policy_targets(policy: &OperationPolicy, output: &mut HashSet<String>) {
    for rule in policy.allow.iter().chain(policy.deny.iter()) {
        collect_policy_predicate_targets(rule.when.as_ref(), output);
        collect_policy_predicate_targets(rule.where_clause.as_ref(), output);
    }
}

fn collect_field_access_policy_targets(
    policy: Option<&FieldAccessPolicy>,
    output: &mut HashSet<String>,
) {
    let Some(policy) = policy else {
        return;
    };
    for rule in policy.allow.iter().chain(policy.deny.iter()) {
        collect_policy_predicate_targets(rule.when.as_ref(), output);
        collect_policy_predicate_targets(rule.where_clause.as_ref(), output);
    }
}

fn collect_policy_predicate_targets(
    predicate: Option<&PolicyPredicate>,
    output: &mut HashSet<String>,
) {
    let Some(predicate) = predicate else {
        return;
    };
    match predicate {
        PolicyPredicate::All { all } => {
            for predicate in all {
                collect_policy_predicate_targets(Some(predicate), output);
            }
        }
        PolicyPredicate::Any { any } => {
            for predicate in any {
                collect_policy_predicate_targets(Some(predicate), output);
            }
        }
        PolicyPredicate::Not { not } => {
            collect_policy_predicate_targets(Some(not), output);
        }
        PolicyPredicate::Related { related } => {
            output.insert(related.target_collection.clone());
        }
        PolicyPredicate::SharesRelation { shares_relation } => {
            output.insert(shares_relation.collection.clone());
        }
        PolicyPredicate::Subject { .. }
        | PolicyPredicate::Field { .. }
        | PolicyPredicate::Operation { .. } => {}
    }
}

fn combine_candidate_ids(
    left: Option<Vec<EntityId>>,
    right: Option<Vec<EntityId>>,
) -> Option<Vec<EntityId>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(intersect_entity_ids(left, right)),
        (Some(ids), None) | (None, Some(ids)) => Some(ids),
        (None, None) => None,
    }
}

fn intersect_entity_ids(left: Vec<EntityId>, right: Vec<EntityId>) -> Vec<EntityId> {
    let right: std::collections::BTreeSet<EntityId> = right.into_iter().collect();
    left.into_iter()
        .filter(|entity_id| right.contains(entity_id))
        .collect()
}

fn union_entity_id_sets(sets: Vec<Vec<EntityId>>) -> Vec<EntityId> {
    let mut output = std::collections::BTreeSet::new();
    for set in sets {
        output.extend(set);
    }
    output.into_iter().collect()
}

fn entity_policy_data(entity: &Entity) -> Value {
    policy_data_with_entity_id(&entity.id, &entity.data)
}

fn policy_data_with_entity_id(entity_id: &EntityId, source: &Value) -> Value {
    let mut data = source.clone();
    if let Some(object) = data.as_object_mut() {
        object
            .entry("_id")
            .or_insert_with(|| Value::String(entity_id.to_string()));
    }
    data
}

fn apply_field_redactions(data: &mut Value, redactions: &[(String, Value)]) {
    for (field_path, redaction) in redactions {
        redact_json_path(data, field_path, redaction);
    }
}

fn apply_existing_field_redactions(data: &mut Value, redactions: &[(String, Value)]) {
    for (field_path, redaction) in redactions {
        redact_existing_json_path(data, field_path, redaction);
    }
}

fn redact_json_path(data: &mut Value, path: &str, redaction: &Value) {
    let segments: Vec<&str> = path.split('.').collect();
    redact_json_segments(data, &segments, redaction);
}

fn redact_existing_json_path(data: &mut Value, path: &str, redaction: &Value) {
    let segments: Vec<&str> = path.split('.').collect();
    redact_existing_json_segments(data, &segments, redaction);
}

fn redact_json_segments(data: &mut Value, segments: &[&str], redaction: &Value) {
    let Some((segment, rest)) = segments.split_first() else {
        return;
    };

    if let Some(array) = data.as_array_mut() {
        for item in array {
            redact_json_segments(item, segments, redaction);
        }
        return;
    }

    if let Some(field) = segment.strip_suffix("[]") {
        let Some(object) = data.as_object_mut() else {
            return;
        };
        if rest.is_empty() {
            object.insert(field.to_string(), redaction.clone());
            return;
        }
        let Some(array) = object.get_mut(field).and_then(Value::as_array_mut) else {
            return;
        };
        for item in array {
            redact_json_segments(item, rest, redaction);
        }
        return;
    }

    let Some(object) = data.as_object_mut() else {
        return;
    };
    if rest.is_empty() {
        object.insert((*segment).to_string(), redaction.clone());
        return;
    }
    let child = object
        .entry((*segment).to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    redact_json_segments(child, rest, redaction);
}

fn redact_existing_json_segments(data: &mut Value, segments: &[&str], redaction: &Value) {
    let Some((segment, rest)) = segments.split_first() else {
        return;
    };

    if let Some(array) = data.as_array_mut() {
        for item in array {
            redact_existing_json_segments(item, segments, redaction);
        }
        return;
    }

    if let Some(field) = segment.strip_suffix("[]") {
        let Some(object) = data.as_object_mut() else {
            return;
        };
        let Some(value) = object.get_mut(field) else {
            return;
        };
        if rest.is_empty() {
            *value = redaction.clone();
            return;
        }
        let Some(array) = value.as_array_mut() else {
            return;
        };
        for item in array {
            redact_existing_json_segments(item, rest, redaction);
        }
        return;
    }

    let Some(object) = data.as_object_mut() else {
        return;
    };
    let Some(value) = object.get_mut(*segment) else {
        return;
    };
    if rest.is_empty() {
        *value = redaction.clone();
        return;
    }
    redact_existing_json_segments(value, rest, redaction);
}

fn apply_diff_redactions(diff: &mut HashMap<String, FieldDiff>, redactions: &[(String, Value)]) {
    for (field_path, redaction) in redactions {
        let mut segments = field_path.split('.');
        let Some(first) = segments.next() else {
            continue;
        };
        let first = first.strip_suffix("[]").unwrap_or(first);
        let Some(field_diff) = diff.get_mut(first) else {
            continue;
        };
        let rest: Vec<&str> = segments.collect();
        if rest.is_empty() {
            field_diff.before = Some(redaction.clone());
            field_diff.after = Some(redaction.clone());
        } else {
            if let Some(before) = field_diff.before.as_mut() {
                redact_json_segments(before, &rest, redaction);
            }
            if let Some(after) = field_diff.after.as_mut() {
                redact_json_segments(after, &rest, redaction);
            }
        }
    }
}

fn apply_diff_value_redactions(diff: &mut Value, redactions: &[(String, Value)]) {
    if redactions.is_empty() {
        return;
    }
    let Ok(mut diff_map) = serde_json::from_value::<HashMap<String, FieldDiff>>(diff.clone())
    else {
        return;
    };
    apply_diff_redactions(&mut diff_map, redactions);
    if let Ok(redacted) = serde_json::to_value(diff_map) {
        *diff = redacted;
    }
}

fn apply_review_summary_diff_redactions(diff: &mut Value, redactions: &[(String, Value)]) {
    if let Some(entries) = diff.as_array_mut() {
        for entry in entries {
            if let Some(child_diff) = entry.get_mut("diff") {
                apply_diff_value_redactions(child_diff, redactions);
            }
        }
        return;
    }
    apply_diff_value_redactions(diff, redactions);
}

fn intent_operation_entity_ref(operation: &Value) -> Option<(CollectionId, EntityId)> {
    Some((
        CollectionId::new(operation.get("collection")?.as_str()?),
        EntityId::new(operation.get("id")?.as_str()?),
    ))
}

fn transaction_child_operation_kind(operation: &Value) -> Option<MutationOperationKind> {
    match operation.get("op")?.as_str()? {
        "create_entity" => Some(MutationOperationKind::CreateEntity),
        "update_entity" => Some(MutationOperationKind::UpdateEntity),
        "delete_entity" => Some(MutationOperationKind::DeleteEntity),
        "patch_entity" => Some(MutationOperationKind::PatchEntity),
        "transition" => Some(MutationOperationKind::Transition),
        "rollback" => Some(MutationOperationKind::Rollback),
        "revert" => Some(MutationOperationKind::Revert),
        "create_link" => Some(MutationOperationKind::CreateLink),
        "delete_link" => Some(MutationOperationKind::DeleteLink),
        "transaction" => Some(MutationOperationKind::Transaction),
        _ => None,
    }
}

fn transaction_child_diff_mut(diff: &mut Value, operation_index: usize) -> Option<&mut Value> {
    diff.as_array_mut()?
        .iter_mut()
        .find(|entry| {
            entry
                .get("operationIndex")
                .and_then(Value::as_u64)
                .map(|index| index as usize == operation_index)
                .unwrap_or(false)
        })?
        .get_mut("diff")
}

fn policy_static_predicate_matches(
    predicate: &CompiledPredicate,
    snapshot: &PolicyRequestSnapshot,
    operation: &PolicyOperation,
) -> Option<bool> {
    match predicate {
        CompiledPredicate::All(items) => {
            let mut saw_unknown = false;
            for item in items {
                match policy_static_predicate_matches(item, snapshot, operation) {
                    Some(true) => {}
                    Some(false) => return Some(false),
                    None => saw_unknown = true,
                }
            }
            (!saw_unknown).then_some(true)
        }
        CompiledPredicate::Any(items) => {
            let mut saw_unknown = false;
            for item in items {
                match policy_static_predicate_matches(item, snapshot, operation) {
                    Some(true) => return Some(true),
                    Some(false) => {}
                    None => saw_unknown = true,
                }
            }
            (!saw_unknown).then_some(false)
        }
        CompiledPredicate::Not(item) => {
            policy_static_predicate_matches(item, snapshot, operation).map(|value| !value)
        }
        CompiledPredicate::Operation(candidate) => Some(candidate == operation),
        CompiledPredicate::Compare(comparison) => {
            if !matches!(comparison.target, PredicateTarget::Subject(_)) {
                return None;
            }
            let ctx = PolicyPredicateContext {
                snapshot,
                operation,
                data: &Value::Null,
                preview: None,
            };
            Some(policy_comparison_matches(comparison, ctx))
        }
        _ => None,
    }
}

fn optional_policy_static_predicate_matches(
    predicate: Option<&CompiledPredicate>,
    snapshot: &PolicyRequestSnapshot,
    operation: &PolicyOperation,
) -> Option<bool> {
    predicate.map_or(Some(true), |predicate| {
        policy_static_predicate_matches(predicate, snapshot, operation)
    })
}

fn combine_static_and(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn policy_rule_static_matches(
    rule: &CompiledPolicyRule,
    snapshot: &PolicyRequestSnapshot,
    operation: &PolicyOperation,
) -> Option<bool> {
    combine_static_and(
        optional_policy_static_predicate_matches(rule.when.as_ref(), snapshot, operation),
        optional_policy_static_predicate_matches(rule.where_clause.as_ref(), snapshot, operation),
    )
}

fn field_policy_rule_static_matches(
    rule: &CompiledFieldPolicyRule,
    snapshot: &PolicyRequestSnapshot,
    operation: &PolicyOperation,
) -> Option<bool> {
    combine_static_and(
        optional_policy_static_predicate_matches(rule.when.as_ref(), snapshot, operation),
        optional_policy_static_predicate_matches(rule.where_clause.as_ref(), snapshot, operation),
    )
}

fn effective_operation_allows_static(
    plan: &PolicyPlan,
    snapshot: &PolicyRequestSnapshot,
    operation: PolicyOperation,
) -> bool {
    let policies = applicable_operation_policies(plan, &operation);
    let mut allow_rules_present = false;

    for policy in &policies {
        for rule in &policy.deny {
            if policy_rule_static_matches(rule, snapshot, &operation) == Some(true) {
                return false;
            }
        }
        allow_rules_present |= !policy.allow.is_empty();
    }

    !allow_rules_present
        || policies.iter().any(|policy| {
            policy
                .allow
                .iter()
                .any(|rule| policy_rule_static_matches(rule, snapshot, &operation) != Some(false))
        })
}

fn effective_field_read_redacted_static(
    policy: &CompiledFieldAccessPolicy,
    snapshot: &PolicyRequestSnapshot,
) -> bool {
    for rule in &policy.deny {
        if field_policy_rule_static_matches(rule, snapshot, &PolicyOperation::Read) != Some(false) {
            return true;
        }
    }

    !policy.allow.is_empty()
        && !policy.allow.iter().any(|rule| {
            field_policy_rule_static_matches(rule, snapshot, &PolicyOperation::Read) != Some(false)
        })
}

fn effective_field_write_denied_static(
    policy: &CompiledFieldAccessPolicy,
    snapshot: &PolicyRequestSnapshot,
    operation: &PolicyOperation,
) -> bool {
    for rule in &policy.deny {
        if field_policy_rule_static_matches(rule, snapshot, operation) != Some(false) {
            return true;
        }
    }

    !policy.allow.is_empty()
        && !policy
            .allow
            .iter()
            .any(|rule| field_policy_rule_static_matches(rule, snapshot, operation) != Some(false))
}

fn effective_field_write_denied_for_data<S: StorageAdapter>(
    handler: &AxonHandler<S>,
    policy: &CompiledFieldAccessPolicy,
    snapshot: &PolicyRequestSnapshot,
    operation: &PolicyOperation,
    data: &Value,
    preview: Option<&PreviewedSchemaPlan<'_>>,
) -> Result<bool, AxonError> {
    for rule in &policy.deny {
        if handler.field_policy_rule_matches(rule, snapshot, operation, data, preview)? {
            return Ok(true);
        }
    }

    if !policy.allow.is_empty() {
        let mut matched = false;
        for rule in &policy.allow {
            if handler.field_policy_rule_matches(rule, snapshot, operation, data, preview)? {
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(true);
        }
    }

    Ok(false)
}

fn applicable_operation_policies<'a>(
    plan: &'a PolicyPlan,
    operation: &PolicyOperation,
) -> Vec<&'a CompiledOperationPolicy> {
    let mut policies = Vec::new();
    if let Some(policy) = plan.operations.get(operation) {
        policies.push(policy);
    }
    if matches!(
        operation,
        PolicyOperation::Create | PolicyOperation::Update | PolicyOperation::Delete
    ) {
        if let Some(policy) = plan.operations.get(&PolicyOperation::Write) {
            policies.push(policy);
        }
    }
    policies
}

fn applicable_policy_envelopes<'a>(
    plan: &'a PolicyPlan,
    operation: &PolicyOperation,
) -> Vec<&'a CompiledPolicyEnvelope> {
    let mut envelopes = Vec::new();
    if let Some(items) = plan.envelopes.get(operation) {
        envelopes.extend(items);
    }
    if matches!(
        operation,
        PolicyOperation::Create | PolicyOperation::Update | PolicyOperation::Delete
    ) {
        if let Some(items) = plan.envelopes.get(&PolicyOperation::Write) {
            envelopes.extend(items);
        }
    }
    envelopes
}

fn policy_rule_label(rule: &CompiledPolicyRule) -> String {
    rule.name.clone().unwrap_or_else(|| rule.rule_id.clone())
}

fn field_policy_rule_label(rule: &CompiledFieldPolicyRule) -> String {
    rule.name.clone().unwrap_or_else(|| rule.rule_id.clone())
}

fn policy_envelope_label(envelope: &CompiledPolicyEnvelope) -> String {
    envelope
        .name
        .clone()
        .unwrap_or_else(|| envelope.envelope_id.clone())
}

#[allow(clippy::too_many_arguments)]
fn policy_explanation(
    operation: impl Into<String>,
    collection: Option<&CollectionId>,
    entity_id: Option<&EntityId>,
    operation_index: Option<usize>,
    decision: impl Into<String>,
    reason: impl Into<String>,
    policy_version: u32,
) -> PolicyExplanationResponse {
    PolicyExplanationResponse {
        operation: operation.into(),
        collection: collection.map(ToString::to_string),
        entity_id: entity_id.map(ToString::to_string),
        operation_index,
        decision: decision.into(),
        reason: reason.into(),
        policy_version,
        rule_ids: Vec::new(),
        policy_ids: Vec::new(),
        field_paths: Vec::new(),
        denied_fields: Vec::new(),
        rules: Vec::new(),
        approval: None,
        operations: Vec::new(),
    }
}

fn operation_rule_match(rule: &CompiledPolicyRule, kind: impl Into<String>) -> PolicyRuleMatch {
    PolicyRuleMatch {
        rule_id: rule.rule_id.clone(),
        name: rule.name.clone(),
        kind: kind.into(),
        field_path: None,
    }
}

fn field_rule_match(
    rule: &CompiledFieldPolicyRule,
    field_path: &str,
    kind: impl Into<String>,
) -> PolicyRuleMatch {
    PolicyRuleMatch {
        rule_id: rule.rule_id.clone(),
        name: rule.name.clone(),
        kind: kind.into(),
        field_path: Some(field_path.to_string()),
    }
}

fn approval_envelope_summary(envelope: &CompiledPolicyEnvelope) -> PolicyApprovalEnvelopeSummary {
    PolicyApprovalEnvelopeSummary {
        policy_id: envelope.envelope_id.clone(),
        name: envelope.name.clone(),
        decision: policy_decision_name(&envelope.decision).to_string(),
        role: envelope
            .approval
            .as_ref()
            .and_then(|approval| approval.role.clone()),
        reason_required: envelope
            .approval
            .as_ref()
            .is_some_and(|approval| approval.reason_required),
        deadline_seconds: envelope
            .approval
            .as_ref()
            .and_then(|approval| approval.deadline_seconds),
        separation_of_duties: envelope
            .approval
            .as_ref()
            .is_some_and(|approval| approval.separation_of_duties),
    }
}

fn policy_decision_name(decision: &PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Allow => "allow",
        PolicyDecision::NeedsApproval => "needs_approval",
        PolicyDecision::Deny => "deny",
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn merge_policy_explanation(
    target: &mut PolicyExplanationResponse,
    mut source: PolicyExplanationResponse,
) {
    for rule_id in source.rule_ids.drain(..) {
        push_unique(&mut target.rule_ids, rule_id);
    }
    for policy_id in source.policy_ids.drain(..) {
        push_unique(&mut target.policy_ids, policy_id);
    }
    for field_path in source.field_paths.drain(..) {
        push_unique(&mut target.field_paths, field_path);
    }
    for field_path in source.denied_fields.drain(..) {
        push_unique(&mut target.denied_fields, field_path);
    }
    target.rules.append(&mut source.rules);
    if target.approval.is_none() {
        target.approval = source.approval;
    }
}

fn finalize_policy_explanation(
    mut response: PolicyExplanationResponse,
) -> PolicyExplanationResponse {
    for rule in &response.rules {
        push_unique(&mut response.rule_ids, rule.rule_id.clone());
        if let Some(field_path) = &rule.field_path {
            push_unique(&mut response.field_paths, field_path.clone());
        }
    }
    if let Some(approval) = &response.approval {
        push_unique(&mut response.policy_ids, approval.policy_id.clone());
    }

    let mut seen_rules = HashSet::new();
    response.rules.retain(|rule| {
        seen_rules.insert(format!(
            "{}\u{1f}{}\u{1f}{}",
            rule.rule_id,
            rule.kind,
            rule.field_path.as_deref().unwrap_or("")
        ))
    });
    response.rule_ids.sort();
    response.policy_ids.sort();
    response.field_paths.sort();
    response.denied_fields.sort();
    response.rule_ids.dedup();
    response.policy_ids.dedup();
    response.field_paths.dedup();
    response.denied_fields.dedup();
    response
}

fn policy_forbidden(
    reason: &str,
    collection: &CollectionId,
    entity_id: Option<&EntityId>,
    field_path: Option<&str>,
    policy: Option<String>,
    operation_index: Option<usize>,
) -> AxonError {
    let mut denial = PolicyDenial::new(reason, collection.to_string());
    if let Some(entity_id) = entity_id {
        denial.entity_id = Some(entity_id.to_string());
    }
    if let Some(field_path) = field_path {
        denial.field_path = Some(field_path.to_string());
    }
    if let Some(policy) = policy {
        denial.policy = Some(policy);
    }
    if let Some(operation_index) = operation_index {
        denial.operation_index = Some(operation_index);
    }
    AxonError::PolicyDenied(Box::new(denial))
}

fn field_write_scope_touches_path(scope: FieldWriteScope<'_>, field_path: &str) -> bool {
    match scope {
        FieldWriteScope::PresentFields(data) => !policy_values_at_path(data, field_path).is_empty(),
        FieldWriteScope::Patch(patch) => patch_touches_field_path(patch, field_path),
    }
}

fn patch_touches_field_path(patch: &Value, field_path: &str) -> bool {
    let segments: Vec<&str> = field_path
        .split('.')
        .map(|segment| segment.strip_suffix("[]").unwrap_or(segment))
        .collect();
    patch_touches_segments(patch, &segments)
}

fn patch_touches_segments(patch: &Value, segments: &[&str]) -> bool {
    if segments.is_empty() {
        return true;
    }
    let Some(object) = patch.as_object() else {
        return true;
    };
    let Some(child) = object.get(segments[0]) else {
        return false;
    };
    if segments.len() == 1 || child.is_null() {
        return true;
    }
    patch_touches_segments(child, &segments[1..])
}

fn policy_comparison_matches(
    comparison: &CompiledComparison,
    ctx: PolicyPredicateContext<'_>,
) -> bool {
    let field_values;
    let mut subject_values = Vec::new();
    let values: Vec<&Value> = match &comparison.target {
        PredicateTarget::Field(path) => {
            field_values = policy_values_at_path(ctx.data, path);
            field_values
        }
        PredicateTarget::Subject(name) => {
            if let Some(value) = policy_subject_value(ctx.snapshot, name) {
                subject_values.push(value);
            }
            subject_values
        }
    };

    match &comparison.op {
        CompiledCompareOp::Eq(expected) => values.contains(&expected),
        CompiledCompareOp::Ne(expected) => !values.contains(&expected),
        CompiledCompareOp::In(expected) => values
            .iter()
            .any(|value| expected.iter().any(|candidate| candidate == *value)),
        CompiledCompareOp::NotNull(expected) => {
            let matched = values.iter().any(|value| !value.is_null());
            matched == *expected
        }
        CompiledCompareOp::IsNull(expected) => {
            let matched = values.is_empty() || values.iter().all(|value| value.is_null());
            matched == *expected
        }
        CompiledCompareOp::Gt(expected) => values.iter().any(|value| {
            compare_values(Some(*value), Some(expected)) == std::cmp::Ordering::Greater
        }),
        CompiledCompareOp::Gte(expected) => values.iter().any(|value| {
            let ord = compare_values(Some(*value), Some(expected));
            ord == std::cmp::Ordering::Greater || ord == std::cmp::Ordering::Equal
        }),
        CompiledCompareOp::Lt(expected) => values
            .iter()
            .any(|value| compare_values(Some(*value), Some(expected)) == std::cmp::Ordering::Less),
        CompiledCompareOp::Lte(expected) => values.iter().any(|value| {
            let ord = compare_values(Some(*value), Some(expected));
            ord == std::cmp::Ordering::Less || ord == std::cmp::Ordering::Equal
        }),
        CompiledCompareOp::ContainsSubject(subject) => {
            let Some(subject) = policy_subject_value(ctx.snapshot, subject) else {
                return false;
            };
            values
                .iter()
                .any(|value| policy_value_contains_subject(value, subject))
        }
        CompiledCompareOp::EqSubject(subject) => {
            let Some(subject) = policy_subject_value(ctx.snapshot, subject) else {
                return false;
            };
            values.contains(&subject)
        }
    }
}

fn policy_subject_value<'a>(
    snapshot: &'a PolicyRequestSnapshot,
    subject: &str,
) -> Option<&'a Value> {
    snapshot
        .subject
        .bindings
        .get(subject)
        .or_else(|| snapshot.subject.attributes.get(subject))
}

fn policy_value_contains_subject(value: &Value, subject: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(|item| item == subject),
        other => other == subject,
    }
}

fn policy_values_at_path<'a>(data: &'a Value, path: &str) -> Vec<&'a Value> {
    let segments: Vec<&str> = path.split('.').collect();
    let mut values = Vec::new();
    collect_policy_values_at_path(data, &segments, &mut values);
    values
}

fn collect_policy_values_at_path<'a>(
    value: &'a Value,
    segments: &[&str],
    values: &mut Vec<&'a Value>,
) {
    let Some((segment, rest)) = segments.split_first() else {
        values.push(value);
        return;
    };

    if let Some(field) = segment.strip_suffix("[]") {
        let Some(array) = value.get(field).and_then(Value::as_array) else {
            return;
        };
        for item in array {
            collect_policy_values_at_path(item, rest, values);
        }
    } else if let Some(next) = value.get(*segment) {
        collect_policy_values_at_path(next, rest, values);
    }
}

#[cfg(test)]
#[allow(clippy::manual_string_new, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fmt::Display;

    use axon_core::auth::Role;
    use axon_core::id::{CollectionId, EntityId, Namespace};
    use axon_schema::schema::{
        Cardinality, CollectionSchema, CollectionView, EsfDocument, IndexDef, IndexType,
        LinkTypeDef, NamedQueryDef,
    };
    use axon_schema::{
        AccessControlPolicy, FieldAccessPolicy, FieldPolicy, FieldPolicyRule,
        IdentityAttributeSource, OperationPolicy, PolicyCompareOp, PolicyDecision, PolicyEnvelope,
        PolicyPredicate, PolicyRule,
    };
    use axon_storage::adapter::StorageAdapter;
    use axon_storage::memory::MemoryStorageAdapter;
    use serde_json::json;

    use crate::test_fixtures::seed_procurement_fixture;

    fn handler() -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new(MemoryStorageAdapter::default())
    }

    fn handler_with_markdown_template_cache_capacity(
        capacity: usize,
    ) -> AxonHandler<MemoryStorageAdapter> {
        AxonHandler::new_with_markdown_template_cache_capacity(
            MemoryStorageAdapter::default(),
            capacity,
        )
    }

    #[derive(Default)]
    struct RaceOnCreateIfAbsentAdapter {
        inner: MemoryStorageAdapter,
        injected_conflict: bool,
    }

    impl StorageAdapter for RaceOnCreateIfAbsentAdapter {
        fn get(
            &self,
            collection: &CollectionId,
            id: &EntityId,
        ) -> Result<Option<Entity>, AxonError> {
            self.inner.get(collection, id)
        }

        fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
            self.inner.put(entity)
        }

        fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
            self.inner.delete(collection, id)
        }

        fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
            self.inner.count(collection)
        }

        fn range_scan(
            &self,
            collection: &CollectionId,
            start: Option<&EntityId>,
            end: Option<&EntityId>,
            limit: Option<usize>,
        ) -> Result<Vec<Entity>, AxonError> {
            self.inner.range_scan(collection, start, end, limit)
        }

        fn compare_and_swap(
            &mut self,
            entity: Entity,
            expected_version: u64,
        ) -> Result<Entity, AxonError> {
            self.inner.compare_and_swap(entity, expected_version)
        }

        fn create_if_absent(
            &mut self,
            entity: Entity,
            expected_absent_version: u64,
        ) -> Result<Entity, AxonError> {
            if !self.injected_conflict {
                self.injected_conflict = true;
                self.inner.put(Entity {
                    collection: entity.collection.clone(),
                    id: entity.id.clone(),
                    version: expected_absent_version + 1,
                    data: json!({"title": "concurrent"}),
                    created_at_ns: entity.created_at_ns,
                    updated_at_ns: entity.updated_at_ns,
                    created_by: Some("racer".into()),
                    updated_by: Some("racer".into()),
                    schema_version: None,
                    gate_results: Default::default(),
                })?;
            }

            self.inner.create_if_absent(entity, expected_absent_version)
        }
    }

    #[derive(Default)]
    struct SchemaReadCountingAdapter {
        inner: MemoryStorageAdapter,
        schema_reads: Mutex<usize>,
    }

    impl SchemaReadCountingAdapter {
        fn schema_reads(&self) -> usize {
            *self.schema_reads.lock().expect("schema read counter")
        }
    }

    impl StorageAdapter for SchemaReadCountingAdapter {
        fn get(
            &self,
            collection: &CollectionId,
            id: &EntityId,
        ) -> Result<Option<Entity>, AxonError> {
            self.inner.get(collection, id)
        }

        fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
            self.inner.put(entity)
        }

        fn delete(&mut self, collection: &CollectionId, id: &EntityId) -> Result<(), AxonError> {
            self.inner.delete(collection, id)
        }

        fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
            self.inner.count(collection)
        }

        fn range_scan(
            &self,
            collection: &CollectionId,
            start: Option<&EntityId>,
            end: Option<&EntityId>,
            limit: Option<usize>,
        ) -> Result<Vec<Entity>, AxonError> {
            self.inner.range_scan(collection, start, end, limit)
        }

        fn compare_and_swap(
            &mut self,
            entity: Entity,
            expected_version: u64,
        ) -> Result<Entity, AxonError> {
            self.inner.compare_and_swap(entity, expected_version)
        }

        fn create_if_absent(
            &mut self,
            entity: Entity,
            expected_absent_version: u64,
        ) -> Result<Entity, AxonError> {
            self.inner.create_if_absent(entity, expected_absent_version)
        }

        fn put_schema(&mut self, schema: &CollectionSchema) -> Result<(), AxonError> {
            self.inner.put_schema(schema)
        }

        fn get_schema(
            &self,
            collection: &CollectionId,
        ) -> Result<Option<CollectionSchema>, AxonError> {
            *self.schema_reads.lock().expect("schema read counter") += 1;
            self.inner.get_schema(collection)
        }
    }

    fn register_prod_billing_and_engineering_collection(
        h: &mut AxonHandler<MemoryStorageAdapter>,
        collection: &str,
    ) -> (CollectionId, CollectionId) {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest};
        use axon_core::id::Namespace;

        let bare = CollectionId::new(collection);
        let billing = CollectionId::new(format!("prod.billing.{collection}"));
        let engineering = CollectionId::new(format!("prod.engineering.{collection}"));

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        for schema in ["billing", "engineering"] {
            h.create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: schema.into(),
            })
            .unwrap();
        }
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "billing"))
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "engineering"))
            .unwrap();

        (billing, engineering)
    }

    fn ok_or_panic<T, E: Display>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{context}: {err}"),
        }
    }

    fn err_or_panic<T, E: Display>(result: Result<T, E>, context: &str) -> E {
        match result {
            Ok(_) => panic!("{context}: expected error"),
            Err(err) => err,
        }
    }

    fn assert_rendered_markdown(response: GetEntityMarkdownResponse, expected: &str) {
        match response {
            GetEntityMarkdownResponse::Rendered {
                rendered_markdown, ..
            } => assert_eq!(rendered_markdown, expected),
            GetEntityMarkdownResponse::RenderFailed { detail, .. } => {
                panic!("expected markdown render to succeed: {detail}")
            }
        }
    }

    fn policy_snapshot_schema(collection: &str) -> CollectionSchema {
        CollectionSchema {
            collection: CollectionId::new(collection),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"}
                },
                "required": ["title"]
            })),
            link_types: Default::default(),
            access_control: Some(AccessControlPolicy {
                identity: Some(AccessControlIdentity {
                    subject: HashMap::from([
                        ("user_id".into(), "subject.user_id".into()),
                        ("tenant_role".into(), "subject.tenant_role".into()),
                    ]),
                    attributes: HashMap::from([(
                        "app_role".into(),
                        IdentityAttributeSource {
                            from: "collection".into(),
                            collection: Some("users".into()),
                            key_field: Some("id".into()),
                            key_subject: Some("user_id".into()),
                            value_field: Some("role".into()),
                        },
                    )]),
                    aliases: HashMap::new(),
                }),
                ..Default::default()
            }),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        }
    }

    #[test]
    fn policy_snapshot_resolves_collection_backed_attributes_per_request() {
        let mut h = handler();
        let users = CollectionId::new("users");
        let orders = CollectionId::new("purchase_orders");

        h.create_entity(CreateEntityRequest {
            collection: users.clone(),
            id: EntityId::new("user-alice"),
            data: json!({"id": "user-alice", "role": "contractor"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed user");
        h.put_schema(policy_snapshot_schema("purchase_orders"))
            .expect("policy schema");

        let caller = CallerIdentity::new("alice@example.test", Role::Write);
        let attribution = AuditAttribution {
            user_id: "user-alice".into(),
            tenant_id: "tenant-acme".into(),
            jti: Some("credential-1".into()),
            auth_method: "jwt".into(),
        };

        let first = h
            .create_entity_with_caller(
                CreateEntityRequest {
                    collection: orders.clone(),
                    id: EntityId::new("po-1"),
                    data: json!({"title": "first"}),
                    actor: Some("spoofed".into()),
                    audit_metadata: None,
                    attribution: None,
                },
                &caller,
                Some(attribution.clone()),
            )
            .expect("first create");
        let first_snapshot = first
            .policy_snapshot
            .expect("policy snapshot should be present");
        assert_eq!(first_snapshot.schema_version, Some(1));
        assert_eq!(first_snapshot.policy_version, Some(1));
        assert_eq!(first_snapshot.database_id, "default");
        assert_eq!(first_snapshot.tenant_id.as_deref(), Some("tenant-acme"));
        assert_eq!(first_snapshot.subject.actor, "alice@example.test");
        assert_eq!(
            first_snapshot.subject.bindings.get("user_id"),
            Some(&json!("user-alice"))
        );
        assert_eq!(
            first_snapshot.subject.bindings.get("tenant_role"),
            Some(&json!("write"))
        );
        assert_eq!(
            first_snapshot.subject.bindings.get("credential_id"),
            Some(&json!("credential-1"))
        );
        assert_eq!(
            first_snapshot.subject.attributes.get("app_role"),
            Some(&json!("contractor"))
        );

        h.update_entity(UpdateEntityRequest {
            collection: users,
            id: EntityId::new("user-alice"),
            data: json!({"id": "user-alice", "role": "finance"}),
            expected_version: 1,
            actor: Some("admin".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("update user role");

        let second = h
            .create_entity_with_caller(
                CreateEntityRequest {
                    collection: orders,
                    id: EntityId::new("po-2"),
                    data: json!({"title": "second"}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                },
                &caller,
                Some(attribution),
            )
            .expect("second create");
        let second_snapshot = second
            .policy_snapshot
            .expect("policy snapshot should be present");
        assert_eq!(
            first_snapshot.subject.attributes.get("app_role"),
            Some(&json!("contractor"))
        );
        assert_eq!(
            second_snapshot.subject.attributes.get("app_role"),
            Some(&json!("finance"))
        );
    }

    #[test]
    fn policy_snapshot_reads_schema_once_and_reuses_version_for_create() {
        let mut h = AxonHandler::new(SchemaReadCountingAdapter::default());
        h.put_schema(policy_snapshot_schema("purchase_orders"))
            .expect("policy schema");

        let created = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("purchase_orders"),
                id: EntityId::new("po-1"),
                data: json!({"title": "first"}),
                actor: Some("alice".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect("create entity");

        assert_eq!(h.storage_ref().schema_reads(), 1);
        assert_eq!(created.entity.schema_version, Some(1));
        let snapshot = created
            .policy_snapshot
            .expect("policy snapshot should be present");
        assert_eq!(snapshot.schema_version, Some(1));
        assert_eq!(snapshot.policy_version, Some(1));
    }

    fn policy_test_schema(
        collection: &str,
        mut access_control: AccessControlPolicy,
    ) -> CollectionSchema {
        access_control.identity = Some(AccessControlIdentity {
            subject: HashMap::from([("user_id".into(), "subject.user_id".into())]),
            attributes: HashMap::new(),
            aliases: HashMap::new(),
        });
        CollectionSchema {
            collection: CollectionId::new(collection),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "owner_id": {"type": "string"},
                    "title": {"type": "string"},
                    "status": {"type": "string", "enum": ["draft", "submitted", "approved"]},
                    "secret": {"type": "string"},
                    "amount": {"type": "integer"}
                }
            })),
            link_types: Default::default(),
            access_control: Some(access_control),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        }
    }

    fn allow_all_policy() -> OperationPolicy {
        OperationPolicy {
            allow: vec![PolicyRule {
                name: Some("allow-all".into()),
                ..Default::default()
            }],
            deny: vec![],
        }
    }

    fn owner_read_policy() -> OperationPolicy {
        OperationPolicy {
            allow: vec![PolicyRule {
                name: Some("owners-read".into()),
                when: None,
                where_clause: Some(PolicyPredicate::Field {
                    field: "owner_id".into(),
                    op: PolicyCompareOp::EqSubject("user_id".into()),
                }),
            }],
            deny: vec![],
        }
    }

    fn policy_visibility_graph_schema(collection: &CollectionId) -> CollectionSchema {
        let mut schema = policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                read: Some(owner_read_policy()),
                ..Default::default()
            },
        );
        schema.link_types.insert(
            "edge".into(),
            LinkTypeDef {
                target_collection: collection.to_string(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );
        schema
    }

    fn seed_policy_node(
        h: &mut AxonHandler<MemoryStorageAdapter>,
        collection: &CollectionId,
        id: &str,
        owner_id: &str,
        title: &str,
    ) {
        h.create_entity(CreateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new(id),
            data: json!({"owner_id": owner_id, "title": title}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed policy node");
    }

    fn link_policy_nodes(
        h: &mut AxonHandler<MemoryStorageAdapter>,
        collection: &CollectionId,
        source_id: &str,
        target_id: &str,
    ) {
        h.create_link(CreateLinkRequest {
            source_collection: collection.clone(),
            source_id: EntityId::new(source_id),
            target_collection: collection.clone(),
            target_id: EntityId::new(target_id),
            link_type: "edge".into(),
            metadata: json!({}),
            actor: None,
            attribution: None,
        })
        .expect("seed policy link");
    }

    fn denied_field_policy(field: &str) -> FieldPolicy {
        FieldPolicy {
            read: None,
            write: Some(FieldAccessPolicy {
                allow: vec![],
                deny: vec![FieldPolicyRule {
                    name: Some(format!("blocked-cannot-write-{field}")),
                    when: Some(PolicyPredicate::Subject {
                        subject: "user_id".into(),
                        op: PolicyCompareOp::Eq(json!("blocked")),
                    }),
                    where_clause: None,
                    redact_as: None,
                }],
            }),
        }
    }

    fn redacted_field_policy(field: &str) -> FieldPolicy {
        FieldPolicy {
            read: Some(FieldAccessPolicy {
                allow: vec![],
                deny: vec![FieldPolicyRule {
                    name: Some(format!("blocked-cannot-read-{field}")),
                    when: Some(PolicyPredicate::Subject {
                        subject: "user_id".into(),
                        op: PolicyCompareOp::Eq(json!("blocked")),
                    }),
                    where_clause: None,
                    redact_as: Some(Value::Null),
                }],
            }),
            write: None,
        }
    }

    fn assert_policy_forbidden(err: AxonError, reason: &str, field_path: Option<&str>) {
        let text = err.to_string();
        assert!(text.contains("forbidden:"), "unexpected error: {text}");
        assert!(text.contains(reason), "missing reason {reason}: {text}");
        if let AxonError::PolicyDenied(denial) = &err {
            assert_eq!(denial.reason, reason);
            assert!(
                !denial.collection.is_empty(),
                "policy denial should include collection"
            );
            if let Some(field_path) = field_path {
                assert_eq!(denial.field_path.as_deref(), Some(field_path));
            }
        }
        if let Some(field_path) = field_path {
            assert!(
                text.contains(&format!("field_path={field_path}")),
                "missing field path {field_path}: {text}"
            );
        }
    }

    fn expect_policy_denial(err: AxonError) -> PolicyDenial {
        match err {
            AxonError::PolicyDenied(denial) => *denial,
            other => panic!("expected structured policy denial, got {other:?}"),
        }
    }

    #[test]
    fn policy_denials_expose_stable_structured_details() {
        let mut h = handler();
        let collection = CollectionId::new("policy_denial_details");
        h.put_schema(policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                create: Some(allow_all_policy()),
                read: Some(owner_read_policy()),
                update: Some(allow_all_policy()),
                fields: HashMap::from([("secret".into(), denied_field_policy("secret"))]),
                ..Default::default()
            },
        ))
        .expect("policy schema");

        let field_denial = expect_policy_denial(
            h.create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("field-denied"),
                data: json!({"owner_id": "blocked", "title": "new", "secret": "classified"}),
                actor: Some("blocked".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("field write should be denied"),
        );
        assert_eq!(field_denial.reason, "field_write_denied");
        assert_eq!(field_denial.collection, collection.to_string());
        assert_eq!(field_denial.entity_id.as_deref(), Some("field-denied"));
        assert_eq!(field_denial.field_path.as_deref(), Some("secret"));
        assert_eq!(
            field_denial.policy.as_deref(),
            Some("blocked-cannot-write-secret")
        );
        assert_eq!(field_denial.operation_index, None);

        h.create_entity(CreateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new("doc-1"),
            data: json!({"owner_id": "alice", "title": "seed", "secret": "v1"}),
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed row");

        let row_denial = expect_policy_denial(
            h.update_entity(UpdateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-1"),
                data: json!({"owner_id": "alice", "title": "changed", "secret": "v1"}),
                expected_version: 1,
                actor: Some("bob".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("hidden row update should be denied"),
        );
        assert_eq!(row_denial.reason, "row_write_denied");
        assert_eq!(row_denial.collection, collection.to_string());
        assert_eq!(row_denial.entity_id.as_deref(), Some("doc-1"));
        assert_eq!(row_denial.field_path, None);
        assert_eq!(row_denial.policy.as_deref(), Some("read"));

        let mut tx = crate::transaction::Transaction::new();
        tx.create(Entity::new(
            collection.clone(),
            EntityId::new("tx-allowed"),
            json!({"owner_id": "blocked", "title": "allowed"}),
        ))
        .expect("stage allowed create");
        tx.create(Entity::new(
            collection.clone(),
            EntityId::new("tx-denied"),
            json!({"owner_id": "blocked", "title": "denied", "secret": "classified"}),
        ))
        .expect("stage denied create");

        let tx_denial = expect_policy_denial(
            h.commit_transaction(tx, Some("blocked".into()), None)
                .expect_err("transaction should be denied"),
        );
        assert_eq!(tx_denial.reason, "field_write_denied");
        assert_eq!(tx_denial.collection, collection.to_string());
        assert_eq!(tx_denial.entity_id.as_deref(), Some("tx-denied"));
        assert_eq!(tx_denial.field_path.as_deref(), Some("secret"));
        assert_eq!(tx_denial.operation_index, Some(1));
    }

    #[test]
    fn policy_denies_hidden_row_mutation_without_audit() {
        let mut h = handler();
        let collection = CollectionId::new("policy_docs");
        h.put_schema(policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                read: Some(owner_read_policy()),
                update: Some(allow_all_policy()),
                ..Default::default()
            },
        ))
        .expect("policy schema");
        h.create_entity(CreateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new("doc-1"),
            data: json!({"owner_id": "alice", "title": "private"}),
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed owner row");
        let audit_before = h.audit_log().entries().len();

        let err = h
            .update_entity(UpdateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-1"),
                data: json!({"owner_id": "alice", "title": "changed"}),
                expected_version: 1,
                actor: Some("bob".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("hidden row update should be denied");

        assert_policy_forbidden(err, "row_write_denied", None);
        assert_eq!(h.audit_log().entries().len(), audit_before);
        let stored = h
            .storage_ref()
            .get(&collection, &EntityId::new("doc-1"))
            .expect("storage read")
            .expect("stored row");
        assert_eq!(stored.data["title"], "private");
    }

    #[test]
    fn policy_read_eq_predicate_uses_declared_index() {
        let mut h = handler();
        let collection = CollectionId::new("policy_indexed_docs");
        let mut schema = policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                read: Some(owner_read_policy()),
                ..Default::default()
            },
        );
        schema.indexes = vec![IndexDef {
            field: "owner_id".into(),
            index_type: IndexType::String,
            unique: false,
        }];
        h.put_schema(schema).expect("policy schema");

        for (id, owner_id) in [("doc-1", "anonymous"), ("doc-2", "alice")] {
            h.create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new(id),
                data: json!({"owner_id": owner_id, "title": id}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .expect("seed row");
        }

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection,
                ..Default::default()
            })
            .expect("policy-filtered query should use index");

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("doc-1"));
        let plan = resp.policy_plan.expect("policy diagnostics");
        assert!(plan
            .storage_filters
            .contains(&"index:owner_id:subject:user_id".to_string()));
        assert!(!plan.post_filter);
    }

    #[test]
    fn policy_read_range_predicate_uses_declared_index() {
        let mut h = handler();
        let collection = CollectionId::new("policy_amount_docs");
        let mut schema = policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                read: Some(OperationPolicy {
                    allow: vec![PolicyRule {
                        name: Some("large-amounts".into()),
                        where_clause: Some(PolicyPredicate::Field {
                            field: "amount".into(),
                            op: PolicyCompareOp::Gte(json!(100)),
                        }),
                        ..Default::default()
                    }],
                    deny: vec![],
                }),
                ..Default::default()
            },
        );
        schema.indexes = vec![IndexDef {
            field: "amount".into(),
            index_type: IndexType::Integer,
            unique: false,
        }];
        h.put_schema(schema).expect("policy schema");

        for (id, amount) in [("small", 50), ("large", 150)] {
            h.create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new(id),
                data: json!({"amount": amount, "title": id}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .expect("seed amount row");
        }

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection,
                ..Default::default()
            })
            .expect("range policy query should use index");
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("large"));
        assert!(resp
            .policy_plan
            .expect("policy diagnostics")
            .storage_filters
            .contains(&"index:amount:gte".to_string()));
    }

    #[test]
    fn policy_read_array_membership_uses_eav_index() {
        let mut h = handler();
        let collection = CollectionId::new("policy_team_docs");
        let mut schema = policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                read: Some(OperationPolicy {
                    allow: vec![PolicyRule {
                        name: Some("team-members".into()),
                        where_clause: Some(PolicyPredicate::Field {
                            field: "team_ids[]".into(),
                            op: PolicyCompareOp::ContainsSubject("user_id".into()),
                        }),
                        ..Default::default()
                    }],
                    deny: vec![],
                }),
                ..Default::default()
            },
        );
        schema.entity_schema = Some(json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "team_ids": {"type": "array", "items": {"type": "string"}}
            }
        }));
        schema.indexes = vec![IndexDef {
            field: "team_ids[]".into(),
            index_type: IndexType::String,
            unique: false,
        }];
        h.put_schema(schema).expect("policy schema");

        for (id, teams) in [
            ("visible", json!(["anonymous", "ops"])),
            ("hidden", json!(["alice"])),
        ] {
            h.create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new(id),
                data: json!({"team_ids": teams, "title": id}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .expect("seed team row");
        }

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection,
                ..Default::default()
            })
            .expect("array policy query should use EAV index");
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("visible"));
        assert!(resp
            .policy_plan
            .expect("policy diagnostics")
            .storage_filters
            .contains(&"index:team_ids[]:subject:user_id".to_string()));
    }

    #[test]
    fn policy_read_unindexed_predicate_is_bounded_or_errors() {
        let mut small = handler();
        let collection = CollectionId::new("policy_small_unindexed");
        small
            .put_schema(policy_test_schema(
                collection.as_str(),
                AccessControlPolicy {
                    read: Some(owner_read_policy()),
                    ..Default::default()
                },
            ))
            .expect("small policy schema");
        for (id, owner_id) in [("doc-1", "anonymous"), ("doc-2", "alice")] {
            small
                .create_entity(CreateEntityRequest {
                    collection: collection.clone(),
                    id: EntityId::new(id),
                    data: json!({"owner_id": owner_id, "title": id}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("seed small row");
        }
        let small_resp = small
            .query_entities(QueryEntitiesRequest {
                collection: collection.clone(),
                ..Default::default()
            })
            .expect("small unindexed policy may post-filter");
        assert_eq!(small_resp.total_count, 1);
        let small_plan = small_resp.policy_plan.expect("policy diagnostics");
        assert_eq!(small_plan.missing_index.as_deref(), Some("owner_id"));
        assert!(small_plan.post_filter);

        let mut large = handler();
        let large_collection = CollectionId::new("policy_large_unindexed");
        large
            .put_schema(policy_test_schema(
                large_collection.as_str(),
                AccessControlPolicy {
                    read: Some(owner_read_policy()),
                    ..Default::default()
                },
            ))
            .expect("large policy schema");
        for i in 0..=POLICY_POST_FILTER_COST_LIMIT {
            large
                .create_entity(CreateEntityRequest {
                    collection: large_collection.clone(),
                    id: EntityId::new(format!("doc-{i:03}")),
                    data: json!({"owner_id": "alice", "title": format!("doc-{i:03}")}),
                    actor: None,
                    audit_metadata: None,
                    attribution: None,
                })
                .expect("seed large row");
        }
        let err = large
            .query_entities(QueryEntitiesRequest {
                collection: large_collection.clone(),
                ..Default::default()
            })
            .expect_err("large unindexed policy should be rejected");
        let text = err.to_string();
        assert!(text.contains("policy_filter_unindexed"), "{text}");
        assert!(text.contains("missing_index=owner_id"), "{text}");
        let AxonError::PolicyDenied(denial) = err else {
            panic!("expected structured policy denial");
        };
        assert_eq!(denial.reason, "policy_filter_unindexed");
        assert_eq!(denial.collection, large_collection.to_string());
        assert_eq!(denial.missing_index.as_deref(), Some("owner_id"));
        assert_eq!(denial.cost_limit, Some(POLICY_POST_FILTER_COST_LIMIT));
        assert_eq!(
            denial.candidate_count,
            Some(POLICY_POST_FILTER_COST_LIMIT + 1)
        );
    }

    #[test]
    fn policy_read_visibility_applies_to_point_reads_and_pagination() {
        let mut h = handler();
        let collection = CollectionId::new("policy_visibility_docs");
        h.put_schema(policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                read: Some(owner_read_policy()),
                ..Default::default()
            },
        ))
        .expect("policy schema");

        for (id, owner_id) in [
            ("a-hidden", "alice"),
            ("b-visible", "anonymous"),
            ("c-hidden", "alice"),
            ("d-visible", "anonymous"),
            ("e-visible", "anonymous"),
        ] {
            seed_policy_node(&mut h, &collection, id, owner_id, id);
        }

        let visible = h
            .get_entity(GetEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("b-visible"),
            })
            .expect("visible point read");
        assert_eq!(visible.entity.id, EntityId::new("b-visible"));

        let hidden = h
            .get_entity(GetEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("a-hidden"),
            })
            .expect_err("hidden point read should look absent");
        assert!(matches!(hidden, AxonError::NotFound(_)));

        let first_page = h
            .query_entities(QueryEntitiesRequest {
                collection: collection.clone(),
                limit: Some(2),
                ..Default::default()
            })
            .expect("first visible page");
        assert_eq!(first_page.total_count, 3);
        assert_eq!(
            first_page
                .entities
                .iter()
                .map(|entity| entity.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b-visible", "d-visible"]
        );
        assert_eq!(first_page.next_cursor.as_deref(), Some("d-visible"));

        let second_page = h
            .query_entities(QueryEntitiesRequest {
                collection,
                after_id: first_page.next_cursor.map(EntityId::new),
                limit: Some(2),
                ..Default::default()
            })
            .expect("second visible page");
        assert_eq!(second_page.total_count, 3);
        assert_eq!(second_page.entities.len(), 1);
        assert_eq!(second_page.entities[0].id, EntityId::new("e-visible"));
    }

    #[test]
    fn policy_read_visibility_omits_hidden_traversal_targets() {
        let mut h = handler();
        let collection = CollectionId::new("policy_visibility_graph");
        h.put_schema(policy_visibility_graph_schema(&collection))
            .expect("policy graph schema");
        seed_policy_node(&mut h, &collection, "source", "anonymous", "source");
        seed_policy_node(&mut h, &collection, "hidden", "alice", "hidden");
        seed_policy_node(&mut h, &collection, "visible", "anonymous", "visible");
        link_policy_nodes(&mut h, &collection, "source", "hidden");
        link_policy_nodes(&mut h, &collection, "source", "visible");

        let traverse = h
            .traverse(TraverseRequest {
                collection: collection.clone(),
                id: EntityId::new("source"),
                max_depth: Some(1),
                direction: TraverseDirection::Forward,
                link_type: Some("edge".into()),
                hop_filter: None,
            })
            .expect("policy-filtered traversal");
        assert_eq!(traverse.entities.len(), 1);
        assert_eq!(traverse.entities[0].id, EntityId::new("visible"));
        assert_eq!(traverse.links.len(), 1);

        let hidden_reachable = h
            .reachable(ReachableRequest {
                source_collection: collection.clone(),
                source_id: EntityId::new("source"),
                target_collection: collection.clone(),
                target_id: EntityId::new("hidden"),
                max_depth: Some(1),
                direction: TraverseDirection::Forward,
                link_type: Some("edge".into()),
            })
            .expect("hidden reachable check");
        assert!(!hidden_reachable.reachable);

        let visible_reachable = h
            .reachable(ReachableRequest {
                source_collection: collection.clone(),
                source_id: EntityId::new("source"),
                target_collection: collection.clone(),
                target_id: EntityId::new("visible"),
                max_depth: Some(1),
                direction: TraverseDirection::Forward,
                link_type: Some("edge".into()),
            })
            .expect("visible reachable check");
        assert!(visible_reachable.reachable);

        let neighbors = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection,
                id: EntityId::new("source"),
                link_type: Some("edge".into()),
                direction: Some(TraverseDirection::Forward),
            })
            .expect("visible neighbors");
        assert_eq!(neighbors.total_count, 1);
        assert_eq!(neighbors.groups[0].entities[0].id, EntityId::new("visible"));
    }

    #[test]
    fn policy_read_visibility_applies_before_link_candidate_limit_and_count() {
        let mut h = handler();
        let collection = CollectionId::new("policy_visibility_candidates");
        h.put_schema(policy_visibility_graph_schema(&collection))
            .expect("policy graph schema");
        seed_policy_node(&mut h, &collection, "source", "anonymous", "source");
        seed_policy_node(&mut h, &collection, "a-hidden", "alice", "candidate");
        seed_policy_node(&mut h, &collection, "b-visible", "anonymous", "candidate");
        link_policy_nodes(&mut h, &collection, "source", "a-hidden");
        link_policy_nodes(&mut h, &collection, "source", "b-visible");

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: collection,
                source_id: EntityId::new("source"),
                link_type: "edge".into(),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "title".into(),
                    op: FilterOp::Eq,
                    value: json!("candidate"),
                })),
                limit: Some(1),
            })
            .expect("visible link candidates");

        assert_eq!(resp.existing_link_count, 1);
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].entity.id, EntityId::new("b-visible"));
        assert!(resp.candidates[0].already_linked);
    }

    #[test]
    fn policy_read_related_target_policy_uses_link_plan() {
        let mut h = handler();
        let users = CollectionId::new("policy_users");
        let mut user_schema = policy_test_schema(
            users.as_str(),
            AccessControlPolicy {
                read: Some(OperationPolicy {
                    allow: vec![PolicyRule {
                        name: Some("visible-users".into()),
                        where_clause: Some(PolicyPredicate::Field {
                            field: "visible".into(),
                            op: PolicyCompareOp::Eq(json!(true)),
                        }),
                        ..Default::default()
                    }],
                    deny: vec![],
                }),
                ..Default::default()
            },
        );
        user_schema.entity_schema = Some(json!({
            "type": "object",
            "properties": {
                "owner_id": {"type": "string"},
                "title": {"type": "string"},
                "visible": {"type": "boolean"}
            }
        }));
        h.put_schema(user_schema).expect("user policy schema");

        let tasks = CollectionId::new("policy_tasks");
        let mut task_schema = policy_test_schema(
            tasks.as_str(),
            AccessControlPolicy {
                read: Some(OperationPolicy {
                    allow: vec![PolicyRule {
                        name: Some("assigned-visible-user".into()),
                        where_clause: Some(PolicyPredicate::Related {
                            related: axon_schema::RelationshipPredicate {
                                link_type: "assigned".into(),
                                direction: Some(LinkDirection::Outgoing),
                                target_collection: users.to_string(),
                                target_policy: Some(PolicyOperation::Read),
                            },
                        }),
                        ..Default::default()
                    }],
                    deny: vec![],
                }),
                ..Default::default()
            },
        );
        task_schema.link_types.insert(
            "assigned".into(),
            LinkTypeDef {
                target_collection: users.to_string(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );
        h.put_schema(task_schema).expect("task policy schema");

        for (id, visible) in [("u-visible", true), ("u-hidden", false)] {
            h.create_entity(CreateEntityRequest {
                collection: users.clone(),
                id: EntityId::new(id),
                data: json!({"owner_id": "anonymous", "title": id, "visible": visible}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .expect("seed user");
        }
        for id in ["t-visible", "t-hidden"] {
            h.create_entity(CreateEntityRequest {
                collection: tasks.clone(),
                id: EntityId::new(id),
                data: json!({"owner_id": "anonymous", "title": id}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .expect("seed task");
        }
        h.create_link(CreateLinkRequest {
            source_collection: tasks.clone(),
            source_id: EntityId::new("t-visible"),
            target_collection: users.clone(),
            target_id: EntityId::new("u-visible"),
            link_type: "assigned".into(),
            metadata: json!({}),
            actor: None,
            attribution: None,
        })
        .expect("visible assignment link");
        h.create_link(CreateLinkRequest {
            source_collection: tasks.clone(),
            source_id: EntityId::new("t-hidden"),
            target_collection: users,
            target_id: EntityId::new("u-hidden"),
            link_type: "assigned".into(),
            metadata: json!({}),
            actor: None,
            attribution: None,
        })
        .expect("hidden assignment link");

        let visible = h
            .get_entity(GetEntityRequest {
                collection: tasks.clone(),
                id: EntityId::new("t-visible"),
            })
            .expect("visible related point read");
        assert_eq!(visible.entity.id, EntityId::new("t-visible"));

        let hidden = h
            .get_entity(GetEntityRequest {
                collection: tasks.clone(),
                id: EntityId::new("t-hidden"),
            })
            .expect_err("hidden related point read should look absent");
        assert!(matches!(hidden, AxonError::NotFound(_)));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: tasks,
                ..Default::default()
            })
            .expect("relationship policy query");
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("t-visible"));
        assert!(resp
            .policy_plan
            .expect("policy diagnostics")
            .storage_filters
            .contains(&"link_index:outgoing:assigned".to_string()));
    }

    #[test]
    fn policy_read_redacts_entity_traverse_and_audit_payloads() {
        let mut h = handler();
        let collection = CollectionId::new("policy_redaction_docs");
        let mut schema = policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                read: Some(allow_all_policy()),
                fields: HashMap::from([("secret".into(), redacted_field_policy("secret"))]),
                ..Default::default()
            },
        );
        schema.entity_schema = Some(json!({
            "type": "object",
            "required": ["title", "secret"],
            "properties": {
                "title": {"type": "string"},
                "secret": {"type": "string"}
            }
        }));
        schema.link_types.insert(
            "edge".into(),
            LinkTypeDef {
                target_collection: collection.to_string(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );
        h.put_schema(schema).expect("redaction policy schema");

        for (id, title, secret) in [
            ("doc-1", "source", "classified"),
            ("doc-2", "target", "target-secret"),
        ] {
            h.create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new(id),
                data: json!({"title": title, "secret": secret}),
                actor: Some("admin".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect("seed redaction row");
        }
        h.update_entity(UpdateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new("doc-1"),
            data: json!({"title": "source-updated", "secret": "changed-secret"}),
            expected_version: 1,
            actor: Some("admin".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("update redaction row");
        h.create_link(CreateLinkRequest {
            source_collection: collection.clone(),
            source_id: EntityId::new("doc-1"),
            target_collection: collection.clone(),
            target_id: EntityId::new("doc-2"),
            link_type: "edge".into(),
            metadata: json!({}),
            actor: Some("admin".into()),
            attribution: None,
        })
        .expect("seed redaction link");

        let caller = CallerIdentity::new("blocked", Role::Read);
        let get = h
            .get_entity_with_caller(
                GetEntityRequest {
                    collection: collection.clone(),
                    id: EntityId::new("doc-1"),
                },
                &caller,
                None,
            )
            .expect("redacted point read");
        assert_eq!(get.entity.data["title"], "source-updated");
        assert_eq!(get.entity.data["secret"], Value::Null);
        let get_json = serde_json::to_string(&get.entity).expect("serialize get");
        assert!(!get_json.contains("changed-secret"));

        let list = h
            .query_entities_with_caller(
                QueryEntitiesRequest {
                    collection: collection.clone(),
                    ..Default::default()
                },
                &caller,
                None,
            )
            .expect("redacted list read");
        assert_eq!(list.total_count, 2);
        assert!(list
            .entities
            .iter()
            .all(|entity| entity.data["secret"] == Value::Null));
        let list_json = serde_json::to_string(&list.entities).expect("serialize list");
        assert!(!list_json.contains("target-secret"));

        let traversal = h
            .traverse_with_caller(
                TraverseRequest {
                    collection: collection.clone(),
                    id: EntityId::new("doc-1"),
                    link_type: Some("edge".into()),
                    max_depth: Some(1),
                    direction: TraverseDirection::Forward,
                    hop_filter: None,
                },
                &caller,
                None,
            )
            .expect("redacted traversal");
        assert_eq!(traversal.entities.len(), 1);
        assert_eq!(traversal.entities[0].data["secret"], Value::Null);
        assert_eq!(
            traversal.paths[0].hops[0].entity.data["secret"],
            Value::Null
        );
        let traversal_json =
            serde_json::to_string(&traversal.entities).expect("serialize traversal");
        assert!(!traversal_json.contains("target-secret"));

        let audit = h
            .query_audit_with_caller(
                QueryAuditRequest {
                    collection: Some(collection),
                    entity_id: Some(EntityId::new("doc-1")),
                    ..Default::default()
                },
                &caller,
                None,
            )
            .expect("redacted audit read");
        let update_entry = audit
            .entries
            .iter()
            .find(|entry| entry.mutation == MutationType::EntityUpdate)
            .expect("update audit entry");
        assert_eq!(
            update_entry.data_before.as_ref().expect("before")["secret"],
            Value::Null
        );
        assert_eq!(
            update_entry.data_after.as_ref().expect("after")["secret"],
            Value::Null
        );
        let diff = update_entry.diff.as_ref().expect("redacted diff");
        assert_eq!(diff["secret"].before, Some(Value::Null));
        assert_eq!(diff["secret"].after, Some(Value::Null));
        let audit_json = serde_json::to_string(&audit.entries).expect("serialize audit");
        assert!(!audit_json.contains("classified"));
        assert!(!audit_json.contains("changed-secret"));
    }

    #[test]
    fn scn_017_intent_audit_reads_redact_commercial_fields_for_contractors() {
        let mut h = handler();
        let fixture = seed_procurement_fixture(&mut h).expect("SCN-017 fixture should seed");
        let invoices = fixture.collections.invoices.clone();
        let invoice_id = fixture.ids.under_threshold_invoice.clone();
        let operator = CallerIdentity::new(fixture.subjects.operator, Role::Read);
        let contractor = CallerIdentity::new(fixture.subjects.contractor, Role::Read);

        let operator_invoice = h
            .get_entity_with_caller(
                GetEntityRequest {
                    collection: invoices.clone(),
                    id: invoice_id.clone(),
                },
                &operator,
                None,
            )
            .expect("operator should read invoice");
        assert_eq!(operator_invoice.entity.data["amount_cents"], 750_000);
        assert_eq!(
            operator_invoice.entity.data["commercial_terms"],
            "net-30 standard procurement terms"
        );

        let contractor_invoice = h
            .get_entity_with_caller(
                GetEntityRequest {
                    collection: invoices.clone(),
                    id: invoice_id.clone(),
                },
                &contractor,
                None,
            )
            .expect("contractor should read assigned invoice");
        assert_eq!(contractor_invoice.entity.data["amount_cents"], Value::Null);
        assert_eq!(
            contractor_invoice.entity.data["commercial_terms"],
            Value::Null
        );

        let before = h
            .storage_ref()
            .get(&invoices, &invoice_id)
            .expect("read invoice before")
            .expect("invoice exists");
        let mut after_data = before.data.clone();
        after_data["amount_cents"] = json!(1_200_000);
        after_data["commercial_terms"] = json!("net-7 confidential override terms");
        let mut after = before.clone();
        after.data = after_data.clone();

        let mut tx = crate::Transaction::new();
        tx.update(after, before.version, Some(before.data.clone()))
            .expect("stage invoice update");
        let operation = crate::canonical_staged_transaction_operation(&tx);
        let diff = serde_json::to_value(compute_diff(&before.data, &after_data))
            .expect("diff should serialize");
        let scope = crate::MutationIntentScopeBinding {
            tenant_id: "default".into(),
            database_id: "default".into(),
        };
        let subject = crate::MutationIntentSubjectBinding {
            user_id: Some(fixture.subjects.finance_agent.into()),
            agent_id: Some("finance-agent-tool".into()),
            tenant_role: Some("finance_agent".into()),
            credential_id: Some("cred-finance-agent".into()),
            grant_version: Some(1),
            ..Default::default()
        };
        let pre_images = vec![crate::PreImageBinding::Entity {
            collection: invoices.clone(),
            id: invoice_id.clone(),
            version: before.version,
        }];
        let intent_id = "mint_scn_017_invoice_commercial_redaction";
        let intent = crate::MutationIntent {
            intent_id: intent_id.into(),
            scope: scope.clone(),
            subject: subject.clone(),
            schema_version: 1,
            policy_version: 1,
            operation: operation.clone(),
            pre_images: pre_images.clone(),
            decision: crate::MutationIntentDecision::NeedsApproval,
            approval_state: crate::ApprovalState::Pending,
            approval_route: Some(crate::MutationApprovalRoute {
                role: Some("finance_approver".into()),
                reason_required: true,
                deadline_seconds: Some(3600),
                separation_of_duties: true,
            }),
            expires_at: 9_000_000_000_000_000_000,
            review_summary: crate::MutationReviewSummary {
                title: Some("invoice commercial update".into()),
                summary: "needs_approval".into(),
                risk: Some("needs_approval".into()),
                affected_records: pre_images,
                affected_fields: vec!["amount_cents".into(), "commercial_terms".into()],
                diff: json!([{ "operationIndex": 0, "diff": diff }]),
                policy_explanation: vec!["require-approval-large-invoice-update".into()],
            },
        };
        let svc = crate::MutationIntentLifecycleService::new(
            crate::MutationIntentTokenSigner::new(b"redaction-test-secret"),
        );
        let token = {
            let (storage, audit) = h.storage_and_audit_mut();
            svc.create_preview_record(storage, audit, intent)
        }
        .expect("preview record should persist")
        .intent_token
        .expect("needs-approval intent should issue a token");
        {
            let (storage, audit) = h.storage_and_audit_mut();
            svc.approve_with_audit(
                storage,
                audit,
                &scope,
                intent_id,
                crate::MutationIntentReviewMetadata {
                    actor: Some(fixture.subjects.finance_approver.into()),
                    reason: Some("approved commercial update".into()),
                },
                1,
            )
            .expect("intent should approve");
        }
        {
            let (storage, audit) = h.storage_and_audit_mut();
            svc.commit_transaction_intent(
                storage,
                audit,
                crate::MutationIntentTransactionCommitRequest {
                    scope: scope.clone(),
                    token,
                    transaction: tx,
                    canonical_operation: Some(operation.clone()),
                    current: crate::MutationIntentCommitValidationContext {
                        subject,
                        schema_version: 1,
                        policy_version: 1,
                        operation_hash: operation.operation_hash.clone(),
                        caller_authorized: true,
                    },
                    now_ns: 2,
                    actor: Some(fixture.subjects.finance_agent.into()),
                    attribution: None,
                },
            )
            .expect("intent transaction should commit");
        }

        let operator_detail = h
            .storage_ref()
            .get_mutation_intent(&scope.tenant_id, &scope.database_id, intent_id)
            .expect("intent lookup should succeed")
            .expect("intent should exist");
        let mut contractor_detail = operator_detail.clone();
        h.redact_mutation_intent_for_read(&mut contractor_detail, &contractor, None)
            .expect("intent detail redaction should succeed");
        assert_eq!(
            operator_detail.review_summary.diff[0]["diff"]["amount_cents"]["before"],
            750_000
        );
        assert_eq!(
            operator_detail.review_summary.diff[0]["diff"]["commercial_terms"]["after"],
            "net-7 confidential override terms"
        );
        assert_eq!(
            contractor_detail.review_summary.diff[0]["diff"]["amount_cents"]["before"],
            Value::Null
        );
        assert_eq!(
            contractor_detail.review_summary.diff[0]["diff"]["commercial_terms"]["after"],
            Value::Null
        );
        let contractor_operation = contractor_detail
            .operation
            .canonical_operation
            .as_ref()
            .unwrap();
        assert_eq!(
            contractor_operation["operations"][0]["data"]["amount_cents"],
            Value::Null
        );
        assert_eq!(
            contractor_operation["operations"][0]["data"]["commercial_terms"],
            Value::Null
        );

        let operator_audit = h
            .query_audit_with_caller(
                QueryAuditRequest {
                    intent_id: Some(intent_id.into()),
                    ..Default::default()
                },
                &operator,
                None,
            )
            .expect("operator lineage audit should query");
        let operator_update = operator_audit
            .entries
            .iter()
            .find(|entry| entry.mutation == MutationType::EntityUpdate)
            .expect("operator should see committed update audit");
        assert_eq!(
            operator_update.data_before.as_ref().unwrap()["commercial_terms"],
            "net-30 standard procurement terms"
        );
        assert_eq!(
            operator_update.diff.as_ref().unwrap()["amount_cents"].after,
            Some(json!(1_200_000))
        );

        let contractor_audit = h
            .query_audit_with_caller(
                QueryAuditRequest {
                    intent_id: Some(intent_id.into()),
                    ..Default::default()
                },
                &contractor,
                None,
            )
            .expect("contractor lineage audit should query");
        let contractor_update = contractor_audit
            .entries
            .iter()
            .find(|entry| entry.mutation == MutationType::EntityUpdate)
            .expect("contractor should see committed update audit");
        assert_eq!(
            contractor_update.data_before.as_ref().unwrap()["amount_cents"],
            Value::Null
        );
        assert_eq!(
            contractor_update.data_after.as_ref().unwrap()["commercial_terms"],
            Value::Null
        );
        assert_eq!(
            contractor_update.diff.as_ref().unwrap()["amount_cents"].after,
            Some(Value::Null)
        );

        let contractor_approval = contractor_audit
            .entries
            .iter()
            .find(|entry| entry.mutation == MutationType::IntentApprove)
            .expect("contractor lineage should include approval audit");
        let approval_pre_image = contractor_approval.data_before.as_ref().unwrap();
        assert_eq!(
            approval_pre_image["review_summary"]["diff"][0]["diff"]["amount_cents"]["after"],
            Value::Null
        );
        assert_eq!(
            approval_pre_image["operation"]["canonical_operation"]["operations"][0]["data"]
                ["commercial_terms"],
            Value::Null
        );
        let approval_payload = contractor_approval.data_after.as_ref().unwrap();
        assert_eq!(
            approval_payload["review_summary"]["diff"][0]["diff"]["amount_cents"]["after"],
            Value::Null
        );
        assert_eq!(
            approval_payload["review_summary"]["diff"][0]["diff"]["commercial_terms"]["before"],
            Value::Null
        );
        assert_eq!(
            approval_payload["operation"]["canonical_operation"]["operations"][0]["data"]
                ["amount_cents"],
            Value::Null
        );
        let contractor_audit_json =
            serde_json::to_string(&contractor_audit.entries).expect("serialize audit");
        assert!(!contractor_audit_json.contains("net-30 standard procurement terms"));
        assert!(!contractor_audit_json.contains("net-7 confidential override terms"));
    }

    /// Asserts that traverse() drops neighbor edges whose target entity is
    /// hidden by row-level read policy. Bead axon-27a73744: handler.rs:6792-6800
    /// short-circuits to `continue` when get_visible_entity_for_read_with_context
    /// returns None for the neighbor; removing that short-circuit must make
    /// this test fail.
    #[test]
    fn scn_017_traverse_drops_contractor_hidden_neighbors() {
        let mut h = handler();
        let fixture = seed_procurement_fixture(&mut h).expect("SCN-017 fixture should seed");
        let invoices = fixture.collections.invoices.clone();

        // The procurement fixture's invoice schema does not declare a
        // self-referential link type. Extend it with `related-invoice` so we
        // can model a forward neighbor in the same collection without
        // changing the access_control policy under test.
        let mut invoice_schema = fixture
            .schemas
            .iter()
            .find(|schema| schema.collection == invoices)
            .cloned()
            .expect("invoice schema should be in fixture.schemas");
        invoice_schema.version += 1;
        invoice_schema.link_types.insert(
            "related-invoice".into(),
            LinkTypeDef {
                target_collection: invoices.as_str().into(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );
        h.put_schema(invoice_schema)
            .expect("extend invoice schema with related-invoice link type");

        // Seed an additional invoice assigned to a non-contractor user. Under
        // the SCN-017 procurement policy, contractor reads require
        // assigned_contractor_id == subject.user_id, so a contractor caller
        // cannot read this row. Cross-link it from the contractor-visible
        // small invoice to model a hidden-target neighbor.
        let hidden_invoice_id = EntityId::new("inv-hidden-from-contractor");
        h.create_entity(CreateEntityRequest {
            collection: invoices.clone(),
            id: hidden_invoice_id.clone(),
            data: json!({
                "number": "INV-9001",
                "vendor_id": fixture.ids.primary_vendor.as_str(),
                "requester_id": fixture.ids.requester.as_str(),
                "assigned_contractor_id": fixture.ids.finance_agent.as_str(),
                "purchase_order_id": fixture.ids.under_threshold_purchase_order.as_str(),
                "status": "submitted",
                "amount_cents": 250_000,
                "currency": "USD",
                "commercial_terms": "net-15 hidden-from-contractor terms",
                "received_at": "2026-04-03T10:00:00Z",
                "metadata": { "source": "test" }
            }),
            actor: Some("admin".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed contractor-hidden invoice");

        h.create_link(CreateLinkRequest {
            source_collection: invoices.clone(),
            source_id: fixture.ids.under_threshold_invoice.clone(),
            target_collection: invoices.clone(),
            target_id: hidden_invoice_id.clone(),
            link_type: "related-invoice".into(),
            metadata: json!({}),
            actor: Some("admin".into()),
            attribution: None,
        })
        .expect("link visible invoice to hidden one");

        // Sanity: the operator (who can read all invoices) sees the hidden
        // target via traversal. This proves the link exists; the contractor
        // assertion below would otherwise pass vacuously if the link were
        // absent.
        let operator = CallerIdentity::new(fixture.subjects.operator, Role::Read);
        let operator_traversal = h
            .traverse_with_caller(
                TraverseRequest {
                    collection: invoices.clone(),
                    id: fixture.ids.under_threshold_invoice.clone(),
                    link_type: Some("related-invoice".into()),
                    max_depth: Some(1),
                    direction: TraverseDirection::Forward,
                    hop_filter: None,
                },
                &operator,
                None,
            )
            .expect("operator traversal");
        assert!(operator_traversal
            .entities
            .iter()
            .any(|entity| entity.id == hidden_invoice_id));

        // Contractor traversal: the hidden neighbor must be filtered out by
        // the row-level read policy. The visibility short-circuit at
        // handler.rs:6792-6800 collapses both the entities and paths sets,
        // so neither should expose the hidden entity id.
        let contractor = CallerIdentity::new(fixture.subjects.contractor, Role::Read);
        let contractor_traversal = h
            .traverse_with_caller(
                TraverseRequest {
                    collection: invoices.clone(),
                    id: fixture.ids.under_threshold_invoice.clone(),
                    link_type: Some("related-invoice".into()),
                    max_depth: Some(1),
                    direction: TraverseDirection::Forward,
                    hop_filter: None,
                },
                &contractor,
                None,
            )
            .expect("contractor traversal");
        assert!(
            !contractor_traversal
                .entities
                .iter()
                .any(|entity| entity.id == hidden_invoice_id),
            "contractor traversal entities must drop hidden invoice"
        );
        assert!(
            !contractor_traversal.paths.iter().any(|path| path
                .hops
                .iter()
                .any(|hop| hop.entity.id == hidden_invoice_id)),
            "contractor traversal paths must drop hidden invoice hops"
        );
        let contractor_traversal_json =
            serde_json::to_string(&contractor_traversal).expect("serialize traversal");
        assert!(
            !contractor_traversal_json.contains(hidden_invoice_id.as_str()),
            "contractor traversal payload must not surface hidden invoice id"
        );
        assert!(
            !contractor_traversal_json.contains("net-15 hidden-from-contractor terms"),
            "contractor traversal payload must not surface hidden commercial_terms"
        );
    }

    #[test]
    fn policy_denies_field_create_update_patch_and_delete_without_audit() {
        let mut h = handler();
        let collection = CollectionId::new("policy_secrets");
        h.put_schema(policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                create: Some(allow_all_policy()),
                read: Some(allow_all_policy()),
                update: Some(allow_all_policy()),
                delete: Some(allow_all_policy()),
                fields: HashMap::from([("secret".into(), denied_field_policy("secret"))]),
                ..Default::default()
            },
        ))
        .expect("policy schema");

        let create_err = h
            .create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-denied-create"),
                data: json!({"title": "new", "secret": "classified"}),
                actor: Some("blocked".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("denied create should fail");
        assert_policy_forbidden(create_err, "field_write_denied", Some("secret"));
        assert!(h
            .storage_ref()
            .get(&collection, &EntityId::new("doc-denied-create"))
            .expect("storage read")
            .is_none());

        h.create_entity(CreateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new("doc-1"),
            data: json!({"title": "seed", "secret": "classified"}),
            actor: Some("admin".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed secret row");
        let audit_before = h.audit_log().entries().len();

        let update_err = h
            .update_entity(UpdateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-1"),
                data: json!({"title": "updated", "secret": "changed"}),
                expected_version: 1,
                actor: Some("blocked".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("denied update should fail");
        assert_policy_forbidden(update_err, "field_write_denied", Some("secret"));

        let patch_err = h
            .patch_entity(PatchEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-1"),
                patch: json!({"secret": null}),
                expected_version: 1,
                actor: Some("blocked".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("denied patch delete should fail");
        assert_policy_forbidden(patch_err, "field_write_denied", Some("secret"));

        let delete_err = h
            .delete_entity(DeleteEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-1"),
                actor: Some("blocked".into()),
                force: false,
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("denied delete should fail");
        assert_policy_forbidden(delete_err, "field_write_denied", Some("secret"));
        assert_eq!(h.audit_log().entries().len(), audit_before);
        let stored = h
            .storage_ref()
            .get(&collection, &EntityId::new("doc-1"))
            .expect("storage read")
            .expect("row should remain");
        assert_eq!(stored.data["secret"], "classified");
        assert_eq!(stored.version, 1);
    }

    #[test]
    fn policy_denies_lifecycle_field_mutation() {
        let mut h = handler();
        let collection = CollectionId::new("policy_lifecycle");
        let mut schema = policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                create: Some(allow_all_policy()),
                read: Some(allow_all_policy()),
                update: Some(allow_all_policy()),
                fields: HashMap::from([("status".into(), denied_field_policy("status"))]),
                ..Default::default()
            },
        );
        schema.lifecycles.insert(
            "status_flow".into(),
            axon_schema::schema::LifecycleDef {
                field: "status".into(),
                initial: "draft".into(),
                transitions: HashMap::from([("draft".into(), vec!["submitted".into()])]),
            },
        );
        h.put_schema(schema).expect("policy lifecycle schema");
        h.create_entity(CreateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new("doc-1"),
            data: json!({"title": "seed"}),
            actor: Some("admin".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed lifecycle row");
        let audit_before = h.audit_log().entries().len();

        let err = h
            .transition_lifecycle(TransitionLifecycleRequest {
                collection_id: collection,
                entity_id: EntityId::new("doc-1"),
                lifecycle_name: "status_flow".into(),
                target_state: "submitted".into(),
                expected_version: 1,
                actor: Some("blocked".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("denied lifecycle transition should fail");

        assert_policy_forbidden(err, "field_write_denied", Some("status"));
        assert_eq!(h.audit_log().entries().len(), audit_before);
    }

    #[test]
    fn policy_denies_rollback_that_writes_denied_field() {
        let mut h = handler();
        let collection = CollectionId::new("policy_rollback");
        h.put_schema(policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                create: Some(allow_all_policy()),
                read: Some(allow_all_policy()),
                update: Some(allow_all_policy()),
                fields: HashMap::from([("secret".into(), denied_field_policy("secret"))]),
                ..Default::default()
            },
        ))
        .expect("policy schema");
        h.create_entity(CreateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new("doc-1"),
            data: json!({"title": "seed", "secret": "v1"}),
            actor: Some("admin".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("seed rollback row");
        h.update_entity(UpdateEntityRequest {
            collection: collection.clone(),
            id: EntityId::new("doc-1"),
            data: json!({"title": "seed", "secret": "v2"}),
            expected_version: 1,
            actor: Some("admin".into()),
            audit_metadata: None,
            attribution: None,
        })
        .expect("update rollback row");
        let audit_before = h.audit_log().entries().len();

        let err = h
            .rollback_entity(RollbackEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-1"),
                target: RollbackEntityTarget::Version(1),
                expected_version: Some(2),
                actor: Some("blocked".into()),
                dry_run: false,
            })
            .expect_err("denied rollback should fail");

        assert_policy_forbidden(err, "field_write_denied", Some("secret"));
        assert_eq!(h.audit_log().entries().len(), audit_before);
        let stored = h
            .get_entity(GetEntityRequest {
                collection,
                id: EntityId::new("doc-1"),
            })
            .expect("stored row")
            .entity;
        assert_eq!(stored.data["secret"], "v2");
    }

    #[test]
    fn policy_refuses_approval_routed_direct_commit() {
        let mut h = handler();
        let collection = CollectionId::new("policy_approval");
        h.put_schema(policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                create: Some(allow_all_policy()),
                read: Some(allow_all_policy()),
                envelopes: HashMap::from([(
                    PolicyOperation::Write,
                    vec![PolicyEnvelope {
                        name: Some("large-write-needs-approval".into()),
                        when: Some(PolicyPredicate::Field {
                            field: "amount".into(),
                            op: PolicyCompareOp::Gt(json!(100)),
                        }),
                        decision: PolicyDecision::NeedsApproval,
                        approval: None,
                    }],
                )]),
                ..Default::default()
            },
        ))
        .expect("approval schema");

        let err = h
            .create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: EntityId::new("doc-1"),
                data: json!({"title": "large", "amount": 1000}),
                actor: Some("alice".into()),
                audit_metadata: None,
                attribution: None,
            })
            .expect_err("approval-routed write should not commit directly");

        assert_policy_forbidden(err, "needs_approval", None);
        assert!(h
            .storage_ref()
            .get(&collection, &EntityId::new("doc-1"))
            .expect("storage read")
            .is_none());
    }

    #[test]
    fn policy_denied_transaction_aborts_by_operation_index_without_audit() {
        let mut h = handler();
        let collection = CollectionId::new("policy_tx");
        h.put_schema(policy_test_schema(
            collection.as_str(),
            AccessControlPolicy {
                create: Some(allow_all_policy()),
                read: Some(allow_all_policy()),
                fields: HashMap::from([("secret".into(), denied_field_policy("secret"))]),
                ..Default::default()
            },
        ))
        .expect("transaction policy schema");
        let audit_before = h.audit_log().entries().len();

        let mut tx = crate::transaction::Transaction::new();
        tx.create(Entity::new(
            collection.clone(),
            EntityId::new("allowed"),
            json!({"title": "allowed"}),
        ))
        .expect("stage allowed create");
        tx.create(Entity::new(
            collection.clone(),
            EntityId::new("denied"),
            json!({"title": "denied", "secret": "classified"}),
        ))
        .expect("stage denied create");

        let err = h
            .commit_transaction(tx, Some("blocked".into()), None)
            .expect_err("transaction should fail before commit");

        let err_text = err.to_string();
        assert!(
            err_text.contains("operation_index=1"),
            "missing denied operation index: {err_text}"
        );
        assert_policy_forbidden(err, "field_write_denied", Some("secret"));
        assert!(ok_or_panic(
            h.storage_ref().get(&collection, &EntityId::new("allowed")),
            "reading allowed entity after denied transaction"
        )
        .is_none());
        assert!(ok_or_panic(
            h.storage_ref().get(&collection, &EntityId::new("denied")),
            "reading denied entity after denied transaction"
        )
        .is_none());
        assert_eq!(h.audit_log().entries().len(), audit_before);
    }

    fn cache_len(handler: &AxonHandler<MemoryStorageAdapter>) -> usize {
        ok_or_panic(
            handler.markdown_template_cache(),
            "reading markdown template cache size",
        )
        .entries
        .len()
    }

    fn is_template_cached(
        handler: &AxonHandler<MemoryStorageAdapter>,
        collection: &CollectionId,
    ) -> bool {
        ok_or_panic(
            handler.markdown_template_cache(),
            "reading markdown template cache entry",
        )
        .entries
        .contains_key(collection)
    }

    fn seed_markdown_collection(
        handler: &mut AxonHandler<MemoryStorageAdapter>,
        name: &str,
        title: &str,
    ) -> (CollectionId, EntityId) {
        let collection = CollectionId::new(name);
        let entity_id = EntityId::new("t-001");

        ok_or_panic(
            handler.create_collection(CreateCollectionRequest {
                name: collection.clone(),
                schema: CollectionSchema::new(collection.clone()),
                actor: None,
            }),
            "creating collection for markdown cache test",
        );
        ok_or_panic(
            handler.create_entity(CreateEntityRequest {
                collection: collection.clone(),
                id: entity_id.clone(),
                data: json!({"title": title}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity for markdown cache test",
        );
        ok_or_panic(
            handler
                .storage_mut()
                .put_collection_view(&CollectionView::new(collection.clone(), "# {{title}}")),
            "storing collection view for markdown cache test",
        );

        (collection, entity_id)
    }

    // ── Entity CRUD ──────────────────────────────────────────────────────────

    #[test]
    fn create_then_get_roundtrip() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        let created = h
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        assert_eq!(created.entity.version, 1);

        let fetched = h
            .get_entity(GetEntityRequest {
                collection: col,
                id,
            })
            .unwrap();
        assert_eq!(fetched.entity.data["title"], "hello");
    }

    #[test]
    fn get_missing_entity_returns_not_found() {
        let h = handler();
        let result = h.get_entity(GetEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("missing"),
        });
        assert!(matches!(result, Err(AxonError::NotFound(_))));
    }

    #[test]
    fn get_entity_markdown_renders_collection_view() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for markdown render test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello", "status": "open"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity for markdown render test",
        );
        ok_or_panic(
            h.storage_mut().put_collection_view(&CollectionView::new(
                col.clone(),
                "# {{title}}\n\nStatus: {{status}}",
            )),
            "storing collection view for markdown render test",
        );

        let rendered = ok_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown with collection view",
        );

        match rendered {
            GetEntityMarkdownResponse::Rendered {
                rendered_markdown, ..
            } => {
                assert_eq!(rendered_markdown, "# hello\n\nStatus: open");
            }
            GetEntityMarkdownResponse::RenderFailed { .. } => {
                panic!("expected markdown render to succeed")
            }
        }
    }

    #[test]
    fn get_entity_markdown_refreshes_compiled_template_after_view_update() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for template cache test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello", "status": "open"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity for template cache test",
        );
        ok_or_panic(
            h.storage_mut()
                .put_collection_view(&CollectionView::new(col.clone(), "# {{title}}")),
            "storing initial collection view for template cache test",
        );

        match ok_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown with initial collection view",
        ) {
            GetEntityMarkdownResponse::Rendered {
                rendered_markdown, ..
            } => assert_eq!(rendered_markdown, "# hello"),
            GetEntityMarkdownResponse::RenderFailed { .. } => {
                panic!("expected initial markdown render to succeed")
            }
        }

        let updated = ok_or_panic(
            h.storage_mut()
                .put_collection_view(&CollectionView::new(col.clone(), "Status: {{status}}")),
            "updating collection view for template cache test",
        );
        assert_eq!(updated.version, 2);

        match ok_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown after collection view update",
        ) {
            GetEntityMarkdownResponse::Rendered {
                rendered_markdown, ..
            } => assert_eq!(rendered_markdown, "Status: open"),
            GetEntityMarkdownResponse::RenderFailed { .. } => {
                panic!("expected markdown render to refresh after template update")
            }
        }
    }

    #[test]
    fn get_entity_markdown_bounds_template_cache_and_recompiles_evicted_entries() {
        let mut h = handler_with_markdown_template_cache_capacity(2);
        let (first_collection, first_id) = seed_markdown_collection(&mut h, "tasks-a", "alpha");
        let (second_collection, second_id) = seed_markdown_collection(&mut h, "tasks-b", "beta");
        let (third_collection, third_id) = seed_markdown_collection(&mut h, "tasks-c", "gamma");

        assert_rendered_markdown(
            ok_or_panic(
                h.get_entity_markdown(&first_collection, &first_id),
                "rendering markdown for first collection",
            ),
            "# alpha",
        );
        assert!(is_template_cached(&h, &first_collection));

        assert_rendered_markdown(
            ok_or_panic(
                h.get_entity_markdown(&second_collection, &second_id),
                "rendering markdown for second collection",
            ),
            "# beta",
        );
        assert_eq!(cache_len(&h), 2);

        assert_rendered_markdown(
            ok_or_panic(
                h.get_entity_markdown(&third_collection, &third_id),
                "rendering markdown for third collection",
            ),
            "# gamma",
        );
        assert_eq!(cache_len(&h), 2);
        assert!(!is_template_cached(&h, &first_collection));
        assert!(is_template_cached(&h, &second_collection));
        assert!(is_template_cached(&h, &third_collection));

        assert_rendered_markdown(
            ok_or_panic(
                h.get_entity_markdown(&first_collection, &first_id),
                "rendering markdown for evicted first collection",
            ),
            "# alpha",
        );
        assert_eq!(cache_len(&h), 2);
        assert!(is_template_cached(&h, &first_collection));
        assert!(!is_template_cached(&h, &second_collection));
        assert!(is_template_cached(&h, &third_collection));
    }

    #[test]
    fn get_entity_markdown_requires_collection_view() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for missing template test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity for missing template test",
        );

        let error = err_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown without collection view",
        );

        assert!(matches!(error, AxonError::InvalidArgument(_)));
        assert!(error
            .to_string()
            .contains("has no markdown template defined"));
    }

    #[test]
    fn put_collection_template_round_trips_and_reports_optional_field_warnings() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema {
                    collection: col.clone(),
                    description: None,
                    version: 1,
                    entity_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "notes": {"type": "string"}
                        },
                        "required": ["title"]
                    })),
                    link_types: Default::default(),
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
                },
                actor: None,
            }),
            "creating collection for template round-trip test",
        );

        let stored = ok_or_panic(
            h.put_collection_template(PutCollectionTemplateRequest {
                collection: col.clone(),
                template: "# {{title}}\n\n{{notes}}".into(),
                actor: Some("operator".into()),
            }),
            "storing collection template through handler",
        );
        assert_eq!(stored.view.markdown_template, "# {{title}}\n\n{{notes}}");
        assert_eq!(stored.view.version, 1);
        assert_eq!(stored.view.updated_by.as_deref(), Some("operator"));
        assert_eq!(stored.warnings.len(), 1);
        assert!(stored.warnings[0].contains("field 'notes' is optional"));

        let retrieved = ok_or_panic(
            h.get_collection_template(GetCollectionTemplateRequest {
                collection: col.clone(),
            }),
            "retrieving collection template through handler",
        );
        assert_eq!(retrieved.view, stored.view);
    }

    #[test]
    fn put_collection_template_rejects_unknown_schema_fields() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema {
                    collection: col.clone(),
                    description: None,
                    version: 1,
                    entity_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"}
                        },
                        "required": ["title"]
                    })),
                    link_types: Default::default(),
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
                },
                actor: None,
            }),
            "creating collection for invalid template test",
        );

        let error = err_or_panic(
            h.put_collection_template(PutCollectionTemplateRequest {
                collection: col,
                template: "{{ghost}}".into(),
                actor: None,
            }),
            "rejecting template with unknown schema fields",
        );
        assert!(matches!(error, AxonError::SchemaValidation(_)));
        assert!(error
            .to_string()
            .contains("template references field 'ghost'"));
    }

    #[test]
    fn delete_collection_template_clears_template_and_cache() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for template delete test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity for template delete test",
        );
        ok_or_panic(
            h.put_collection_template(PutCollectionTemplateRequest {
                collection: col.clone(),
                template: "# {{title}}".into(),
                actor: None,
            }),
            "storing template for delete test",
        );

        assert_rendered_markdown(
            ok_or_panic(
                h.get_entity_markdown(&col, &id),
                "rendering markdown before template delete",
            ),
            "# hello",
        );
        assert!(is_template_cached(&h, &col));

        let deleted = ok_or_panic(
            h.delete_collection_template(DeleteCollectionTemplateRequest {
                collection: col.clone(),
                actor: None,
            }),
            "deleting collection template through handler",
        );
        assert_eq!(deleted.collection, col.to_string());
        assert!(!is_template_cached(&h, &col));

        let error = err_or_panic(
            h.get_entity_markdown(&col, &id),
            "rejecting markdown render after template delete",
        );
        assert!(matches!(error, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn collection_template_crud_produces_audited_create_update_delete_entries() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema {
                    collection: col.clone(),
                    description: None,
                    version: 1,
                    entity_schema: Some(json!({
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "notes": {"type": "string"},
                            "status": {"type": "string"}
                        },
                        "required": ["title"]
                    })),
                    link_types: Default::default(),
                    access_control: None,
                    gates: Default::default(),
                    validation_rules: Default::default(),
                    indexes: Default::default(),
                    compound_indexes: Default::default(),
                    queries: Default::default(),
                    lifecycles: Default::default(),
                },
                actor: None,
            }),
            "creating collection for template audit test",
        );

        let created = ok_or_panic(
            h.put_collection_template(PutCollectionTemplateRequest {
                collection: col.clone(),
                template: "# {{title}}".into(),
                actor: Some("creator".into()),
            }),
            "creating template for audit test",
        );
        let updated = ok_or_panic(
            h.put_collection_template(PutCollectionTemplateRequest {
                collection: col.clone(),
                template: "## {{title}}\n\nStatus: {{status}}".into(),
                actor: Some("editor".into()),
            }),
            "updating template for audit test",
        );
        ok_or_panic(
            h.delete_collection_template(DeleteCollectionTemplateRequest {
                collection: col.clone(),
                actor: Some("cleaner".into()),
            }),
            "deleting template for audit test",
        );

        let created_entries = ok_or_panic(
            h.query_audit(QueryAuditRequest {
                collection: Some(col.clone()),
                operation: Some("template.create".into()),
                ..Default::default()
            }),
            "querying template create audit entries",
        );
        assert_eq!(created_entries.entries.len(), 1);
        let created_entry = &created_entries.entries[0];
        assert_eq!(created_entry.actor, "creator");
        assert!(created_entry.timestamp_ns > 0);
        assert_eq!(created_entry.mutation, MutationType::TemplateCreate);
        assert_eq!(created_entry.version, 1);
        assert!(created_entry.data_before.is_none());
        assert_eq!(
            created_entry.data_after,
            Some(json!({
                "collection": "tasks",
                "markdown_template": "# {{title}}",
                "version": 1,
                "updated_at_ns": created.view.updated_at_ns,
                "updated_by": "creator",
            }))
        );

        let updated_entries = ok_or_panic(
            h.query_audit(QueryAuditRequest {
                collection: Some(col.clone()),
                operation: Some("template.update".into()),
                ..Default::default()
            }),
            "querying template update audit entries",
        );
        assert_eq!(updated_entries.entries.len(), 1);
        let updated_entry = &updated_entries.entries[0];
        assert_eq!(updated_entry.actor, "editor");
        assert!(updated_entry.timestamp_ns > 0);
        assert_eq!(updated_entry.mutation, MutationType::TemplateUpdate);
        assert_eq!(updated_entry.version, 2);
        assert_eq!(
            updated_entry.data_before,
            Some(json!({
                "collection": "tasks",
                "markdown_template": "# {{title}}",
                "version": 1,
                "updated_at_ns": created.view.updated_at_ns,
                "updated_by": "creator",
            }))
        );
        assert_eq!(
            updated_entry.data_after,
            Some(json!({
                "collection": "tasks",
                "markdown_template": "## {{title}}\n\nStatus: {{status}}",
                "version": 2,
                "updated_at_ns": updated.view.updated_at_ns,
                "updated_by": "editor",
            }))
        );

        let deleted_entries = ok_or_panic(
            h.query_audit(QueryAuditRequest {
                collection: Some(col),
                operation: Some("template.delete".into()),
                ..Default::default()
            }),
            "querying template delete audit entries",
        );
        assert_eq!(deleted_entries.entries.len(), 1);
        let deleted_entry = &deleted_entries.entries[0];
        assert_eq!(deleted_entry.actor, "cleaner");
        assert!(deleted_entry.timestamp_ns > 0);
        assert_eq!(deleted_entry.mutation, MutationType::TemplateDelete);
        assert_eq!(deleted_entry.version, 2);
        assert_eq!(
            deleted_entry.data_before,
            Some(json!({
                "collection": "tasks",
                "markdown_template": "## {{title}}\n\nStatus: {{status}}",
                "version": 2,
                "updated_at_ns": updated.view.updated_at_ns,
                "updated_by": "editor",
            }))
        );
        assert!(deleted_entry.data_after.is_none());
    }

    #[test]
    fn qualified_collection_template_crud_round_trips_in_registered_namespace() {
        let mut h = handler();
        let qualified = CollectionId::new("prod.billing.tasks");
        let bare = CollectionId::new("tasks");
        let id = EntityId::new("task-001");
        let billing = Namespace::new("prod", "billing");

        h.storage_mut()
            .create_database("prod")
            .expect("database create should succeed");
        h.storage_mut()
            .create_namespace(&billing)
            .expect("namespace create should succeed");
        h.storage_mut()
            .register_collection_in_namespace(&bare, &billing)
            .expect("collection register should succeed");
        h.storage_mut()
            .put_schema(&CollectionSchema {
                collection: qualified.clone(),
                description: None,
                version: 1,
                entity_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "notes": {"type": "string"}
                    },
                    "required": ["title"]
                })),
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            })
            .expect("qualified schema put should succeed");
        h.create_entity(CreateEntityRequest {
            collection: qualified.clone(),
            id: id.clone(),
            data: json!({"title": "Qualified", "notes": "Scoped"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .expect("qualified entity create should succeed");

        let stored = ok_or_panic(
            h.put_collection_template(PutCollectionTemplateRequest {
                collection: qualified.clone(),
                template: "# {{title}}\n\n{{notes}}".into(),
                actor: Some("operator".into()),
            }),
            "storing qualified collection template through handler",
        );
        assert_eq!(stored.view.collection, qualified);
        assert_eq!(stored.view.version, 1);

        let retrieved = ok_or_panic(
            h.get_collection_template(GetCollectionTemplateRequest {
                collection: qualified.clone(),
            }),
            "retrieving qualified collection template through handler",
        );
        assert_eq!(retrieved.view, stored.view);

        assert_rendered_markdown(
            ok_or_panic(
                h.get_entity_markdown(&qualified, &id),
                "rendering markdown with qualified collection view",
            ),
            "# Qualified\n\nScoped",
        );

        let deleted = ok_or_panic(
            h.delete_collection_template(DeleteCollectionTemplateRequest {
                collection: qualified.clone(),
                actor: None,
            }),
            "deleting qualified collection template through handler",
        );
        assert_eq!(deleted.collection, qualified.to_string());

        let error = err_or_panic(
            h.get_collection_template(GetCollectionTemplateRequest {
                collection: qualified,
            }),
            "expecting missing template after qualified delete",
        );
        assert!(matches!(error, AxonError::NotFound(_)));
    }

    #[test]
    fn get_entity_markdown_preserves_entity_on_render_failure() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating collection for render failure test",
        );
        ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "hello"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity for render failure test",
        );
        ok_or_panic(
            h.storage_mut()
                .put_collection_view(&CollectionView::new(col.clone(), "{{#title}")),
            "storing invalid collection view for render failure test",
        );

        let rendered = ok_or_panic(
            h.get_entity_markdown(&col, &id),
            "rendering markdown with invalid template",
        );

        match rendered {
            GetEntityMarkdownResponse::RenderFailed { entity, detail } => {
                assert_eq!(entity.id, id);
                assert_eq!(entity.data["title"], "hello");
                assert!(detail.contains("failed to render markdown"));
            }
            GetEntityMarkdownResponse::Rendered { .. } => {
                panic!("expected markdown render to fail")
            }
        }
    }

    #[test]
    fn update_entity_increments_version() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let updated = h
            .update_entity(UpdateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"title": "v2"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(updated.entity.version, 2);
        assert_eq!(updated.entity.data["title"], "v2");
    }

    #[test]
    fn occ_rejects_stale_version() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let err = h
            .update_entity(UpdateEntityRequest {
                collection: col,
                id,
                data: json!({"title": "v2"}),
                expected_version: 99, // wrong version
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(
                err,
                AxonError::ConflictingVersion {
                    expected: 99,
                    actual: 1,
                    ..
                }
            ),
            "unexpected error: {err}"
        );
        // current_entity must carry the stored state so callers can merge and retry (FEAT-004, FEAT-008).
        if let AxonError::ConflictingVersion { current_entity, .. } = err {
            let ce = current_entity.expect("current_entity must be present in conflict response");
            assert_eq!(ce.version, 1);
        }
    }

    #[test]
    fn delete_entity_removes_it() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "to-delete"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            actor: None,
            audit_metadata: None,
            force: false,
            attribution: None,
        })
        .unwrap();

        let result = h.get_entity(GetEntityRequest {
            collection: col,
            id,
        });
        assert!(matches!(result, Err(AxonError::NotFound(_))));
    }

    #[test]
    fn create_update_delete_produce_audit_entries() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: Some("agent-1".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: Some("agent-1".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col,
            id,
            actor: None,
            audit_metadata: None,
            force: false,
            attribution: None,
        })
        .unwrap();

        assert_eq!(
            h.audit_log().len(),
            3,
            "expected 3 audit entries (create/update/delete)"
        );
    }

    // ── Schema validation ────────────────────────────────────────────────────

    const TASK_ESF: &str = r#"
esf_version: "1.0"
collection: tasks
entity_schema:
  type: object
  required: [title]
  properties:
    title:
      type: string
    done:
      type: boolean
"#;

    #[test]
    fn schema_validation_rejects_invalid_write() {
        let mut h = handler();
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema()
            .unwrap();
        h.put_schema(schema).unwrap();

        // Missing required "title" field.
        let err = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                data: json!({"done": false}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );
    }

    #[test]
    fn schema_validation_accepts_valid_write() {
        let mut h = handler();
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema()
            .unwrap();
        h.put_schema(schema).unwrap();

        let result = h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "My task", "done": false}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        });

        assert!(result.is_ok(), "valid entity should be accepted");
    }

    // ── Link operations ──────────────────────────────────────────────────────

    fn make_entity(h: &mut AxonHandler<MemoryStorageAdapter>, col: &str, id: &str) {
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new(col),
            id: EntityId::new(id),
            data: json!({"name": id}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    #[test]
    fn link_creation_between_entities() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "assigned-to");
    }

    #[test]
    fn link_to_missing_entity_fails() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");

        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("ghost"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn create_link_produces_audit_entry() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        // Two audit entries already exist from make_entity calls.
        let before = h.audit_log().len();

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: Some("agent-1".into()),
            attribution: None,
        })
        .unwrap();

        assert_eq!(
            h.audit_log().len(),
            before + 1,
            "create_link must produce exactly one audit entry"
        );

        let resp = h
            .query_audit(QueryAuditRequest {
                operation: Some("link.create".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(resp.entries.len(), 1, "exactly one link.create entry");
        let entry = &resp.entries[0];
        assert_eq!(entry.mutation, axon_audit::entry::MutationType::LinkCreate);
        assert_eq!(entry.actor, "agent-1");
        assert!(
            entry.data_before.is_none(),
            "link.create must have no before state"
        );
        assert!(
            entry.data_after.is_some(),
            "link.create must record after state"
        );
    }

    #[test]
    fn delete_link_produces_audit_entry() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: None,
            attribution: None,
        })
        .unwrap();

        let before = h.audit_log().len();

        h.delete_link(DeleteLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            actor: Some("agent-2".into()),
            attribution: None,
        })
        .unwrap();

        assert_eq!(
            h.audit_log().len(),
            before + 1,
            "delete_link must produce exactly one audit entry"
        );

        let resp = h
            .query_audit(QueryAuditRequest {
                operation: Some("link.delete".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(resp.entries.len(), 1, "exactly one link.delete entry");
        let entry = &resp.entries[0];
        assert_eq!(entry.mutation, axon_audit::entry::MutationType::LinkDelete);
        assert_eq!(entry.actor, "agent-2");
        assert!(
            entry.data_before.is_some(),
            "link.delete must record before state"
        );
        assert!(
            entry.data_after.is_none(),
            "link.delete must have no after state"
        );
    }

    #[test]
    fn traversal_follows_links_to_depth_3() {
        let mut h = handler();
        // Chain: a -> b -> c -> d (depth 3 from a reaches d)
        for name in ["a", "b", "c", "d"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c"), ("c", "d")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "next".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }

        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("a"),
                link_type: Some("next".into()),
                max_depth: Some(3),
                direction: TraverseDirection::Forward,
                hop_filter: None,
            })
            .unwrap();

        let ids: Vec<_> = resp.entities.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"b"), "should reach b");
        assert!(ids.contains(&"c"), "should reach c");
        assert!(ids.contains(&"d"), "should reach d at depth 3");
    }

    #[test]
    fn traversal_does_not_revisit_cycles() {
        let mut h = handler();
        // Ring: a -> b -> a
        for name in ["a", "b"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "a")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "edge".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }

        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("a"),
                link_type: None,
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
                hop_filter: None,
            })
            .unwrap();

        // Should only see "b" (not "a" again, not infinite loop)
        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "b");
    }

    #[test]
    fn traversal_reverse_follows_inbound_links() {
        let mut h = handler();
        // Chain: a -> b -> c. Reverse from c should reach b, then a.
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "next".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }

        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("c"),
                link_type: Some("next".into()),
                max_depth: Some(3),
                direction: TraverseDirection::Reverse,
                hop_filter: None,
            })
            .unwrap();

        let ids: Vec<_> = resp.entities.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"b"), "reverse from c should reach b");
        assert!(ids.contains(&"a"), "reverse from c should reach a");
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn traversal_returns_paths_and_links() {
        let mut h = handler();
        // Chain: a -> b -> c
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "next".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }

        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("a"),
                link_type: Some("next".into()),
                max_depth: Some(3),
                direction: TraverseDirection::Forward,
                hop_filter: None,
            })
            .unwrap();

        // Two entities reachable, two links traversed, two paths.
        assert_eq!(resp.entities.len(), 2);
        assert_eq!(resp.links.len(), 2);
        assert_eq!(resp.paths.len(), 2);

        // Path to b has 1 hop, path to c has 2 hops.
        let path_to_b = resp
            .paths
            .iter()
            .find(|p| p.hops.last().unwrap().entity.id.as_str() == "b")
            .expect("path to b");
        assert_eq!(path_to_b.hops.len(), 1);

        let path_to_c = resp
            .paths
            .iter()
            .find(|p| p.hops.last().unwrap().entity.id.as_str() == "c")
            .expect("path to c");
        assert_eq!(path_to_c.hops.len(), 2);

        // Each hop carries the link that was traversed.
        assert_eq!(path_to_c.hops[0].link.link_type, "next");
        assert_eq!(path_to_c.hops[0].entity.id.as_str(), "b");
        assert_eq!(path_to_c.hops[1].entity.id.as_str(), "c");
    }

    #[test]
    fn traversal_hop_filter_excludes_entities() {
        let mut h = handler();
        // Chain: a -> b -> c. b has status "inactive", c has "active".
        make_entity(&mut h, "nodes", "a");
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("nodes"),
            id: EntityId::new("b"),
            data: json!({"status": "inactive"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("nodes"),
            id: EntityId::new("c"),
            data: json!({"status": "active"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        for (src, tgt) in [("a", "b"), ("b", "c")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "next".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }

        // Filter: only entities where status == "active".
        let resp = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("nodes"),
                id: EntityId::new("a"),
                link_type: None,
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
                hop_filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("active"),
                })),
            })
            .unwrap();

        // b is excluded by hop_filter, so traversal stops at b and never reaches c.
        assert!(
            resp.entities.is_empty(),
            "no entities match the hop filter at depth 1"
        );
    }

    #[test]
    fn reachable_returns_true_when_path_exists() {
        let mut h = handler();
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "next".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }

        let resp = h
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("c"),
                link_type: Some("next".into()),
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
            })
            .unwrap();

        assert!(resp.reachable);
        assert_eq!(resp.depth, Some(2));
    }

    #[test]
    fn reachable_returns_false_when_no_path() {
        let mut h = handler();
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        // Only a -> b, no path from a to c.
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("nodes"),
            source_id: EntityId::new("a"),
            target_collection: CollectionId::new("nodes"),
            target_id: EntityId::new("b"),
            link_type: "next".into(),
            metadata: json!(null),
            actor: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("c"),
                link_type: Some("next".into()),
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
            })
            .unwrap();

        assert!(!resp.reachable);
        assert_eq!(resp.depth, None);
    }

    #[test]
    fn reachable_same_entity_returns_depth_zero() {
        let mut h = handler();
        make_entity(&mut h, "nodes", "a");

        let resp = h
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("a"),
                link_type: None,
                max_depth: Some(5),
                direction: TraverseDirection::Forward,
            })
            .unwrap();

        assert!(resp.reachable);
        assert_eq!(resp.depth, Some(0));
    }

    #[test]
    fn reachable_reverse_finds_inbound_path() {
        let mut h = handler();
        for name in ["a", "b", "c"] {
            make_entity(&mut h, "nodes", name);
        }
        for (src, tgt) in [("a", "b"), ("b", "c")] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new(src),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new(tgt),
                link_type: "next".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }

        // Reverse from c should reach a in 2 hops.
        let resp = h
            .reachable(ReachableRequest {
                source_collection: CollectionId::new("nodes"),
                source_id: EntityId::new("c"),
                target_collection: CollectionId::new("nodes"),
                target_id: EntityId::new("a"),
                link_type: Some("next".into()),
                max_depth: Some(5),
                direction: TraverseDirection::Reverse,
            })
            .unwrap();

        assert!(resp.reachable);
        assert_eq!(resp.depth, Some(2));
    }

    // ── Audit query ──────────────────────────────────────────────────────────

    #[test]
    fn update_audit_entry_has_diff() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1", "done": false}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2", "done": false}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let entries = h.audit_log().query_by_entity(&col, &id).unwrap();
        let update_entry = entries
            .iter()
            .find(|e| e.mutation == axon_audit::entry::MutationType::EntityUpdate)
            .unwrap();
        let diff = update_entry
            .diff
            .as_ref()
            .expect("diff should be present on update");
        assert!(
            diff.contains_key("title"),
            "title field should appear in diff"
        );
        assert_eq!(diff["title"].before, Some(json!("v1")));
        assert_eq!(diff["title"].after, Some(json!("v2")));
    }

    #[test]
    fn query_audit_filters_by_actor() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "by alice"}),
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-002"),
            data: json!({"title": "by bob"}),
            actor: Some("bob".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .query_audit(QueryAuditRequest {
                actor: Some("alice".into()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].actor, "alice");
    }

    #[test]
    fn query_audit_filters_by_explicit_database_scope() {
        let mut h = handler();
        let default = CollectionId::new("tasks");
        let (prod, _) = register_prod_billing_and_engineering_collection(&mut h, "tasks");

        h.create_collection(CreateCollectionRequest {
            name: default.clone(),
            schema: CollectionSchema::new(default.clone()),
            actor: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: default,
            id: EntityId::new("t-001"),
            data: json!({"scope": "default"}),
            actor: Some("default-agent".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: prod.clone(),
            id: EntityId::new("t-001"),
            data: json!({"scope": "prod"}),
            actor: Some("prod-agent".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .query_audit(QueryAuditRequest {
                database: Some("prod".into()),
                ..Default::default()
            })
            .unwrap();

        assert!(!resp.entries.is_empty());
        assert!(resp.entries.iter().all(|entry| entry.collection == prod));
    }

    #[test]
    fn query_audit_pagination() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        for i in 0..5u32 {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("t-{i:03}")),
                data: json!({"title": format!("task {i}")}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        let page1 = h
            .query_audit(QueryAuditRequest {
                limit: Some(2),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page1.entries.len(), 2);
        assert!(page1.next_cursor.is_some());

        let page2 = h
            .query_audit(QueryAuditRequest {
                limit: Some(2),
                after_id: page1.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page2.entries.len(), 2);

        let page3 = h
            .query_audit(QueryAuditRequest {
                limit: Some(2),
                after_id: page2.next_cursor,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page3.entries.len(), 1);
        assert!(page3.next_cursor.is_none());
    }

    // ── Revert ───────────────────────────────────────────────────────────────

    #[test]
    fn revert_restores_entity_to_before_state() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Find the update audit entry.
        let entries = h.audit_log().query_by_entity(&col, &id).unwrap();
        let update_entry = entries
            .iter()
            .find(|e| e.mutation == axon_audit::entry::MutationType::EntityUpdate)
            .unwrap();

        let resp = h
            .revert_entity_to_audit_entry(RevertEntityRequest {
                audit_entry_id: update_entry.id,
                actor: Some("admin".into()),
                force: false,
                attribution: None,
            })
            .unwrap();

        assert_eq!(
            resp.entity.data["title"], "v1",
            "entity should be restored to v1"
        );
        assert_eq!(
            resp.audit_entry.mutation,
            axon_audit::entry::MutationType::EntityRevert
        );
        assert_eq!(
            resp.audit_entry
                .metadata
                .get("reverted_from_entry_id")
                .map(String::as_str),
            Some(&update_entry.id.to_string() as &str)
        );
        // Audit log should have 4 entries: create, update, revert
        assert_eq!(h.audit_log().len(), 3);
    }

    #[test]
    fn revert_missing_audit_entry_returns_not_found() {
        let mut h = handler();
        let err = h
            .revert_entity_to_audit_entry(RevertEntityRequest {
                audit_entry_id: 9999,
                actor: None,
                force: false,
                attribution: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn revert_create_entry_fails_no_before() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let entries = h.audit_log().query_by_entity(&col, &id).unwrap();
        let create_entry = &entries[0];

        let err = h
            .revert_entity_to_audit_entry(RevertEntityRequest {
                audit_entry_id: create_entry.id,
                actor: None,
                force: false,
                attribution: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidOperation(_)));
    }

    #[test]
    fn rollback_entity_to_version_restores_target_state_and_audits() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v3"}),
            expected_version: 2,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .rollback_entity(RollbackEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                target: RollbackEntityTarget::Version(1),
                expected_version: None,
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        let RollbackEntityResponse::Applied {
            entity,
            audit_entry,
        } = resp
        else {
            panic!("rollback should apply");
        };

        assert_eq!(entity.version, 4);
        assert_eq!(entity.data["title"], "v1");
        assert_eq!(audit_entry.mutation, MutationType::EntityRevert);
        assert_eq!(
            audit_entry
                .metadata
                .get("reverted_from_entry_id")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn rollback_entity_dry_run_returns_current_target_and_diff() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .rollback_entity(RollbackEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                target: RollbackEntityTarget::Version(1),
                expected_version: None,
                actor: Some("admin".into()),
                dry_run: true,
            })
            .unwrap();

        let RollbackEntityResponse::DryRun {
            current,
            target,
            diff,
        } = resp
        else {
            panic!("rollback should return dry-run preview");
        };

        let current = current.expect("live entity dry run should return current state");
        assert_eq!(current.version, 2);
        assert_eq!(current.data["title"], "v2");
        assert_eq!(target.version, 3);
        assert_eq!(target.data["title"], "v1");
        assert_eq!(
            diff.get("title").and_then(|field| field.after.as_ref()),
            Some(&json!("v1"))
        );

        let stored = h
            .get_entity(GetEntityRequest {
                collection: col,
                id,
            })
            .unwrap();
        assert_eq!(stored.entity.version, 2, "dry run must not persist changes");
    }

    #[test]
    fn rollback_entity_missing_version_returns_not_found() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let err = h
            .rollback_entity(RollbackEntityRequest {
                collection: col,
                id,
                target: RollbackEntityTarget::Version(99),
                expected_version: None,
                actor: None,
                dry_run: false,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn rollback_entity_recreates_deleted_entity_from_audit_snapshot() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            actor: Some("alice".into()),
            force: false,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .rollback_entity(RollbackEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                target: RollbackEntityTarget::Version(1),
                expected_version: None,
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        let RollbackEntityResponse::Applied {
            entity,
            audit_entry,
        } = resp
        else {
            panic!("rollback should recreate deleted entity");
        };

        assert_eq!(entity.version, 3);
        assert_eq!(entity.data["title"], "v1");
        assert_eq!(entity.created_by.as_deref(), Some("alice"));
        assert_eq!(entity.updated_by.as_deref(), Some("admin"));
        assert_eq!(audit_entry.mutation, MutationType::EntityRevert);
        assert_eq!(audit_entry.data_before, None);
        assert_eq!(audit_entry.data_after, Some(json!({"title": "v1"})));
        assert_eq!(
            audit_entry
                .metadata
                .get("reverted_from_entry_id")
                .map(String::as_str),
            Some("1")
        );

        let stored = h
            .get_entity(GetEntityRequest {
                collection: col,
                id,
            })
            .unwrap();
        assert_eq!(stored.entity.version, 3);
        assert_eq!(stored.entity.data["title"], "v1");
    }

    #[test]
    fn rollback_entity_recreate_conflicts_when_another_writer_restores_first() {
        let mut h = AxonHandler::new(RaceOnCreateIfAbsentAdapter::default());
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            actor: Some("alice".into()),
            force: false,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let err = h
            .rollback_entity(RollbackEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                target: RollbackEntityTarget::Version(1),
                expected_version: None,
                actor: Some("admin".into()),
                dry_run: false,
            })
            .expect_err("stale deleted-entity rollback should conflict");

        assert!(matches!(
            err,
            AxonError::ConflictingVersion {
                expected: 2,
                actual: 3,
                ..
            }
        ));
        if let AxonError::ConflictingVersion {
            current_entity: Some(current_entity),
            ..
        } = err
        {
            assert_eq!(current_entity.version, 3);
            assert_eq!(current_entity.data["title"], "concurrent");
        } else {
            panic!("expected current entity snapshot on rollback conflict");
        }

        let stored = h
            .storage_mut()
            .get(&col, &id)
            .unwrap()
            .expect("concurrent recreate should survive");
        assert_eq!(stored.version, 3);
        assert_eq!(stored.data["title"], "concurrent");
        assert_eq!(
            h.audit_log()
                .query_by_operation(&MutationType::EntityRevert)
                .unwrap()
                .len(),
            0,
            "failed rollback must not append revert audit entries"
        );
    }

    #[test]
    fn rollback_entity_dry_run_previews_deleted_entity_recreation_without_write() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1", "status": "draft"}),
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2", "status": "published"}),
            expected_version: 1,
            actor: Some("alice".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            actor: Some("alice".into()),
            force: false,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .rollback_entity(RollbackEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                target: RollbackEntityTarget::Version(1),
                expected_version: None,
                actor: Some("admin".into()),
                dry_run: true,
            })
            .unwrap();

        let RollbackEntityResponse::DryRun {
            current,
            target,
            diff,
        } = resp
        else {
            panic!("rollback should return deleted-entity dry-run preview");
        };

        assert!(current.is_none());
        assert_eq!(target.version, 3);
        assert_eq!(target.data["title"], "v1");
        assert_eq!(target.data["status"], "draft");
        assert_eq!(
            diff.get("title").and_then(|field| field.before.as_ref()),
            None
        );
        assert_eq!(
            diff.get("title").and_then(|field| field.after.as_ref()),
            Some(&json!("v1"))
        );
        assert_eq!(
            diff.get("status").and_then(|field| field.after.as_ref()),
            Some(&json!("draft"))
        );

        let stored = h
            .get_entity(GetEntityRequest {
                collection: col,
                id,
            })
            .unwrap_err();
        assert!(matches!(stored, AxonError::NotFound(_)));
    }

    #[test]
    fn rollback_entity_honors_expected_version() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let id = EntityId::new("t-001");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let err = h
            .rollback_entity(RollbackEntityRequest {
                collection: col,
                id,
                target: RollbackEntityTarget::Version(1),
                expected_version: Some(1),
                actor: None,
                dry_run: false,
            })
            .unwrap_err();

        assert!(matches!(
            err,
            AxonError::ConflictingVersion {
                expected: 1,
                actual: 2,
                ..
            }
        ));
    }

    #[test]
    fn rollback_entity_returns_schema_validation_when_save_gate_fails() {
        let mut h = handler();
        let col = CollectionId::new("items");
        let id = EntityId::new("g-rollback");

        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({"title": "draft"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let gated_schema = handler_with_gated_schema()
            .storage
            .get_schema(&CollectionId::new("items"))
            .unwrap()
            .unwrap();
        h.put_schema(gated_schema).unwrap();

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            data: json!({
                "title": "draft",
                "bead_type": "task",
                "description": "ready",
                "acceptance": "defined"
            }),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let err = h
            .rollback_entity(RollbackEntityRequest {
                collection: col,
                id,
                target: RollbackEntityTarget::Version(1),
                expected_version: None,
                actor: None,
                dry_run: false,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::SchemaValidation(_)));
        assert!(err.to_string().contains("save gate failed"));
    }

    // ── Collection-level rollback ────────────────────────────────────────────

    #[test]
    fn rollback_collection_reverts_entities_created_after_timestamp() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        // Create entity e1 — this will be "before" the rollback timestamp.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "original"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Record the timestamp after e1 creation (the audit entry timestamp).
        let entries = h.audit.query_by_entity(&col, &EntityId::new("e1")).unwrap();
        let cutoff_ns = entries.last().unwrap().timestamp_ns;

        // Mutate e1 after the cutoff.
        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "modified"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Create entity e2 after the cutoff (should be deleted on rollback).
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e2"),
            data: json!({"title": "new entity"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Perform collection rollback to the cutoff.
        let resp = h
            .rollback_collection(RollbackCollectionRequest {
                collection: col.clone(),
                timestamp_ns: cutoff_ns,
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        assert_eq!(resp.entities_affected, 2);
        assert_eq!(resp.entities_rolled_back, 2);
        assert_eq!(resp.errors, 0);
        assert!(!resp.dry_run);

        // e1 should be rolled back to "original".
        let e1 = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("e1"),
            })
            .unwrap();
        assert_eq!(e1.entity.data["title"], "original");

        // e2 should no longer exist.
        let e2_err = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("e2"),
            })
            .unwrap_err();
        assert!(matches!(e2_err, AxonError::NotFound(_)));
    }

    #[test]
    fn rollback_collection_dry_run_does_not_modify_data() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let entries = h.audit.query_by_entity(&col, &EntityId::new("e1")).unwrap();
        let cutoff_ns = entries.last().unwrap().timestamp_ns;

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .rollback_collection(RollbackCollectionRequest {
                collection: col.clone(),
                timestamp_ns: cutoff_ns,
                actor: Some("admin".into()),
                dry_run: true,
            })
            .unwrap();

        assert_eq!(resp.entities_affected, 1);
        assert_eq!(resp.entities_rolled_back, 1);
        assert!(resp.dry_run);

        // Data should remain unchanged because dry_run=true.
        let e1 = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("e1"),
            })
            .unwrap();
        assert_eq!(e1.entity.data["title"], "v2");
    }

    #[test]
    fn rollback_collection_no_mutations_after_timestamp() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Use a timestamp far in the future — nothing to roll back.
        let resp = h
            .rollback_collection(RollbackCollectionRequest {
                collection: col.clone(),
                timestamp_ns: u64::MAX - 1,
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        assert_eq!(resp.entities_affected, 0);
        assert_eq!(resp.entities_rolled_back, 0);
        assert_eq!(resp.errors, 0);
    }

    #[test]
    fn rollback_collection_restores_deleted_entity() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "alive"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let entries = h.audit.query_by_entity(&col, &EntityId::new("e1")).unwrap();
        let cutoff_ns = entries.last().unwrap().timestamp_ns;

        // Delete e1 after the cutoff.
        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            actor: None,
            force: false,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Rollback should restore e1.
        let resp = h
            .rollback_collection(RollbackCollectionRequest {
                collection: col.clone(),
                timestamp_ns: cutoff_ns,
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        assert_eq!(resp.entities_affected, 1);
        assert_eq!(resp.entities_rolled_back, 1);

        let e1 = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("e1"),
            })
            .unwrap();
        assert_eq!(e1.entity.data["title"], "alive");
    }

    #[test]
    fn rollback_collection_audit_entries_contain_metadata() {
        let mut h = handler();
        let col = CollectionId::new("tasks");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "v1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let entries = h.audit.query_by_entity(&col, &EntityId::new("e1")).unwrap();
        let cutoff_ns = entries.last().unwrap().timestamp_ns;

        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("e1"),
            data: json!({"title": "v2"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.rollback_collection(RollbackCollectionRequest {
            collection: col.clone(),
            timestamp_ns: cutoff_ns,
            actor: Some("admin".into()),
            dry_run: false,
        })
        .unwrap();

        // The latest audit entry for e1 should be the rollback revert.
        let entries = h.audit.query_by_entity(&col, &EntityId::new("e1")).unwrap();
        let last = entries.last().unwrap();
        assert_eq!(last.mutation, MutationType::EntityRevert);
        assert_eq!(
            last.metadata.get("collection_rollback").map(String::as_str),
            Some("true")
        );
        let expected_ts = cutoff_ns.to_string();
        assert_eq!(
            last.metadata
                .get("rollback_timestamp_ns")
                .map(String::as_str),
            Some(expected_ts.as_str())
        );
    }

    // ── Transaction rollback ──────────────────────────────────────────────

    #[test]
    fn rollback_transaction_reverts_updates() {
        let mut h = handler();
        let col = CollectionId::new("accounts");

        // Seed two entities.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("a"),
            data: json!({"balance": 100}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("b"),
            data: json!({"balance": 50}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Commit a transaction that updates both.
        use crate::transaction::Transaction;
        let a_before = h
            .storage_ref()
            .get(&col, &EntityId::new("a"))
            .unwrap()
            .unwrap();
        let b_before = h
            .storage_ref()
            .get(&col, &EntityId::new("b"))
            .unwrap()
            .unwrap();

        let mut tx = Transaction::new();
        let tx_id = tx.id.clone();
        tx.update(
            Entity::new(col.clone(), EntityId::new("a"), json!({"balance": 70})),
            a_before.version,
            Some(a_before.data.clone()),
        )
        .unwrap();
        tx.update(
            Entity::new(col.clone(), EntityId::new("b"), json!({"balance": 80})),
            b_before.version,
            Some(b_before.data.clone()),
        )
        .unwrap();

        h.commit_transaction(tx, Some("system".into()), None)
            .unwrap();

        // Verify the transaction was applied.
        let a = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("a"),
            })
            .unwrap();
        assert_eq!(a.entity.data["balance"], 70);
        let b = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("b"),
            })
            .unwrap();
        assert_eq!(b.entity.data["balance"], 80);

        // Roll back the transaction.
        let resp = h
            .rollback_transaction(RollbackTransactionRequest {
                transaction_id: tx_id.clone(),
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        assert_eq!(resp.transaction_id, tx_id);
        assert_eq!(resp.entities_affected, 2);
        assert_eq!(resp.entities_rolled_back, 2);
        assert_eq!(resp.errors, 0);
        assert!(!resp.dry_run);

        // Both entities should be back to their pre-transaction state.
        let a = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("a"),
            })
            .unwrap();
        assert_eq!(a.entity.data["balance"], 100);
        let b = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("b"),
            })
            .unwrap();
        assert_eq!(b.entity.data["balance"], 50);
    }

    #[test]
    fn rollback_transaction_reverts_creates() {
        let mut h = handler();
        let col = CollectionId::new("accounts");

        // Commit a transaction that creates two entities.
        use crate::transaction::Transaction;
        let mut tx = Transaction::new();
        let tx_id = tx.id.clone();
        tx.create(Entity::new(
            col.clone(),
            EntityId::new("x"),
            json!({"balance": 200}),
        ))
        .unwrap();
        tx.create(Entity::new(
            col.clone(),
            EntityId::new("y"),
            json!({"balance": 300}),
        ))
        .unwrap();

        h.commit_transaction(tx, Some("system".into()), None)
            .unwrap();

        // Both entities should exist.
        assert!(h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("x")
            })
            .is_ok());
        assert!(h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("y")
            })
            .is_ok());

        // Roll back the transaction.
        let resp = h
            .rollback_transaction(RollbackTransactionRequest {
                transaction_id: tx_id.clone(),
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        assert_eq!(resp.entities_affected, 2);
        assert_eq!(resp.entities_rolled_back, 2);
        assert_eq!(resp.errors, 0);

        // Both entities should no longer exist.
        assert!(h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("x")
            })
            .is_err());
        assert!(h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("y")
            })
            .is_err());
    }

    #[test]
    fn rollback_transaction_reverts_deletes() {
        let mut h = handler();
        let col = CollectionId::new("accounts");

        // Create an entity, then delete it in a transaction.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("z"),
            data: json!({"balance": 999}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let z_before = h
            .storage_ref()
            .get(&col, &EntityId::new("z"))
            .unwrap()
            .unwrap();

        use crate::transaction::Transaction;
        let mut tx = Transaction::new();
        let tx_id = tx.id.clone();
        tx.delete(
            col.clone(),
            EntityId::new("z"),
            z_before.version,
            Some(z_before.data.clone()),
        )
        .unwrap();

        h.commit_transaction(tx, Some("system".into()), None)
            .unwrap();

        // Entity should be gone.
        assert!(h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("z")
            })
            .is_err());

        // Roll back the transaction — entity should be restored.
        let resp = h
            .rollback_transaction(RollbackTransactionRequest {
                transaction_id: tx_id.clone(),
                actor: Some("admin".into()),
                dry_run: false,
            })
            .unwrap();

        assert_eq!(resp.entities_affected, 1);
        assert_eq!(resp.entities_rolled_back, 1);
        assert_eq!(resp.errors, 0);

        let z = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("z"),
            })
            .unwrap();
        assert_eq!(z.entity.data["balance"], 999);
    }

    #[test]
    fn rollback_transaction_dry_run_does_not_modify_data() {
        let mut h = handler();
        let col = CollectionId::new("accounts");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("a"),
            data: json!({"balance": 100}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        use crate::transaction::Transaction;
        let a_before = h
            .storage_ref()
            .get(&col, &EntityId::new("a"))
            .unwrap()
            .unwrap();

        let mut tx = Transaction::new();
        let tx_id = tx.id.clone();
        tx.update(
            Entity::new(col.clone(), EntityId::new("a"), json!({"balance": 70})),
            a_before.version,
            Some(a_before.data.clone()),
        )
        .unwrap();

        h.commit_transaction(tx, Some("system".into()), None)
            .unwrap();

        // Dry-run rollback.
        let resp = h
            .rollback_transaction(RollbackTransactionRequest {
                transaction_id: tx_id.clone(),
                actor: Some("admin".into()),
                dry_run: true,
            })
            .unwrap();

        assert_eq!(resp.entities_affected, 1);
        assert_eq!(resp.entities_rolled_back, 1);
        assert!(resp.dry_run);

        // Data should remain unchanged (balance = 70, not reverted to 100).
        let a = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("a"),
            })
            .unwrap();
        assert_eq!(a.entity.data["balance"], 70);
    }

    #[test]
    fn rollback_transaction_not_found_for_unknown_id() {
        let mut h = handler();
        let err = h
            .rollback_transaction(RollbackTransactionRequest {
                transaction_id: "tx-nonexistent".into(),
                actor: None,
                dry_run: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn rollback_transaction_audit_entries_contain_metadata() {
        let mut h = handler();
        let col = CollectionId::new("accounts");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("a"),
            data: json!({"balance": 100}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        use crate::transaction::Transaction;
        let a_before = h
            .storage_ref()
            .get(&col, &EntityId::new("a"))
            .unwrap()
            .unwrap();

        let mut tx = Transaction::new();
        let tx_id = tx.id.clone();
        tx.update(
            Entity::new(col.clone(), EntityId::new("a"), json!({"balance": 70})),
            a_before.version,
            Some(a_before.data.clone()),
        )
        .unwrap();

        h.commit_transaction(tx, Some("system".into()), None)
            .unwrap();

        h.rollback_transaction(RollbackTransactionRequest {
            transaction_id: tx_id.clone(),
            actor: Some("admin".into()),
            dry_run: false,
        })
        .unwrap();

        // The latest audit entry for "a" should be the rollback revert
        // with transaction_rollback metadata.
        let entries = h.audit.query_by_entity(&col, &EntityId::new("a")).unwrap();
        let last = entries.last().unwrap();
        assert_eq!(last.mutation, MutationType::EntityRevert);
        assert_eq!(
            last.metadata
                .get("transaction_rollback")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            last.metadata
                .get("rolled_back_transaction_id")
                .map(String::as_str),
            Some(tx_id.as_str())
        );
    }

    // ── Collection lifecycle ─────────────────────────────────────────────────

    #[test]
    fn create_and_drop_collection_produce_audit_entries() {
        let mut h = handler();

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("widgets"),
            schema: CollectionSchema::new(CollectionId::new("widgets")),
            actor: Some("admin".into()),
        })
        .unwrap();

        // Populate with some entities.
        for i in 0..3u32 {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new("widgets"),
                id: EntityId::new(format!("w-{i:03}")),
                data: json!({"name": format!("widget {i}")}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        let drop_resp = h
            .drop_collection(DropCollectionRequest {
                name: CollectionId::new("widgets"),
                actor: Some("admin".into()),
                confirm: true,
            })
            .unwrap();

        assert_eq!(drop_resp.entities_removed, 3);

        // Audit log: 1 CollectionCreate + 3 EntityCreate + 1 CollectionDrop = 5.
        assert_eq!(h.audit_log().len(), 5);

        let col_creates = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionCreate)
            .unwrap();
        assert_eq!(col_creates.len(), 1);

        let col_drops = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionDrop)
            .unwrap();
        assert_eq!(col_drops.len(), 1);
    }

    #[test]
    fn create_duplicate_collection_returns_already_exists() {
        let mut h = handler();
        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("dup"),
            schema: CollectionSchema::new(CollectionId::new("dup")),
            actor: None,
        })
        .unwrap();

        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("dup"),
                schema: CollectionSchema::new(CollectionId::new("dup")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::AlreadyExists(_)));
    }

    #[test]
    fn drop_unknown_collection_returns_not_found() {
        let mut h = handler();
        let err = h
            .drop_collection(DropCollectionRequest {
                name: CollectionId::new("ghost"),
                actor: None,
                confirm: true,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Schema binding at collection creation (FEAT-001) ─────────────────────

    #[test]
    fn create_collection_persists_schema() {
        let mut h = handler();
        let col = CollectionId::new("typed-col");
        let schema = CollectionSchema {
            collection: col.clone(),
            description: Some("a typed collection".into()),
            version: 1,
            entity_schema: Some(json!({"type": "object"})),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: schema.clone(),
            actor: None,
        })
        .unwrap();

        let stored = h
            .get_schema(&col)
            .unwrap()
            .expect("schema must be stored at creation");
        assert_eq!(stored.version, 1);
        assert_eq!(stored.description.as_deref(), Some("a typed collection"));
        assert_eq!(stored.entity_schema, Some(json!({"type": "object"})));
    }

    #[test]
    fn create_collection_rejects_schema_collection_mismatch() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("foo"),
                schema: CollectionSchema::new(CollectionId::new("bar")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    // ── Collection name validation ───────────────────────────────────────────

    #[test]
    fn create_collection_rejects_empty_name() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new(""),
                schema: CollectionSchema::new(CollectionId::new("")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_rejects_name_starting_with_digit() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("1bad"),
                schema: CollectionSchema::new(CollectionId::new("1bad")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_rejects_name_with_uppercase() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("Bad-Name"),
                schema: CollectionSchema::new(CollectionId::new("Bad-Name")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_rejects_name_with_spaces() {
        let mut h = handler();
        let err = h
            .create_collection(CreateCollectionRequest {
                name: CollectionId::new("bad name"),
                schema: CollectionSchema::new(CollectionId::new("bad name")),
                actor: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn create_collection_accepts_valid_names() {
        let mut h = handler();
        for name in &["tasks", "my-tasks", "my_tasks", "tasks2", "a"] {
            h.create_collection(CreateCollectionRequest {
                name: CollectionId::new(*name),
                schema: CollectionSchema::new(CollectionId::new(*name)),
                actor: None,
            })
            .unwrap_or_else(|e| panic!("valid name '{}' rejected: {}", name, e));
        }
    }

    #[test]
    fn create_collection_invalid_entity_schema_leaves_no_orphan() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({"type": "bogus"})),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        let err = h
            .create_collection(CreateCollectionRequest {
                name: col,
                schema,
                actor: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );

        // No orphan: the collection must not appear in the registry.
        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert!(
            resp.collections.is_empty(),
            "orphan collection registered despite invalid schema: {:?}",
            resp.collections
        );
    }

    // ── list_collections ─────────────────────────────────────────────────────

    #[test]
    fn list_collections_empty_when_none_created() {
        let h = handler();
        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert!(resp.collections.is_empty());
    }

    #[test]
    fn list_collections_returns_created_collections() {
        let mut h = handler();

        for name in &["apples", "bananas", "cherries"] {
            h.create_collection(CreateCollectionRequest {
                name: CollectionId::new(*name),
                schema: CollectionSchema::new(CollectionId::new(*name)),
                actor: None,
            })
            .unwrap();
        }

        // Add two entities to "bananas".
        for i in 0..2u32 {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new("bananas"),
                id: EntityId::new(format!("b-{i}")),
                data: json!({"name": format!("b-{i}")}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(resp.collections.len(), 3);

        // Results are sorted by name.
        assert_eq!(resp.collections[0].name, "apples");
        assert_eq!(resp.collections[1].name, "bananas");
        assert_eq!(resp.collections[2].name, "cherries");

        assert_eq!(resp.collections[1].entity_count, 2);
        assert_eq!(resp.collections[0].entity_count, 0);
    }

    #[test]
    fn list_collections_schema_version_reflects_stored_schema() {
        let mut h = handler();

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("items"),
            schema: CollectionSchema::new(CollectionId::new("items")),
            actor: None,
        })
        .unwrap();
        // Auto-increment: create_collection stores v1, this put_schema stores v2.
        h.put_schema(CollectionSchema {
            collection: CollectionId::new("items"),
            description: None,
            version: 99, // ignored — auto-increment assigns v2
            entity_schema: None,
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        })
        .unwrap();

        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(resp.collections[0].schema_version, Some(2));
    }

    // ── describe_collection ──────────────────────────────────────────────────

    #[test]
    fn describe_collection_returns_metadata_and_schema() {
        let mut h = handler();

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("things"),
            schema: CollectionSchema::new(CollectionId::new("things")),
            actor: None,
        })
        .unwrap();
        h.put_schema(CollectionSchema {
            collection: CollectionId::new("things"),
            description: Some("a thing".into()),
            version: 2,
            entity_schema: None,
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("things"),
            id: EntityId::new("t-001"),
            data: json!({}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .describe_collection(DescribeCollectionRequest {
                name: CollectionId::new("things"),
            })
            .unwrap();

        assert_eq!(resp.name, "things");
        assert_eq!(resp.entity_count, 1);
        assert!(resp.schema.is_some());
        assert_eq!(resp.schema.unwrap().version, 2);
        // Timestamp fields populated from audit log (FEAT-001).
        assert!(
            resp.created_at_ns.is_some(),
            "created_at_ns should be populated from audit log"
        );
        assert!(
            resp.updated_at_ns.is_some(),
            "updated_at_ns should be populated from audit log"
        );
        assert!(
            resp.updated_at_ns.unwrap() >= resp.created_at_ns.unwrap(),
            "updated_at_ns should be >= created_at_ns"
        );
    }

    #[test]
    fn describe_collection_not_found_for_unknown() {
        let h = handler();
        let err = h
            .describe_collection(DescribeCollectionRequest {
                name: CollectionId::new("nope"),
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Collection durability (hx-31638e63) ──────────────────────────────────

    /// A handler constructed from storage that already has registered
    /// collections correctly reports them via list_collections and
    /// describe_collection — no re-creation required.
    ///
    /// This is the analogue of a SQLite process-restart: the adapter is
    /// durable; only the AxonHandler is freshly constructed.
    #[test]
    fn pre_populated_storage_reports_collections_on_new_handler() {
        use axon_storage::adapter::StorageAdapter as _;
        let mut storage = MemoryStorageAdapter::default();

        // Directly register a collection into storage (simulates a durable
        // backend that was populated before this handler was constructed).
        storage
            .register_collection(&CollectionId::new("tasks"))
            .unwrap();

        let h = AxonHandler::new(storage);
        let resp = h.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(
            resp.collections.len(),
            1,
            "list_collections should see pre-populated collection"
        );
        assert_eq!(resp.collections[0].name, "tasks");

        // describe_collection must not return NotFound.
        h.describe_collection(DescribeCollectionRequest {
            name: CollectionId::new("tasks"),
        })
        .unwrap();
    }

    /// After creating a collection and extracting the storage adapter, a brand-
    /// new AxonHandler built from that same adapter still sees the collection.
    #[test]
    fn collection_survives_handler_reconstruction() {
        // Build the first handler, create a collection, then recover the storage.
        let mut h1 = handler();
        h1.create_collection(CreateCollectionRequest {
            name: CollectionId::new("widgets"),
            schema: CollectionSchema::new(CollectionId::new("widgets")),
            actor: None,
        })
        .unwrap();

        // Extract storage by consuming the first handler.
        let storage = h1.into_storage();

        // Reconstruct a new handler from the same storage.
        let h2 = AxonHandler::new(storage);
        let resp = h2.list_collections(ListCollectionsRequest {}).unwrap();
        assert_eq!(
            resp.collections.len(),
            1,
            "collection must survive handler reconstruction"
        );
        assert_eq!(resp.collections[0].name, "widgets");

        h2.describe_collection(DescribeCollectionRequest {
            name: CollectionId::new("widgets"),
        })
        .unwrap();
    }

    // ── Link deletion ────────────────────────────────────────────────────────

    #[test]
    fn delete_link_removes_forward_and_reverse_entries() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: None,
            attribution: None,
        })
        .unwrap();

        // Delete the link.
        let resp = h
            .delete_link(DeleteLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                actor: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.link_type, "assigned-to");

        // Forward link must be gone — traversal from u-001 should return nothing.
        let trav = h
            .traverse(TraverseRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-001"),
                link_type: Some("assigned-to".into()),
                max_depth: Some(1),
                direction: TraverseDirection::Forward,
                hop_filter: None,
            })
            .unwrap();
        assert!(trav.entities.is_empty(), "forward link must be removed");

        // Reverse-index must be gone — delete_entity on t-001 must now succeed.
        h.delete_entity(DeleteEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            actor: None,
            audit_metadata: None,
            force: false,
            attribution: None,
        })
        .expect("delete_entity must succeed after reverse-index entry is removed");
    }

    #[test]
    fn delete_link_missing_returns_not_found() {
        let mut h = handler();
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        let err = h
            .delete_link(DeleteLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                actor: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Link-type enforcement (axon-f48352d5) ────────────────────────────────

    const USERS_ESF_WITH_LINKS: &str = r#"
esf_version: "1.0"
collection: users
entity_schema:
  type: object
  required: [name]
  properties:
    name:
      type: string
link_types:
  assigned-to:
    target_collection: tasks
    cardinality: many-to-many
  mentor:
    target_collection: users
    cardinality: many-to-one
    metadata_schema:
      type: object
      required: [since]
      properties:
        since:
          type: string
  manager:
    target_collection: users
    cardinality: one-to-one
"#;

    fn setup_linked_collections(h: &mut AxonHandler<MemoryStorageAdapter>) {
        let schema = EsfDocument::parse(USERS_ESF_WITH_LINKS)
            .unwrap()
            .into_collection_schema()
            .unwrap();
        h.put_schema(schema).unwrap();

        // Also register a tasks schema (no link_types needed for this test).
        let tasks_schema = CollectionSchema::new(CollectionId::new("tasks"));
        h.put_schema(tasks_schema).unwrap();
    }

    #[test]
    fn create_link_rejects_undeclared_link_type() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "undeclared-type".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation for undeclared link type, got: {err}"
        );
    }

    #[test]
    fn create_link_rejects_wrong_target_collection() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");

        // "assigned-to" declares target_collection=tasks, but we target users.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation for wrong target collection, got: {err}"
        );
    }

    #[test]
    fn create_link_validates_metadata_against_schema() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");

        // "mentor" requires metadata with a "since" field.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "mentor".into(),
                metadata: json!({}), // missing required "since"
                actor: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation for invalid metadata, got: {err}"
        );
    }

    #[test]
    fn create_link_accepts_valid_metadata() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "mentor".into(),
                metadata: json!({"since": "2026-01-01"}),
                actor: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "mentor");
    }

    #[test]
    fn create_link_rejects_duplicate_triple() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: json!(null),
            actor: None,
            attribution: None,
        })
        .unwrap();

        // Same triple again should fail.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-001"),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::AlreadyExists(_)),
            "expected AlreadyExists for duplicate link triple, got: {err}"
        );
    }

    #[test]
    fn create_link_allows_untyped_collections() {
        // Collections without schemas should still allow links (no enforcement).
        let mut h = handler();
        make_entity(&mut h, "loose", "a");
        make_entity(&mut h, "loose", "b");

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("loose"),
                source_id: EntityId::new("a"),
                target_collection: CollectionId::new("loose"),
                target_id: EntityId::new("b"),
                link_type: "anything".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "anything");
    }

    #[test]
    fn create_link_allows_schema_without_link_types() {
        // Collections with a schema but no link_types should allow any link.
        let mut h = handler();
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema()
            .unwrap();
        h.put_schema(schema).unwrap();
        // Create entities that match the tasks schema (requires "title").
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "Task 1"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-002"),
            data: json!({"title": "Task 2"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new("t-002"),
                link_type: "depends-on".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.link.link_type, "depends-on");
    }

    // ── Cardinality enforcement (axon-7ac24886) ──────────────────────────────

    #[test]
    fn create_link_enforces_many_to_one_source_limit() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");
        make_entity(&mut h, "users", "u-003");

        // "mentor" is many-to-one: source can have at most one outgoing mentor link.
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("users"),
            target_id: EntityId::new("u-002"),
            link_type: "mentor".into(),
            metadata: json!({"since": "2026-01-01"}),
            actor: None,
            attribution: None,
        })
        .unwrap();

        // Second mentor link from same source should fail.
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-003"),
                link_type: "mentor".into(),
                metadata: json!({"since": "2026-02-01"}),
                actor: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected cardinality violation, got: {err}"
        );
    }

    #[test]
    fn create_link_enforces_one_to_one_both_directions() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "users", "u-002");
        make_entity(&mut h, "users", "u-003");

        // "manager" is one-to-one: at most one outgoing AND one inbound.
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("users"),
            target_id: EntityId::new("u-002"),
            link_type: "manager".into(),
            metadata: json!(null),
            actor: None,
            attribution: None,
        })
        .unwrap();

        // Second outgoing from u-001 should fail (source limit).
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-003"),
                link_type: "manager".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected source cardinality violation, got: {err}"
        );

        // Second inbound to u-002 from different source should fail (target limit).
        let err = h
            .create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-003"),
                target_collection: CollectionId::new("users"),
                target_id: EntityId::new("u-002"),
                link_type: "manager".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected target cardinality violation, got: {err}"
        );
    }

    #[test]
    fn create_link_allows_many_to_many_without_limit() {
        let mut h = handler();
        setup_linked_collections(&mut h);
        make_entity(&mut h, "users", "u-001");
        make_entity(&mut h, "tasks", "t-001");
        make_entity(&mut h, "tasks", "t-002");
        make_entity(&mut h, "tasks", "t-003");

        // "assigned-to" is many-to-many: no limits.
        for tid in ["t-001", "t-002", "t-003"] {
            h.create_link(CreateLinkRequest {
                source_collection: CollectionId::new("users"),
                source_id: EntityId::new("u-001"),
                target_collection: CollectionId::new("tasks"),
                target_id: EntityId::new(tid),
                link_type: "assigned-to".into(),
                metadata: json!(null),
                actor: None,
                attribution: None,
            })
            .unwrap();
        }
    }

    // ── Entity query / filter (US-011) ────────────────────────────────────────

    fn make_entity_with_data(
        h: &mut AxonHandler<MemoryStorageAdapter>,
        collection: &str,
        id: &str,
        data: serde_json::Value,
    ) {
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new(collection),
            id: EntityId::new(id),
            data,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    use crate::request::{
        FieldFilter, FilterNode, FilterOp, QueryEntitiesRequest, SortDirection, SortField,
    };

    #[test]
    fn query_no_filter_returns_all() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: None,
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        assert_eq!(resp.entities.len(), 2);
    }

    #[test]
    fn query_filter_eq() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "open"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("open"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        assert!(resp.entities.iter().all(|e| e.data["status"] == "open"));
    }

    #[test]
    fn query_filter_ne() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Ne,
                    value: json!("done"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].data["status"], "open");
    }

    #[test]
    fn query_filter_gt_and_lte() {
        let mut h = handler();
        make_entity_with_data(&mut h, "issues", "i-1", json!({"priority": 1}));
        make_entity_with_data(&mut h, "issues", "i-2", json!({"priority": 3}));
        make_entity_with_data(&mut h, "issues", "i-3", json!({"priority": 5}));

        // priority > 2
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("issues"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "priority".into(),
                    op: FilterOp::Gt,
                    value: json!(2),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 2);

        // priority <= 3
        let resp2 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("issues"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "priority".into(),
                    op: FilterOp::Lte,
                    value: json!(3),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp2.total_count, 2);
    }

    #[test]
    fn query_filter_in() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "in_progress"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::In,
                    value: json!(["open", "in_progress"]),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
    }

    #[test]
    fn query_filter_contains() {
        let mut h = handler();
        make_entity_with_data(&mut h, "docs", "d-1", json!({"title": "Hello World"}));
        make_entity_with_data(&mut h, "docs", "d-2", json!({"title": "Goodbye World"}));
        make_entity_with_data(&mut h, "docs", "d-3", json!({"title": "Nothing here"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("docs"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "title".into(),
                    op: FilterOp::Contains,
                    value: json!("World"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
    }

    #[test]
    fn query_filter_contains_is_case_sensitive() {
        let mut h = handler();
        make_entity_with_data(&mut h, "docs", "d-1", json!({"title": "Hello World"}));
        make_entity_with_data(&mut h, "docs", "d-2", json!({"title": "hello again"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("docs"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "title".into(),
                    op: FilterOp::Contains,
                    value: json!("Hello"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id.as_str(), "d-1");
    }

    #[test]
    fn query_filter_and_combinator() {
        let mut h = handler();
        make_entity_with_data(
            &mut h,
            "tasks",
            "t-1",
            json!({"status": "open", "assignee": "alice"}),
        );
        make_entity_with_data(
            &mut h,
            "tasks",
            "t-2",
            json!({"status": "open", "assignee": "bob"}),
        );
        make_entity_with_data(
            &mut h,
            "tasks",
            "t-3",
            json!({"status": "done", "assignee": "alice"}),
        );

        // status = "open" AND assignee = "alice"
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::And {
                    filters: vec![
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("open"),
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "assignee".into(),
                            op: FilterOp::Eq,
                            value: json!("alice"),
                        }),
                    ],
                }),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].data["assignee"], "alice");
        assert_eq!(resp.entities[0].data["status"], "open");
    }

    #[test]
    fn query_filter_or_combinator() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "done"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "archived"}));

        // status = "open" OR status = "done"
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Or {
                    filters: vec![
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("open"),
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("done"),
                        }),
                    ],
                }),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
    }

    #[test]
    fn query_sort_asc_and_desc() {
        let mut h = handler();
        make_entity_with_data(&mut h, "items", "i-1", json!({"priority": 3}));
        make_entity_with_data(&mut h, "items", "i-2", json!({"priority": 1}));
        make_entity_with_data(&mut h, "items", "i-3", json!({"priority": 2}));

        // Sort ascending
        let asc = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![SortField {
                    field: "priority".into(),
                    direction: SortDirection::Asc,
                }],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        let priorities_asc: Vec<i64> = asc
            .entities
            .iter()
            .map(|e| e.data["priority"].as_i64().unwrap())
            .collect();
        assert_eq!(priorities_asc, vec![1, 2, 3]);

        // Sort descending
        let desc = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![SortField {
                    field: "priority".into(),
                    direction: SortDirection::Desc,
                }],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        let priorities_desc: Vec<i64> = desc
            .entities
            .iter()
            .map(|e| e.data["priority"].as_i64().unwrap())
            .collect();
        assert_eq!(priorities_desc, vec![3, 2, 1]);
    }

    #[test]
    fn query_cursor_pagination() {
        let mut h = handler();
        // Insert 5 entities in a predictable order.
        for i in 1..=5 {
            make_entity_with_data(&mut h, "items", &format!("i-{i:03}"), json!({"n": i}));
        }

        // Page 1: limit=2, no cursor → returns i-001, i-002; next_cursor = "i-002"
        let page1 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![],
                limit: Some(2),
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(page1.entities.len(), 2);
        assert_eq!(page1.total_count, 5);
        assert!(page1.next_cursor.is_some());

        // Page 2: pick up after cursor from page 1.
        let cursor_id = EntityId::new(page1.next_cursor.as_deref().unwrap());
        let page2 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![],
                limit: Some(2),
                after_id: Some(cursor_id),
                count_only: false,
            })
            .unwrap();
        assert_eq!(page2.entities.len(), 2);

        // Last page: no further results.
        let cursor_id2 = EntityId::new(page2.next_cursor.as_deref().unwrap());
        let page3 = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("items"),
                filter: None,
                sort: vec![],
                limit: Some(2),
                after_id: Some(cursor_id2),
                count_only: false,
            })
            .unwrap();
        assert_eq!(page3.entities.len(), 1);
        assert!(page3.next_cursor.is_none());
    }

    #[test]
    fn query_cursor_invalid_after_id_returns_error() {
        let mut h = handler();
        for i in 1..=3 {
            make_entity_with_data(&mut h, "items", &format!("i-{i:03}"), json!({"n": i}));
        }

        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: None,
            sort: vec![],
            limit: None,
            after_id: Some(EntityId::new("nonexistent-id")),
            count_only: false,
        });

        assert!(
            matches!(result, Err(AxonError::InvalidArgument(_))),
            "expected InvalidArgument for unknown cursor, got {result:?}"
        );
    }

    #[test]
    fn query_count_only() {
        let mut h = handler();
        make_entity_with_data(&mut h, "tasks", "t-1", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-2", json!({"status": "open"}));
        make_entity_with_data(&mut h, "tasks", "t-3", json!({"status": "done"}));

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("open"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: true,
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        assert!(resp.entities.is_empty());
    }

    #[test]
    fn query_dot_path_field() {
        let mut h = handler();
        make_entity_with_data(
            &mut h,
            "contacts",
            "c-1",
            json!({"address": {"city": "Berlin"}}),
        );
        make_entity_with_data(
            &mut h,
            "contacts",
            "c-2",
            json!({"address": {"city": "Paris"}}),
        );

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("contacts"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "address.city".into(),
                    op: FilterOp::Eq,
                    value: json!("Berlin"),
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].data["address"]["city"], "Berlin");
    }

    // ── FilterNode depth limit tests ──────────────────────────────────────────

    /// Build a left-spine And tree of the given depth.
    fn nested_and(depth: usize) -> FilterNode {
        let leaf = FilterNode::Field(FieldFilter {
            field: "x".into(),
            op: FilterOp::Eq,
            value: json!(1),
        });
        if depth <= 1 {
            return leaf;
        }
        FilterNode::And {
            filters: vec![nested_and(depth - 1)],
        }
    }

    #[test]
    fn filter_depth_at_max_succeeds() {
        let mut h = handler();
        make_entity_with_data(&mut h, "items", "i-1", json!({"x": 1}));

        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: Some(nested_and(MAX_FILTER_DEPTH)),
            sort: vec![],
            limit: None,
            after_id: None,
            count_only: false,
        });

        assert!(result.is_ok(), "filter at max depth should succeed");
    }

    #[test]
    fn filter_depth_exceeds_max_returns_invalid_argument() {
        let mut h = handler();
        make_entity_with_data(&mut h, "items", "i-1", json!({"x": 1}));

        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: Some(nested_and(MAX_FILTER_DEPTH + 1)),
            sort: vec![],
            limit: None,
            after_id: None,
            count_only: false,
        });

        match result {
            Err(AxonError::InvalidArgument(msg)) => {
                assert!(
                    msg.contains("depth"),
                    "error message should mention depth: {msg}"
                );
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }

    #[test]
    fn filter_deeply_nested_no_stack_overflow() {
        // 1000 levels deep — well beyond MAX_FILTER_DEPTH but should not
        // overflow the stack; it must return InvalidArgument instead.
        let h = handler();
        let result = h.query_entities(QueryEntitiesRequest {
            collection: CollectionId::new("items"),
            filter: Some(nested_and(1000)),
            sort: vec![],
            limit: None,
            after_id: None,
            count_only: false,
        });

        assert!(
            matches!(result, Err(AxonError::InvalidArgument(_))),
            "deeply nested filter must return InvalidArgument, not stack overflow"
        );
    }

    #[test]
    fn filter_depth_iterative_100k_deep_no_stack_overflow() {
        // Build a linear chain of depth 100_000. The old recursive implementation
        // would overflow the stack; the iterative implementation must not.
        let leaf = FilterNode::Field(FieldFilter {
            field: "x".to_string(),
            op: FilterOp::Eq,
            value: serde_json::json!(1),
        });
        let mut node = leaf;
        for _ in 0..99_999 {
            node = FilterNode::And {
                filters: vec![node],
            };
        }
        let depth = filter_depth(&node);
        // Avoid recursive Drop stack overflow on the deep tree; the tree is
        // intentionally leaked here — this is test-only and the process exits anyway.
        std::mem::forget(node);
        assert_eq!(
            depth, 100_000,
            "iterative filter_depth must return exact depth for deep tree"
        );
    }

    // ── Schema persistence ───────────────────────────────────────────────────

    #[test]
    fn put_schema_then_get_schema_roundtrip() {
        let mut h = handler();
        let col = CollectionId::new("invoices");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: Some("Invoice collection".into()),
            version: 1,
            entity_schema: Some(json!({"type": "object"})),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        h.put_schema(schema.clone()).unwrap();

        let retrieved = h
            .get_schema(&col)
            .unwrap()
            .expect("schema should be retrievable after put_schema");
        assert_eq!(retrieved.collection, col);
        assert_eq!(retrieved.version, 1);
        assert_eq!(retrieved.description.as_deref(), Some("Invoice collection"));
    }

    #[test]
    fn get_schema_missing_returns_none() {
        let h = handler();
        let result = h.get_schema(&CollectionId::new("nonexistent")).unwrap();
        assert!(result.is_none(), "missing schema should return None");
    }

    #[test]
    fn handle_get_schema_missing_returns_not_found() {
        let h = handler();
        let err = h
            .handle_get_schema(GetSchemaRequest {
                collection: CollectionId::new("nope"),
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }

    #[test]
    fn handle_put_schema_creates_audit_entry() {
        let mut h = handler();
        let col = CollectionId::new("invoices");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        h.handle_put_schema(PutSchemaRequest {
            schema,
            actor: Some("alice".into()),
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        let entries = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::SchemaUpdate)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].collection, col);
        assert_eq!(entries[0].actor, "alice");
    }

    fn named_query_schema(collection: &str, cypher: &str) -> CollectionSchema {
        let mut schema = CollectionSchema::new(CollectionId::new(collection));
        schema.entity_schema = Some(json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "priority": { "type": "integer" }
            }
        }));
        schema.indexes = vec![IndexDef {
            field: "status".into(),
            index_type: IndexType::String,
            unique: false,
        }];
        schema.queries.insert(
            "ready_beads".into(),
            NamedQueryDef {
                description: "Open ready beads".into(),
                cypher: cypher.into(),
                parameters: Vec::new(),
            },
        );
        schema
    }

    #[test]
    fn handle_put_schema_valid_named_query_activates_and_introspects() {
        let mut h = handler();
        let col = CollectionId::new("ddx_beads");
        let schema = named_query_schema(
            col.as_str(),
            "MATCH (b:DdxBead {status: 'open'}) RETURN b ORDER BY b.status",
        );

        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: Some("alice".into()),
                force: false,
                dry_run: false,
                explain_inputs: Vec::new(),
            })
            .expect("valid named query should activate");

        let report = resp.compile_report.expect("query compile report present");
        assert!(report.is_success(), "{report:?}");
        let stored = h
            .handle_get_schema(GetSchemaRequest {
                collection: col.clone(),
            })
            .expect("schema should be active")
            .schema;
        assert!(stored.queries.contains_key("ready_beads"));
        assert_eq!(
            stored.queries["ready_beads"].description,
            "Open ready beads"
        );
    }

    #[test]
    fn handle_put_schema_dry_run_reports_named_query_errors_without_activation() {
        let mut h = handler();
        let col = CollectionId::new("ddx_beads");
        let schema = named_query_schema(col.as_str(), "MATCH (b:DdxBead RETURN b");

        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: Some("alice".into()),
                force: false,
                dry_run: true,
                explain_inputs: Vec::new(),
            })
            .expect("dry-run should return diagnostics instead of activating");

        assert!(resp.dry_run);
        let report = resp.compile_report.expect("query compile report present");
        assert_eq!(report.queries.len(), 1);
        assert_eq!(
            report.queries[0].status,
            axon_schema::NamedQueryStatus::ParseError
        );
        assert!(
            h.get_schema(&col).unwrap().is_none(),
            "dry-run must not activate the schema"
        );
    }

    #[test]
    fn handle_put_schema_rejects_named_query_compile_errors() {
        let mut h = handler();
        let col = CollectionId::new("ddx_beads");
        let schema = named_query_schema(col.as_str(), "MATCH (b:Missing) RETURN b");

        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: Some("alice".into()),
                force: false,
                dry_run: false,
                explain_inputs: Vec::new(),
            })
            .expect_err("activation must reject invalid named query");

        assert!(matches!(
            err,
            AxonError::SchemaValidation(ref msg) if msg.starts_with("query_compile_failed")
        ));
        assert!(h.get_schema(&col).unwrap().is_none());
    }

    #[test]
    fn handle_put_schema_dry_run_returns_compile_errors_in_report() {
        let mut h = handler();
        let col = CollectionId::new("policy_dry_run");

        // Seed v1 with no access_control so we have a baseline collection.
        let baseline = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "title": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: baseline,
            actor: Some("setup".into()),
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Dry-run with a broken policy must surface a structured diagnostic
        // rather than bubbling AxonError::SchemaValidation.
        let proposed = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "title": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [{
                            "name": "broken",
                            "where": { "field": "missing_field", "eq": "x" }
                        }]
                    }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: proposed,
                actor: Some("alice".into()),
                force: false,
                dry_run: true,
                explain_inputs: Vec::new(),
            })
            .expect("dry-run with invalid policy must not error");
        assert!(resp.dry_run);
        let report = resp.policy_compile_report.expect("report present");
        assert_eq!(
            report.errors.len(),
            1,
            "expected one compile diagnostic, got {report:?}"
        );
        let diag = &report.errors[0];
        assert_eq!(diag.code, axon_schema::POLICY_COMPILE_ERROR_DEFAULT_CODE);
        assert!(diag.message.contains("missing_field"));
        assert_eq!(diag.collection.as_deref(), Some("policy_dry_run"));
        assert_eq!(diag.field.as_deref(), Some("missing_field"));
        assert!(diag.path.is_some());

        // Persisted schema is still v1: dry-run never wrote.
        let retrieved = h.get_schema(&col).unwrap().expect("schema present");
        assert_eq!(retrieved.version, 1);
    }

    #[test]
    fn handle_put_schema_dry_run_no_policy_returns_empty_explanations() {
        let mut h = handler();
        let col = CollectionId::new("policy_no_ac");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "title": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: None,
                force: false,
                dry_run: true,
                explain_inputs: vec![ExplainPolicyRequest {
                    operation: "read".into(),
                    collection: Some(col),
                    entity_id: None,
                    expected_version: None,
                    data: Some(json!({"title": "hi"})),
                    patch: None,
                    lifecycle_name: None,
                    target_state: None,
                    to_version: None,
                    operations: Vec::new(),
                    actor_override: None,
                }],
            })
            .expect("dry-run with explain inputs but no access_control must succeed");
        let explanations = resp
            .dry_run_explanations
            .expect("explanations should be Some(vec![]) when no policy");
        assert!(explanations.is_empty());
    }

    #[test]
    fn handle_put_schema_dry_run_explain_inputs_use_proposed_plan() {
        let mut h = handler();
        let col = CollectionId::new("policy_diff");

        let active = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "status": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [{
                            "name": "only-open",
                            "where": { "field": "status", "eq": "open" }
                        }]
                    }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: active,
            actor: Some("setup".into()),
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        let active_resp = h
            .explain_policy_with_caller(
                ExplainPolicyRequest {
                    operation: "read".into(),
                    collection: Some(col.clone()),
                    entity_id: None,
                    expected_version: None,
                    data: Some(json!({"status": "archived"})),
                    patch: None,
                    lifecycle_name: None,
                    target_state: None,
                    to_version: None,
                    operations: Vec::new(),
                    actor_override: None,
                },
                &axon_core::auth::CallerIdentity::new("admin", axon_core::auth::Role::Admin),
                None,
            )
            .unwrap();
        assert_eq!(active_resp.decision, "deny");

        let proposed = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "status": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [
                            { "name": "only-open", "where": { "field": "status", "eq": "open" } },
                            { "name": "also-archived", "where": { "field": "status", "eq": "archived" } }
                        ]
                    }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: proposed,
                actor: Some("alice".into()),
                force: false,
                dry_run: true,
                explain_inputs: vec![ExplainPolicyRequest {
                    operation: "read".into(),
                    collection: Some(col.clone()),
                    entity_id: None,
                    expected_version: None,
                    data: Some(json!({"status": "archived"})),
                    patch: None,
                    lifecycle_name: None,
                    target_state: None,
                    to_version: None,
                    operations: Vec::new(),
                    actor_override: None,
                }],
            })
            .expect("valid proposed policy must dry-run successfully");
        let explanations = resp
            .dry_run_explanations
            .expect("dry-run should populate explanations on success");
        assert_eq!(explanations.len(), 1);
        assert_eq!(
            explanations[0].decision, "allow",
            "proposed plan must override active deny"
        );
        let stored = h.get_schema(&col).unwrap().expect("schema present");
        assert_eq!(stored.version, 1);
    }

    #[test]
    fn handle_put_schema_dry_run_rejects_cross_collection_explain_input() {
        let mut h = handler();
        let col = CollectionId::new("policy_root");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "title": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": { "allow": [{ "name": "any" }] }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: None,
                force: false,
                dry_run: true,
                explain_inputs: vec![ExplainPolicyRequest {
                    operation: "read".into(),
                    collection: Some(CollectionId::new("other")),
                    entity_id: None,
                    expected_version: None,
                    data: Some(json!({})),
                    patch: None,
                    lifecycle_name: None,
                    target_state: None,
                    to_version: None,
                    operations: Vec::new(),
                    actor_override: None,
                }],
            })
            .expect_err("cross-collection explain input must error");
        assert!(matches!(err, AxonError::InvalidArgument(_)));
    }

    /// Empty explain-policy request with `operation` set; centralizes the
    /// boilerplate so the dry-run tests in this module stay focused on the
    /// fixture difference under test.
    fn empty_explain_request(operation: &str) -> ExplainPolicyRequest {
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
            actor_override: None,
        }
    }

    /// Schema whose proposed read policy gates on `subject.tenant_role`. Used
    /// by the actor-override dry-run test so a single proposed plan can decide
    /// "allow" or "deny" depending on which synthetic caller the override
    /// supplies.
    fn subject_role_gated_schema(col: &CollectionId) -> CollectionSchema {
        CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "title": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [{
                            "name": "admins-only",
                            "where": { "subject": "tenant_role", "eq": "admin" }
                        }]
                    }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        }
    }

    #[test]
    fn put_schema_dry_run_actor_override_drives_synthetic_caller() {
        let mut h = handler();
        let col = CollectionId::new("policy_actor_override");
        let proposed = subject_role_gated_schema(&col);

        // Override -> Role::Admin: subject-aware predicate matches and the
        // proposed plan allows.
        let admin_resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: proposed.clone(),
                actor: Some("schema-writer".into()),
                force: false,
                dry_run: true,
                explain_inputs: vec![ExplainPolicyRequest {
                    actor_override: Some(ExplainActorOverride {
                        actor: Some("alice".into()),
                        role: Some("admin".into()),
                        subject: HashMap::new(),
                    }),
                    data: Some(json!({"title": "hi"})),
                    ..empty_explain_request("read")
                }],
            })
            .expect("admin override dry-run");
        let admin_explanations = admin_resp
            .dry_run_explanations
            .expect("explanations should be Some on success");
        assert_eq!(admin_explanations.len(), 1);
        assert_eq!(admin_explanations[0].decision, "allow");

        // Override -> Role::Read: same proposed policy, but the override flips
        // the subject role binding so the predicate fails.
        let reader_resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: proposed,
                actor: Some("schema-writer".into()),
                force: false,
                dry_run: true,
                explain_inputs: vec![ExplainPolicyRequest {
                    actor_override: Some(ExplainActorOverride {
                        actor: Some("bob".into()),
                        role: Some("read".into()),
                        subject: HashMap::new(),
                    }),
                    data: Some(json!({"title": "hi"})),
                    ..empty_explain_request("read")
                }],
            })
            .expect("reader override dry-run");
        let reader_explanations = reader_resp
            .dry_run_explanations
            .expect("explanations should be Some on success");
        assert_eq!(reader_explanations.len(), 1);
        assert_eq!(reader_explanations[0].decision, "deny");
    }

    #[test]
    fn put_schema_dry_run_self_target_policy_uses_proposed_plan() {
        let mut h = handler();
        let col = CollectionId::new("policy_self_target");

        // Active plan: only owner=bob may read or update. Self-link declared
        // so the proposed plan can attach a `target_policy: update` recursion
        // that lands back on this same collection. (Direct read→read self
        // recursion is rejected by the compile cycle check; read→update is
        // permitted and still exercises the proposed-catalog resolver because
        // the recursion target collection equals the previewed root.)
        let mut active = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "owner": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [{ "name": "owner-bob", "where": { "field": "owner", "eq": "bob" } }]
                    },
                    "update": {
                        "allow": [{ "name": "owner-bob", "where": { "field": "owner", "eq": "bob" } }]
                    }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        active.link_types.insert(
            "follows".into(),
            LinkTypeDef {
                target_collection: col.to_string(),
                cardinality: Cardinality::ManyToMany,
                required: false,
                metadata_schema: None,
            },
        );
        h.handle_put_schema(PutSchemaRequest {
            schema: active.clone(),
            actor: Some("setup".into()),
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Seed two entities and a self-link. w_alice matches owner-alice under
        // the proposed plan; w_carol matches no direct rule but follows
        // w_alice, so the self-target_policy recursion should make it readable
        // when the proposed plan governs that recursion.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("w_alice"),
            data: json!({"_id": "w_alice", "owner": "alice"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("w_carol"),
            data: json!({"_id": "w_carol", "owner": "carol"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_link(CreateLinkRequest {
            source_collection: col.clone(),
            source_id: EntityId::new("w_carol"),
            target_collection: col.clone(),
            target_id: EntityId::new("w_alice"),
            link_type: "follows".into(),
            metadata: json!({}),
            actor: None,
            attribution: None,
        })
        .unwrap();

        // Proposed plan: read = owner-alice OR follows-updateable (self
        // target_policy = update); update = owner-alice. If the recursion
        // consulted the active stored plan, w_alice (owner=alice) would not
        // match the active update rule (which only allows owner=bob), the
        // related-target_policy clause would fail, and w_carol would deny.
        // With the proposed catalog driving recursion, the recursive update
        // for w_alice matches owner-alice and w_carol is allowed.
        let proposed = CollectionSchema {
            version: 2,
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [
                            { "name": "owner-alice", "where": { "field": "owner", "eq": "alice" } },
                            {
                                "name": "follows-updateable",
                                "where": {
                                    "related": {
                                        "link_type": "follows",
                                        "direction": "outgoing",
                                        "target_collection": col.as_str(),
                                        "target_policy": "update"
                                    }
                                }
                            }
                        ]
                    },
                    "update": {
                        "allow": [{ "name": "owner-alice", "where": { "field": "owner", "eq": "alice" } }]
                    }
                }))
                .unwrap(),
            ),
            ..active.clone()
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: proposed,
                actor: Some("admin".into()),
                force: false,
                dry_run: true,
                explain_inputs: vec![ExplainPolicyRequest {
                    entity_id: Some(EntityId::new("w_carol")),
                    ..empty_explain_request("read")
                }],
            })
            .expect("self-referential dry-run must succeed");
        let explanations = resp.dry_run_explanations.unwrap_or_else(|| {
            let report = resp.policy_compile_report.as_ref().unwrap();
            panic!("expected compile to succeed; errors: {:?}", report.errors)
        });
        assert_eq!(explanations.len(), 1);
        assert_eq!(
            explanations[0].decision, "allow",
            "self-referential target_policy must consult the proposed plan, not storage"
        );
    }

    #[test]
    fn put_schema_dry_run_transaction_threads_proposed_plan_into_children() {
        let mut h = handler();
        let col = CollectionId::new("policy_tx_dry_run");

        let active = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "status": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [{
                            "name": "only-open",
                            "where": { "field": "status", "eq": "open" }
                        }]
                    }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: active.clone(),
            actor: Some("setup".into()),
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Proposed plan: read also allows status="archived". Run a transaction
        // explain whose only child is a read of an archived record. Under the
        // active plan that read denies (status=archived doesn't match
        // only-open). Under the proposed plan it allows. Transaction recursion
        // must thread the proposed plan into the child explanation.
        let proposed = CollectionSchema {
            version: 2,
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [
                            { "name": "only-open", "where": { "field": "status", "eq": "open" } },
                            { "name": "also-archived", "where": { "field": "status", "eq": "archived" } }
                        ]
                    }
                }))
                .unwrap(),
            ),
            ..active.clone()
        };
        let child = ExplainPolicyRequest {
            data: Some(json!({"status": "archived"})),
            ..empty_explain_request("read")
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: proposed,
                actor: Some("admin".into()),
                force: false,
                dry_run: true,
                explain_inputs: vec![ExplainPolicyRequest {
                    operations: vec![child],
                    ..empty_explain_request("transaction")
                }],
            })
            .expect("transaction dry-run must succeed");
        let explanations = resp
            .dry_run_explanations
            .expect("transaction explanations on success");
        assert_eq!(explanations.len(), 1);
        assert_eq!(explanations[0].operation, "transaction");
        assert_eq!(
            explanations[0].decision, "allow",
            "transaction must allow when proposed plan permits the child"
        );
        assert_eq!(explanations[0].operations.len(), 1);
        assert_eq!(
            explanations[0].operations[0].decision, "allow",
            "child read must decide against proposed plan, not storage"
        );
    }

    #[test]
    fn handle_put_schema_blocks_activation_on_compile_errors() {
        let mut h = handler();
        let col = CollectionId::new("policy_gate");

        // Seed v1.
        let baseline = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "title": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: baseline,
            actor: Some("setup".into()),
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();
        let pre_audit_count = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::SchemaUpdate)
            .unwrap()
            .len();

        // Activation with a broken predicate must error and not persist.
        let proposed = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "properties": { "title": { "type": "string" } }
            })),
            link_types: Default::default(),
            access_control: Some(
                serde_json::from_value(json!({
                    "read": {
                        "allow": [{
                            "name": "broken",
                            "where": { "field": "missing_field", "eq": "x" }
                        }]
                    }
                }))
                .unwrap(),
            ),
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema: proposed,
                actor: Some("alice".into()),
                force: false,
                dry_run: false,
                explain_inputs: Vec::new(),
            })
            .expect_err("activation must refuse on compile errors");
        match err {
            AxonError::SchemaValidation(msg) => {
                assert!(
                    msg.starts_with("policy_compile_failed"),
                    "expected policy_compile_failed prefix, got {msg}"
                );
            }
            other => panic!("expected SchemaValidation, got {other:?}"),
        }

        // Persisted schema must still be v1.
        let retrieved = h.get_schema(&col).unwrap().expect("schema present");
        assert_eq!(retrieved.version, 1);

        // No new audit entry from the failed activation.
        let post_audit_count = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::SchemaUpdate)
            .unwrap()
            .len();
        assert_eq!(post_audit_count, pre_audit_count);
    }

    #[test]
    fn put_schema_persists_across_handler_method_calls() {
        // Verify that schema written via put_schema is visible to create_entity validation.
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = EsfDocument::parse(TASK_ESF)
            .unwrap()
            .into_collection_schema()
            .unwrap();

        h.put_schema(schema).unwrap();

        // Invalid entity should be rejected.
        let err = h
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-bad"),
                data: json!({"done": false}), // missing required "title"
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::SchemaValidation(_)));

        // Valid entity should be accepted.
        h.create_entity(CreateEntityRequest {
            collection: col,
            id: EntityId::new("t-good"),
            data: json!({"title": "ok", "done": false}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    #[test]
    fn put_schema_rejects_invalid_entity_schema() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col,
            description: None,
            version: 1,
            entity_schema: Some(json!({"type": "bogus"})),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        let err = h.put_schema(schema).unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );
    }

    #[test]
    fn handle_put_schema_rejects_invalid_json_schema() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({"type": "bogus"})),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema,
                actor: None,
                force: false,
                dry_run: false,
                explain_inputs: Vec::new(),
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation error, got: {err}"
        );
    }

    #[test]
    fn handle_put_schema_accepts_valid_json_schema() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let schema = axon_schema::schema::CollectionSchema {
            collection: col,
            description: None,
            version: 1,
            entity_schema: Some(
                json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            ),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        h.handle_put_schema(PutSchemaRequest {
            schema,
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();
    }

    #[test]
    fn put_schema_breaking_change_rejected_without_force() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": {"type": "string"},
                    "status": {"type": "string", "enum": ["draft", "active"]}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Breaking change: add required field
        let v2 = CollectionSchema {
            collection: col,
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title", "assignee"],
                "properties": {
                    "title": {"type": "string"},
                    "status": {"type": "string", "enum": ["draft", "active"]},
                    "assignee": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let err = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: None,
                force: false,
                dry_run: false,
                explain_inputs: Vec::new(),
            })
            .unwrap_err();
        assert!(
            matches!(err, AxonError::InvalidOperation(_)),
            "breaking change without force should be rejected, got: {err:?}"
        );
    }

    #[test]
    fn put_schema_breaking_change_accepted_with_force() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}}
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Breaking: add required field, with force=true
        let v2 = CollectionSchema {
            collection: col,
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title", "priority"],
                "properties": {
                    "title": {"type": "string"},
                    "priority": {"type": "integer"}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: Some("admin".into()),
                force: true,
                dry_run: false,
                explain_inputs: Vec::new(),
            })
            .unwrap();
        assert_eq!(
            resp.compatibility,
            Some(axon_schema::Compatibility::Breaking)
        );
        assert!(!resp.dry_run);
    }

    #[test]
    fn put_schema_dry_run_does_not_apply() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}}
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Dry-run breaking change
        let v2 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title", "owner"],
                "properties": {
                    "title": {"type": "string"},
                    "owner": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: None,
                force: false,
                dry_run: true,
                explain_inputs: Vec::new(),
            })
            .unwrap();
        assert!(resp.dry_run);
        assert_eq!(
            resp.compatibility,
            Some(axon_schema::Compatibility::Breaking)
        );

        // Schema should still be v1
        let stored = h.get_schema(&col).unwrap().unwrap();
        assert_eq!(stored.version, 1);
    }

    #[test]
    fn put_schema_compatible_change_succeeds_without_force() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let v1 = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}}
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v1,
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Compatible: add optional field
        let v2 = CollectionSchema {
            collection: col,
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": {"type": "string"},
                    "tags": {"type": "array", "items": {"type": "string"}}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        let resp = h
            .handle_put_schema(PutSchemaRequest {
                schema: v2,
                actor: None,
                force: false,
                dry_run: false,
                explain_inputs: Vec::new(),
            })
            .unwrap();
        assert_eq!(
            resp.compatibility,
            Some(axon_schema::Compatibility::Compatible)
        );
        assert!(!resp.dry_run);
    }

    #[test]
    fn drop_collection_removes_schema() {
        let mut h = handler();
        let col = CollectionId::new("invoices");

        // Explicit collection create so drop_collection can find it.
        // Schema version 1 is persisted as part of create_collection.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();
        assert!(h.get_schema(&col).unwrap().is_some());

        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
            confirm: true,
        })
        .unwrap();

        assert!(
            h.get_schema(&col).unwrap().is_none(),
            "schema must be removed when collection is dropped"
        );
    }

    #[test]
    fn drop_collection_removes_collection_view_and_resets_version_on_recreate() {
        let mut h = handler();
        let col = CollectionId::new("notes");

        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let initial_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(col.clone(), "# {{title}}"))
            .unwrap();
        assert_eq!(initial_view.version, 1);

        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
            confirm: true,
        })
        .unwrap();

        assert!(
            h.storage.get_collection_view(&col).unwrap().is_none(),
            "collection view must be removed when collection is dropped"
        );

        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();
        assert!(
            h.storage.get_collection_view(&col).unwrap().is_none(),
            "recreated collections must not inherit a prior collection view"
        );

        let recreated_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(col.clone(), "# {{summary}}"))
            .unwrap();
        assert_eq!(recreated_view.version, 1);
        assert_eq!(recreated_view.markdown_template, "# {{summary}}");
    }

    // ── Validation gate integration tests (US-067) ──────────────────────

    fn handler_with_gated_schema() -> AxonHandler<MemoryStorageAdapter> {
        use axon_schema::rules::{
            ConditionOp, RequirementOp, RuleCondition, RuleRequirement, ValidationRule,
        };
        use axon_schema::schema::GateDef;
        use std::collections::HashMap;

        let mut h = handler();
        let col = CollectionId::new("items");

        // Create collection first.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        // Schema with save, complete, review gates and advisory.
        let schema = CollectionSchema {
            collection: col,
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            access_control: None,
            gates: HashMap::from([
                (
                    "complete".into(),
                    GateDef {
                        description: Some("Ready for processing".into()),
                        includes: vec![],
                    },
                ),
                (
                    "review".into(),
                    GateDef {
                        description: Some("Ready for review".into()),
                        includes: vec!["complete".into()],
                    },
                ),
            ]),
            validation_rules: vec![
                // Save gate: bead_type required.
                ValidationRule {
                    name: "need-type".into(),
                    gate: Some("save".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "bead_type".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "bead_type is required".into(),
                    fix: Some("Set bead_type".into()),
                },
                // Complete gate: description required.
                ValidationRule {
                    name: "need-desc".into(),
                    gate: Some("complete".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "description".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Description required for completion".into(),
                    fix: Some("Add a description".into()),
                },
                // Complete gate: conditional - bugs need priority.
                ValidationRule {
                    name: "bugs-need-priority".into(),
                    gate: Some("complete".into()),
                    advisory: false,
                    when: Some(RuleCondition::Field {
                        field: "bead_type".into(),
                        op: ConditionOp::Eq(serde_json::json!("bug")),
                    }),
                    require: RuleRequirement {
                        field: "priority".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Bugs must have priority".into(),
                    fix: Some("Set priority (0-4)".into()),
                },
                // Review gate: acceptance required.
                ValidationRule {
                    name: "need-acceptance".into(),
                    gate: Some("review".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "acceptance".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Acceptance criteria required for review".into(),
                    fix: Some("Add acceptance criteria".into()),
                },
                // Advisory: recommend tags.
                ValidationRule {
                    name: "recommend-tags".into(),
                    gate: None,
                    advisory: true,
                    when: None,
                    require: RuleRequirement {
                        field: "tags".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "Consider adding tags".into(),
                    fix: Some("Add tags for categorization".into()),
                },
            ],
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.put_schema(schema).unwrap();
        h
    }

    #[test]
    fn save_gate_blocks_create() {
        let mut h = handler_with_gated_schema();
        // Missing bead_type → save gate blocks.
        let result = h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("items"),
            id: EntityId::new("g-1"),
            data: json!({"title": "Test"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        });
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("save gate failed"), "got: {err}");
        assert!(err.contains("bead_type is required"), "got: {err}");
    }

    #[test]
    fn save_gate_rejects_privileged_role_on_browser_writable_create() {
        use axon_schema::rules::{RequirementOp, RuleRequirement, ValidationRule};

        let mut h = handler();
        let col = CollectionId::new("users");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: Some("Browser bootstrap users".into()),
                version: 1,
                entity_schema: Some(json!({
                    "type": "object",
                    "required": ["display_name", "role"],
                    "properties": {
                        "display_name": { "type": "string" },
                        "role": {
                            "type": "string",
                            "enum": ["member", "admin"]
                        }
                    }
                })),
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: vec![ValidationRule {
                    name: "browser-bootstrap-role-member-only".into(),
                    gate: Some("save".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "role".into(),
                        op: RequirementOp::In(vec![json!("member")]),
                    },
                    message: "Browser bootstrap users must start as member".into(),
                    fix: Some(
                        "Set role to member; create admin users through a privileged path".into(),
                    ),
                }],
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: Some("schema-admin".into()),
        })
        .unwrap();

        let rejected = h
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new("u-admin"),
                data: json!({
                    "display_name": "Mallory",
                    "role": "admin"
                }),
                actor: Some("browser:self-service-bootstrap".into()),
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();
        assert!(matches!(rejected, AxonError::SchemaValidation(_)));
        assert!(rejected
            .to_string()
            .contains("Browser bootstrap users must start as member"));

        let allowed = h
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new("u-member"),
                data: json!({
                    "display_name": "Ada",
                    "role": "member"
                }),
                actor: Some("browser:self-service-bootstrap".into()),
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        assert_eq!(allowed.entity.data["role"], "member");
        assert_eq!(
            allowed.entity.created_by.as_deref(),
            Some("browser:self-service-bootstrap")
        );
    }

    #[test]
    fn custom_gate_allows_save_reports_failures() {
        let mut h = handler_with_gated_schema();
        // Has bead_type (save passes) but missing description (complete gate fails).
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-2"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        // Entity was saved.
        assert_eq!(resp.entity.data["bead_type"], "task");

        // Complete gate fails.
        let complete = resp.gates.get("complete").unwrap();
        assert!(!complete.pass);
        assert!(complete.failures.iter().any(|f| f.rule == "need-desc"));

        // Review gate also fails (inherits complete).
        let review = resp.gates.get("review").unwrap();
        assert!(!review.pass);
        assert!(review.failures.iter().any(|f| f.rule == "need-desc"));
        assert!(review.failures.iter().any(|f| f.rule == "need-acceptance"));
    }

    #[test]
    fn advisory_reported_in_response() {
        let mut h = handler_with_gated_schema();
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-3"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.advisories.len(), 1);
        assert_eq!(resp.advisories[0].rule, "recommend-tags");
        assert!(resp.advisories[0].advisory);
    }

    #[test]
    fn all_gates_pass_when_all_fields_present() {
        let mut h = handler_with_gated_schema();
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-4"),
                data: json!({
                    "bead_type": "task",
                    "description": "Something",
                    "acceptance": "Tests pass",
                    "tags": ["core"]
                }),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        // All gates pass.
        for gate in resp.gates.values() {
            assert!(gate.pass, "gate {} should pass", gate.gate);
        }
        // No advisories.
        assert!(resp.advisories.is_empty());
    }

    #[test]
    fn save_gate_blocks_update() {
        let mut h = handler_with_gated_schema();
        // Create with valid data.
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-5"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        // Update removing bead_type → save gate blocks.
        let result = h.update_entity(UpdateEntityRequest {
            collection: CollectionId::new("items"),
            id: EntityId::new("g-5"),
            data: json!({"title": "Updated"}),
            expected_version: resp.entity.version,
            actor: None,
            audit_metadata: None,
            attribution: None,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("save gate failed"));
    }

    #[test]
    fn update_reports_gate_status() {
        let mut h = handler_with_gated_schema();
        let create_resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-6"),
                data: json!({"bead_type": "bug"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        // Update with description but no priority (bug needs priority for complete gate).
        let resp = h
            .update_entity(UpdateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-6"),
                data: json!({
                    "bead_type": "bug",
                    "description": "A bug"
                }),
                expected_version: create_resp.entity.version,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        let complete = resp.gates.get("complete").unwrap();
        assert!(!complete.pass);
        assert!(complete
            .failures
            .iter()
            .any(|f| f.rule == "bugs-need-priority"));
    }

    #[test]
    fn gate_inclusion_review_inherits_complete_failures() {
        let mut h = handler_with_gated_schema();
        let resp = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("items"),
                id: EntityId::new("g-7"),
                data: json!({"bead_type": "task"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        // Review gate should contain complete-gate failures too.
        let review = resp.gates.get("review").unwrap();
        let failure_rules: Vec<&str> = review.failures.iter().map(|f| f.rule.as_str()).collect();
        assert!(
            failure_rules.contains(&"need-desc"),
            "review should inherit complete's need-desc failure"
        );
        assert!(
            failure_rules.contains(&"need-acceptance"),
            "review should have its own need-acceptance failure"
        );
    }

    #[test]
    fn gate_definitions_registered_on_schema_save() {
        let h = handler_with_gated_schema();
        let schema = h.get_schema(&CollectionId::new("items")).unwrap().unwrap();
        assert!(schema.gates.contains_key("complete"));
        assert!(schema.gates.contains_key("review"));
        assert_eq!(schema.gates["review"].includes, vec!["complete"]);
    }

    /// FEAT-019: gate results live on the entity blob; after a write the
    /// persisted Entity must carry the materialized gate verdicts so a
    /// subsequent read returns them without a storage side-table lookup.
    #[test]
    fn entity_write_persists_gate_results_via_storage_roundtrip() {
        let mut h = handler_with_gated_schema();
        let col = CollectionId::new("items");
        let id = EntityId::new("rt-1");

        // Create a bug with no description/priority — the "complete" gate
        // should fail on creation and the verdict should be stored on the
        // entity.
        let create_resp = h
            .create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: id.clone(),
                data: json!({"bead_type": "bug"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        assert!(!create_resp.gates.get("complete").unwrap().pass);

        let stored = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: id.clone(),
            })
            .unwrap()
            .entity;
        let complete_on_entity = stored
            .gate_results
            .get("complete")
            .expect("complete gate result should be persisted on the entity blob");
        assert!(!complete_on_entity.pass);
        assert!(complete_on_entity
            .failures
            .iter()
            .any(|f| f.rule == "need-desc" || f.rule == "bugs-need-priority"));

        // Patch the entity to satisfy all the complete-gate rules; the
        // updated verdict must flow onto the stored blob.
        h.patch_entity(PatchEntityRequest {
            collection: col.clone(),
            id: id.clone(),
            patch: json!({"description": "fix it", "priority": "high"}),
            expected_version: stored.version,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let after_patch = h
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: id.clone(),
            })
            .unwrap()
            .entity;
        let complete_after = after_patch
            .gate_results
            .get("complete")
            .expect("complete gate result should still be persisted after patch");
        assert!(
            complete_after.pass,
            "complete gate should pass after patch supplies description and priority"
        );
        assert!(complete_after.failures.is_empty());
    }

    // ── Aggregation tests (US-062) ──────────────────────────────────────

    fn handler_with_entities() -> AxonHandler<MemoryStorageAdapter> {
        let mut h = handler();
        let col = CollectionId::new("beads");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        // Create entities with various statuses and types.
        let items = vec![
            json!({"bead_type": "task", "status": "draft"}),
            json!({"bead_type": "task", "status": "draft"}),
            json!({"bead_type": "task", "status": "pending"}),
            json!({"bead_type": "bug", "status": "pending"}),
            json!({"bead_type": "bug", "status": "done"}),
            json!({"bead_type": "epic"}), // missing status
        ];
        for (i, data) in items.into_iter().enumerate() {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("b-{i}")),
                data,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }
        h
    }

    #[test]
    fn count_without_group_by_returns_total() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.total_count, 6);
        assert!(resp.groups.is_empty());
    }

    #[test]
    fn count_group_by_status() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: None,
                group_by: Some("status".into()),
            })
            .unwrap();
        assert_eq!(resp.total_count, 6);

        // Should have groups for draft, pending, done, and null (missing status).
        assert!(!resp.groups.is_empty());

        let draft_count = resp
            .groups
            .iter()
            .find(|g| g.key == json!("draft"))
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(draft_count, 2);

        let pending_count = resp
            .groups
            .iter()
            .find(|g| g.key == json!("pending"))
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(pending_count, 2);

        let done_count = resp
            .groups
            .iter()
            .find(|g| g.key == json!("done"))
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(done_count, 1);

        // Null group for the entity missing status.
        let null_count = resp
            .groups
            .iter()
            .find(|g| g.key.is_null())
            .map(|g| g.count)
            .unwrap_or(0);
        assert_eq!(null_count, 1);
    }

    #[test]
    fn count_with_filter() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "bead_type".into(),
                    op: FilterOp::Eq,
                    value: json!("task"),
                })),
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.total_count, 3);
    }

    #[test]
    fn count_with_filter_and_group_by() {
        let h = handler_with_entities();
        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: CollectionId::new("beads"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "bead_type".into(),
                    op: FilterOp::Eq,
                    value: json!("task"),
                })),
                group_by: Some("status".into()),
            })
            .unwrap();
        assert_eq!(resp.total_count, 3);

        let draft = resp.groups.iter().find(|g| g.key == json!("draft"));
        assert_eq!(draft.unwrap().count, 2);

        let pending = resp.groups.iter().find(|g| g.key == json!("pending"));
        assert_eq!(pending.unwrap().count, 1);
    }

    #[test]
    fn count_empty_collection() {
        let mut h = handler();
        let col = CollectionId::new("empty");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let resp = h
            .count_entities(CountEntitiesRequest {
                collection: col,
                filter: None,
                group_by: Some("status".into()),
            })
            .unwrap();
        assert_eq!(resp.total_count, 0);
        assert!(resp.groups.is_empty());
    }

    // ── Namespace management tests (US-036) ───────────────────────────────

    #[test]
    fn create_namespace() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        let resp = h
            .create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
            })
            .unwrap();
        assert_eq!(resp.database, "prod");
        assert_eq!(resp.schema, "billing");
    }

    #[test]
    fn create_duplicate_namespace_fails() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        let result = h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn list_namespace_collections_empty() {
        use crate::request::{
            CreateDatabaseRequest, CreateNamespaceRequest, ListNamespaceCollectionsRequest,
        };
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        let resp = h
            .list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "billing".into(),
            })
            .unwrap();
        assert!(resp.collections.is_empty());
    }

    #[test]
    fn drop_empty_namespace() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        let resp = h
            .drop_namespace(DropNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
                force: false,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 0);
    }

    #[test]
    fn drop_nonempty_namespace_without_force_fails() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        use axon_core::id::{CollectionId, Namespace};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("invoices"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();

        let result = h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: false,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invoices"));
    }

    #[test]
    fn drop_nonempty_namespace_with_force() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        use axon_core::id::{CollectionId, Namespace};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("invoices"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("receipts"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();

        let resp = h
            .drop_namespace(DropNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 2);
    }

    #[test]
    fn create_collection_in_default_namespace_allows_same_name_elsewhere() {
        use crate::request::{
            CreateDatabaseRequest, CreateNamespaceRequest, ListNamespaceCollectionsRequest,
        };
        use axon_core::id::{CollectionId, Namespace};

        let mut h = handler();
        let invoices = CollectionId::new("invoices");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&invoices, &Namespace::new("prod", "billing"))
            .unwrap();

        h.create_collection(CreateCollectionRequest {
            name: invoices.clone(),
            schema: CollectionSchema::new(invoices.clone()),
            actor: None,
        })
        .unwrap();

        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "default".into(),
                schema: "default".into(),
            })
            .unwrap()
            .collections,
            vec!["invoices".to_string()]
        );
        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "billing".into(),
            })
            .unwrap()
            .collections,
            vec!["invoices".to_string()]
        );
    }

    #[test]
    fn create_collection_accepts_qualified_name_for_non_default_database() {
        use crate::request::{
            CreateCollectionRequest, CreateDatabaseRequest, ListNamespaceCollectionsRequest,
        };

        let mut h = handler();
        let qualified = CollectionId::new("prod.default.invoices");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();

        h.create_collection(CreateCollectionRequest {
            name: qualified.clone(),
            schema: CollectionSchema::new(qualified),
            actor: None,
        })
        .unwrap();

        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "default".into(),
            })
            .unwrap()
            .collections,
            vec!["invoices".to_string()]
        );
    }

    #[test]
    fn drop_collection_accepts_qualified_name_for_non_default_database() {
        use crate::request::{
            CreateCollectionRequest, CreateDatabaseRequest, DropCollectionRequest,
            ListNamespaceCollectionsRequest,
        };

        let mut h = handler();
        let qualified = CollectionId::new("prod.default.invoices");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_collection(CreateCollectionRequest {
            name: qualified.clone(),
            schema: CollectionSchema::new(qualified.clone()),
            actor: None,
        })
        .unwrap();

        h.drop_collection(DropCollectionRequest {
            name: qualified,
            actor: None,
            confirm: true,
        })
        .unwrap();

        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "default".into(),
            })
            .unwrap()
            .collections,
            Vec::<String>::new()
        );
    }

    #[test]
    fn drop_namespace_force_preserves_same_name_in_other_namespace() {
        use crate::request::{
            CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest,
            ListNamespaceCollectionsRequest,
        };
        use axon_core::id::{CollectionId, Namespace};

        let mut h = handler();
        let invoices = CollectionId::new("invoices");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        for schema in ["billing", "engineering"] {
            h.create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: schema.into(),
            })
            .unwrap();
        }

        h.storage_mut()
            .register_collection_in_namespace(&invoices, &Namespace::new("prod", "billing"))
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&invoices, &Namespace::new("prod", "engineering"))
            .unwrap();

        h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: true,
        })
        .unwrap();

        assert_eq!(
            h.list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "prod".into(),
                schema: "engineering".into(),
            })
            .unwrap()
            .collections,
            vec!["invoices".to_string()]
        );
    }

    #[test]
    fn drop_namespace_with_force_records_collection_drop_audits() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropNamespaceRequest};
        use axon_core::id::Namespace;
        use axon_core::types::Entity;

        let mut h = handler();
        let billing = Namespace::new("prod", "billing");
        let invoices = CollectionId::new("invoices");
        let receipts = CollectionId::new("receipts");
        let keep = CollectionId::new("keep");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&invoices, &billing)
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&receipts, &billing)
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&keep, &Namespace::default_ns())
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                invoices.clone(),
                EntityId::new("inv-001"),
                json!({"title": "invoice"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                receipts.clone(),
                EntityId::new("rcpt-001"),
                json!({"title": "receipt"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                json!({"title": "keep"}),
            ))
            .unwrap();

        let resp = h
            .drop_namespace(DropNamespaceRequest {
                database: "prod".into(),
                schema: "billing".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 2);
        assert!(h
            .storage
            .get(&invoices, &EntityId::new("inv-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&receipts, &EntityId::new("rcpt-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&keep, &EntityId::new("keep-001"))
            .unwrap()
            .is_some());

        let drops = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionDrop)
            .unwrap();
        assert_eq!(drops.len(), 2);
        let dropped: std::collections::BTreeSet<_> = drops
            .iter()
            .map(|entry| entry.collection.to_string())
            .collect();
        assert_eq!(
            dropped,
            std::collections::BTreeSet::from(["invoices".to_string(), "receipts".to_string()])
        );
    }

    #[test]
    fn drop_namespace_with_force_removes_links_for_deleted_collections() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest, CreateNamespaceRequest,
            DropNamespaceRequest, ListNeighborsRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let billing_invoice = CollectionId::new("prod.billing.invoices");
        let engineering_ledger = CollectionId::new("prod.engineering.ledger");
        let keep = CollectionId::new("keep");
        let archive = CollectionId::new("archive");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        for schema in ["billing", "engineering"] {
            h.create_namespace(CreateNamespaceRequest {
                database: "prod".into(),
                schema: schema.into(),
            })
            .unwrap();
        }
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("invoices"),
                &Namespace::new("prod", "billing"),
            )
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("ledger"),
                &Namespace::new("prod", "engineering"),
            )
            .unwrap();
        h.storage_mut().register_collection(&keep).unwrap();
        h.storage_mut().register_collection(&archive).unwrap();

        for (collection, id) in [
            (billing_invoice.clone(), "inv-001"),
            (engineering_ledger.clone(), "led-001"),
            (keep.clone(), "keep-001"),
            (archive.clone(), "arc-001"),
        ] {
            h.create_entity(CreateEntityRequest {
                collection,
                id: EntityId::new(id),
                data: json!({ "title": id }),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        for request in [
            CreateLinkRequest {
                source_collection: billing_invoice.clone(),
                source_id: EntityId::new("inv-001"),
                target_collection: engineering_ledger.clone(),
                target_id: EntityId::new("led-001"),
                link_type: "relates-to".into(),
                metadata: serde_json::Value::Null,
                actor: None,
                attribution: None,
            },
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: billing_invoice.clone(),
                target_id: EntityId::new("inv-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
                attribution: None,
            },
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: archive.clone(),
                target_id: EntityId::new("arc-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
                attribution: None,
            },
        ] {
            h.create_link(request).unwrap();
        }

        h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: true,
        })
        .unwrap();

        let keep_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: keep.clone(),
                id: EntityId::new("keep-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(keep_neighbors.total_count, 1);
        assert_eq!(
            keep_neighbors.groups[0].entities[0].id,
            EntityId::new("arc-001")
        );

        let ledger_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: engineering_ledger,
                id: EntityId::new("led-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(ledger_neighbors.total_count, 0);
    }

    #[test]
    fn drop_namespace_with_force_clears_compiled_markdown_cache_for_removed_collections() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateNamespaceRequest,
            DropNamespaceRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let qualified = CollectionId::new("prod.billing.notes");
        let bare = CollectionId::new("notes");
        let id = EntityId::new("note-001");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "billing"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: qualified.clone(),
            id: id.clone(),
            data: json!({"title": "old", "status": "open"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let initial_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(qualified.clone(), "# {{title}}"))
            .unwrap();
        assert_eq!(initial_view.version, 1);
        assert_rendered_markdown(h.get_entity_markdown(&qualified, &id).unwrap(), "# old");

        h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: true,
        })
        .unwrap();

        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "billing"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: qualified.clone(),
            id: id.clone(),
            data: json!({"title": "new", "status": "closed"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let recreated_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(
                qualified.clone(),
                "Status: {{status}}",
            ))
            .unwrap();
        assert_eq!(recreated_view.version, 1);

        assert_rendered_markdown(
            h.get_entity_markdown(&qualified, &id).unwrap(),
            "Status: closed",
        );
    }

    #[test]
    fn drop_namespace_with_force_clears_compiled_markdown_cache_for_ambiguous_bare_aliases() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateNamespaceRequest,
            DropNamespaceRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let bare = CollectionId::new("notes");
        let billing = CollectionId::new("prod.billing.notes");
        let engineering = CollectionId::new("prod.engineering.notes");
        let id = EntityId::new("note-001");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "billing"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: billing.clone(),
            id: id.clone(),
            data: json!({"title": "old", "status": "open"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let initial_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(billing.clone(), "# {{title}}"))
            .unwrap();
        assert_eq!(initial_view.version, 1);
        assert_rendered_markdown(h.get_entity_markdown(&bare, &id).unwrap(), "# old");

        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "engineering".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "engineering"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: engineering.clone(),
            id: id.clone(),
            data: json!({"title": "new", "status": "closed"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let sibling_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(
                engineering.clone(),
                "Status: {{status}}",
            ))
            .unwrap();
        assert_eq!(sibling_view.version, 1);

        h.drop_namespace(DropNamespaceRequest {
            database: "prod".into(),
            schema: "billing".into(),
            force: true,
        })
        .unwrap();

        assert_rendered_markdown(h.get_entity_markdown(&bare, &id).unwrap(), "Status: closed");
    }

    #[test]
    fn default_namespace_exists_on_startup() {
        use crate::request::ListNamespaceCollectionsRequest;
        let h = handler();
        let resp = h
            .list_namespace_collections(ListNamespaceCollectionsRequest {
                database: "default".into(),
                schema: "default".into(),
            })
            .unwrap();
        assert_eq!(resp.database, "default");
        assert_eq!(resp.schema, "default");
    }

    // ── Revalidation tests (US-060) ───────────────────────────────────────

    #[test]
    fn revalidate_all_valid() {
        use crate::request::RevalidateRequest;

        let mut h = handler();
        let col = CollectionId::new("rv-test");
        let schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema,
            actor: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("rv-1"),
            data: json!({"title": "valid"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h.revalidate(RevalidateRequest { collection: col }).unwrap();
        assert_eq!(resp.total_scanned, 1);
        assert_eq!(resp.valid_count, 1);
        assert!(resp.invalid.is_empty());
    }

    #[test]
    fn revalidate_finds_invalid_after_schema_tightened() {
        use crate::request::RevalidateRequest;

        let mut h = handler();
        let col = CollectionId::new("rv-test-2");

        // Loose schema first.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        // Create entities with no constraints.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("rv-2"),
            data: json!({"title": "valid"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("rv-3"),
            data: json!({"no_title": true}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Now tighten the schema.
        h.handle_put_schema(PutSchemaRequest {
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 2,
                entity_schema: Some(json!({
                    "type": "object",
                    "required": ["title"],
                    "properties": {
                        "title": {"type": "string"}
                    }
                })),
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: None,
            force: true,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        let resp = h.revalidate(RevalidateRequest { collection: col }).unwrap();
        assert_eq!(resp.total_scanned, 2);
        assert_eq!(resp.valid_count, 1);
        assert_eq!(resp.invalid.len(), 1);
        assert_eq!(resp.invalid[0].id, "rv-3");
        assert!(!resp.invalid[0].errors.is_empty());
    }

    #[test]
    fn revalidate_empty_collection() {
        use crate::request::RevalidateRequest;

        let mut h = handler();
        let col = CollectionId::new("rv-empty");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: Some(json!({"type": "object"})),
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: None,
        })
        .unwrap();

        let resp = h.revalidate(RevalidateRequest { collection: col }).unwrap();
        assert_eq!(resp.total_scanned, 0);
        assert_eq!(resp.valid_count, 0);
        assert!(resp.invalid.is_empty());
    }

    // ── Gate filter tests (US-074b) ───────────────────────────────────────

    #[test]
    fn query_gate_filter_pass_true() {
        use crate::request::GateFilter;

        let mut h = handler_with_gated_schema();
        let col = CollectionId::new("items");

        // Create entities: one with description (complete gate passes), one without.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-1"),
            data: json!({
                "bead_type": "task",
                "description": "complete",
                "acceptance": "yes",
                "tags": ["x"]
            }),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-2"),
            data: json!({"bead_type": "task"}), // missing description
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Query: gate.complete = true.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col.clone(),
                filter: Some(FilterNode::Gate(GateFilter {
                    gate: "complete".into(),
                    pass: true,
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("gf-1"));
    }

    #[test]
    fn query_gate_filter_pass_false() {
        use crate::request::GateFilter;

        let mut h = handler_with_gated_schema();
        let col = CollectionId::new("items");

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-3"),
            data: json!({
                "bead_type": "task",
                "description": "done",
                "acceptance": "yes",
                "tags": ["x"]
            }),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-4"),
            data: json!({"bead_type": "task"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Query: gate.complete = false.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col.clone(),
                filter: Some(FilterNode::Gate(GateFilter {
                    gate: "complete".into(),
                    pass: false,
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("gf-4"));
    }

    #[test]
    fn gate_filter_combines_with_field_filter() {
        use crate::request::GateFilter;

        let mut h = handler_with_gated_schema();
        let col = CollectionId::new("items");

        // Two passing entities, different types.
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-5"),
            data: json!({
                "bead_type": "task",
                "description": "done",
                "acceptance": "yes",
                "tags": ["x"]
            }),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-6"),
            data: json!({
                "bead_type": "bug",
                "description": "done",
                "priority": 1,
                "acceptance": "yes",
                "tags": ["y"]
            }),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("gf-7"),
            data: json!({"bead_type": "task"}), // fails complete
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // gate.complete = true AND bead_type = "task"
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col.clone(),
                filter: Some(FilterNode::And {
                    filters: vec![
                        FilterNode::Gate(GateFilter {
                            gate: "complete".into(),
                            pass: true,
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "bead_type".into(),
                            op: FilterOp::Eq,
                            value: json!("task"),
                        }),
                    ],
                }),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.entities[0].id, EntityId::new("gf-5"));
    }

    #[test]
    fn gate_filter_no_rules_returns_empty() {
        // Collection without validation rules: gate filters return no results.
        let mut h = handler();
        let col = CollectionId::new("norules");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("nr-1"),
            data: json!({"title": "test"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        use crate::request::GateFilter;
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: col,
                filter: Some(FilterNode::Gate(GateFilter {
                    gate: "complete".into(),
                    pass: true,
                })),
                sort: vec![],
                limit: None,
                after_id: None,
                count_only: false,
            })
            .unwrap();
        assert_eq!(resp.total_count, 0, "no gate results without rules");
    }

    // ── Schema diff tests (US-061) ────────────────────────────────────────

    #[test]
    fn diff_schema_versions_shows_added_fields() {
        use crate::request::DiffSchemaRequest;

        let mut h = handler();
        let col = CollectionId::new("diff-test");

        // Create collection with v1 schema (title only).
        let v1_schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: v1_schema,
            actor: None,
        })
        .unwrap();

        // v2: title + description.
        let v2_schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 2,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "description": {"type": "string"}
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };
        h.handle_put_schema(PutSchemaRequest {
            schema: v2_schema,
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Diff v1 vs v2: v1 is version 1 from create_collection, v2 is version 2.
        let resp = h
            .diff_schema_versions(DiffSchemaRequest {
                collection: col,
                version_a: 1,
                version_b: 2,
            })
            .unwrap();

        assert_eq!(resp.version_a, 1);
        assert_eq!(resp.version_b, 2);
        assert!(
            resp.diff.changes.iter().any(|c| c.path == "description"),
            "should show description was added: {:?}",
            resp.diff.changes
        );
    }

    #[test]
    fn diff_nonexistent_version_returns_error() {
        use crate::request::DiffSchemaRequest;

        let mut h = handler();
        let col = CollectionId::new("diff-test-2");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let result = h.diff_schema_versions(DiffSchemaRequest {
            collection: col,
            version_a: 1,
            version_b: 99,
        });
        assert!(result.is_err());
    }

    #[test]
    fn diff_non_adjacent_versions() {
        use crate::request::DiffSchemaRequest;

        let mut h = handler();
        let col = CollectionId::new("diff-test-3");

        // v1: title.
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"}
                    }
                })),
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: None,
        })
        .unwrap();

        // v2: title + desc.
        h.handle_put_schema(PutSchemaRequest {
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 2,
                entity_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "description": {"type": "string"}
                    }
                })),
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // v3: title + desc + priority.
        h.handle_put_schema(PutSchemaRequest {
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 3,
                entity_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "description": {"type": "string"},
                        "priority": {"type": "integer"}
                    }
                })),
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: None,
            force: false,
            dry_run: false,
            explain_inputs: Vec::new(),
        })
        .unwrap();

        // Diff v1 to v3 (non-adjacent).
        let resp = h
            .diff_schema_versions(DiffSchemaRequest {
                collection: col,
                version_a: 1,
                version_b: 3,
            })
            .unwrap();

        let paths: Vec<&str> = resp.diff.changes.iter().map(|c| c.path.as_str()).collect();
        assert!(
            paths.contains(&"description"),
            "should show description added"
        );
        assert!(paths.contains(&"priority"), "should show priority added");
    }

    // ── Numeric aggregation tests (US-063) ──────────────────────────────

    fn handler_with_numeric_entities() -> AxonHandler<MemoryStorageAdapter> {
        let mut h = handler();
        let col = CollectionId::new("invoices");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema::new(col.clone()),
            actor: None,
        })
        .unwrap();

        let items = vec![
            json!({"amount": 100, "status": "draft", "priority": 1}),
            json!({"amount": 200, "status": "draft", "priority": 2}),
            json!({"amount": 300, "status": "pending", "priority": 1}),
            json!({"amount": 50, "status": "pending"}), // no priority
            json!({"status": "done", "title": "no-amount"}), // no amount
        ];
        for (i, data) in items.into_iter().enumerate() {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("inv-{i}")),
                data,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }
        h
    }

    #[test]
    fn aggregate_sum() {
        let h = handler_with_numeric_entities();
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Sum,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.results.len(), 1);
        assert!((resp.results[0].value - 650.0).abs() < f64::EPSILON);
        assert_eq!(resp.results[0].count, 4); // 4 entities have amount
    }

    #[test]
    fn aggregate_avg_returns_float() {
        let h = handler_with_numeric_entities();
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Avg,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert_eq!(resp.results.len(), 1);
        assert!((resp.results[0].value - 162.5).abs() < f64::EPSILON); // 650/4
    }

    #[test]
    fn aggregate_min_max() {
        let h = handler_with_numeric_entities();
        let min_resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Min,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert!((min_resp.results[0].value - 50.0).abs() < f64::EPSILON);

        let max_resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Max,
                field: "amount".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        assert!((max_resp.results[0].value - 300.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_group_by() {
        let h = handler_with_numeric_entities();
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Avg,
                field: "priority".into(),
                filter: None,
                group_by: Some("status".into()),
            })
            .unwrap();

        // draft: avg(1,2) = 1.5
        let draft = resp.results.iter().find(|g| g.key == json!("draft"));
        assert!(draft.is_some());
        assert!((draft.unwrap().value - 1.5).abs() < f64::EPSILON);

        // pending: avg(1) = 1.0 (only one entity has priority)
        let pending = resp.results.iter().find(|g| g.key == json!("pending"));
        assert!(pending.is_some());
        assert!((pending.unwrap().value - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_non_numeric_field_returns_error() {
        let h = handler_with_numeric_entities();
        let result = h.aggregate(AggregateRequest {
            collection: CollectionId::new("invoices"),
            function: AggregateFunction::Sum,
            field: "status".into(),
            filter: None,
            group_by: None,
        });
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("not numeric"),
            "should report type error"
        );
    }

    #[test]
    fn aggregate_null_excluded() {
        let h = handler_with_numeric_entities();
        // Priority has nulls for some entities.
        let resp = h
            .aggregate(AggregateRequest {
                collection: CollectionId::new("invoices"),
                function: AggregateFunction::Sum,
                field: "priority".into(),
                filter: None,
                group_by: None,
            })
            .unwrap();
        // Only 3 entities have priority: 1 + 2 + 1 = 4
        assert!((resp.results[0].value - 4.0).abs() < f64::EPSILON);
        assert_eq!(resp.results[0].count, 3);
    }

    // ── Secondary index tests (FEAT-013, US-031) ────────────────────────

    fn setup_indexed_collection() -> AxonHandler<MemoryStorageAdapter> {
        use axon_schema::schema::{IndexDef, IndexType};

        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        let schema = CollectionSchema {
            collection: CollectionId::new("tasks"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "priority": { "type": "integer" }
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![
                IndexDef {
                    field: "status".into(),
                    index_type: IndexType::String,
                    unique: false,
                },
                IndexDef {
                    field: "priority".into(),
                    index_type: IndexType::Integer,
                    unique: false,
                },
            ],
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("tasks"),
            schema,
            actor: Some("test".into()),
        })
        .unwrap();

        // Insert test entities.
        for (id, status, priority) in &[
            ("t-001", "pending", 1),
            ("t-002", "pending", 2),
            ("t-003", "done", 3),
            ("t-004", "done", 1),
            ("t-005", "in_progress", 2),
        ] {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new(*id),
                data: json!({"status": status, "priority": priority}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        h
    }

    #[test]
    fn index_equality_query_returns_matching_entities() {
        let h = setup_indexed_collection();

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("pending"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 2);
        let ids: Vec<&str> = resp.entities.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"t-001"));
        assert!(ids.contains(&"t-002"));
    }

    #[test]
    fn index_range_query_gt() {
        let h = setup_indexed_collection();

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "priority".into(),
                    op: FilterOp::Gt,
                    value: json!(1),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 3);
        // priority > 1: t-002 (2), t-003 (3), t-005 (2)
    }

    #[test]
    fn non_indexed_field_falls_back_to_scan() {
        let h = setup_indexed_collection();

        // Filter on a field that has no index.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "nonexistent".into(),
                    op: FilterOp::Eq,
                    value: json!("value"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 0);
    }

    #[test]
    fn and_filter_uses_index_for_one_field() {
        let h = setup_indexed_collection();

        // AND filter: status=pending AND priority=2
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::And {
                    filters: vec![
                        FilterNode::Field(FieldFilter {
                            field: "status".into(),
                            op: FilterOp::Eq,
                            value: json!("pending"),
                        }),
                        FilterNode::Field(FieldFilter {
                            field: "priority".into(),
                            op: FilterOp::Eq,
                            value: json!(2),
                        }),
                    ],
                }),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "t-002");
    }

    #[test]
    fn index_maintenance_on_update() {
        let mut h = setup_indexed_collection();

        // Update t-001 status from pending to done.
        h.update_entity(UpdateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"status": "done", "priority": 1}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Query for pending — should now only return t-002.
        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("pending"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "t-002");
    }

    #[test]
    fn namespaced_update_preserves_qualified_collection_for_indexes_and_audit() {
        use axon_schema::schema::{IndexDef, IndexType};
        use axon_storage::adapter::IndexValue;

        let mut h = handler();
        let (billing, engineering) =
            register_prod_billing_and_engineering_collection(&mut h, "invoices");
        let entity_id = EntityId::new("inv-001");

        let indexed_schema = |collection: CollectionId| CollectionSchema {
            collection,
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![IndexDef {
                field: "status".into(),
                index_type: IndexType::String,
                unique: false,
            }],
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        h.put_schema(indexed_schema(billing.clone())).unwrap();
        h.put_schema(indexed_schema(engineering.clone())).unwrap();

        for collection in [billing.clone(), engineering.clone()] {
            h.create_entity(CreateEntityRequest {
                collection,
                id: entity_id.clone(),
                data: json!({ "status": "pending" }),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        let response = h
            .update_entity(UpdateEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
                data: json!({ "status": "paid" }),
                expected_version: 1,
                actor: Some("agent-1".into()),
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(response.entity.collection, billing);
        assert_eq!(
            h.storage_mut()
                .index_lookup(&billing, "status", &IndexValue::String("paid".into()))
                .unwrap(),
            vec![entity_id.clone()]
        );
        assert!(h
            .storage_mut()
            .index_lookup(&billing, "status", &IndexValue::String("pending".into()))
            .unwrap()
            .is_empty());
        assert_eq!(
            h.storage_mut()
                .index_lookup(
                    &engineering,
                    "status",
                    &IndexValue::String("pending".into())
                )
                .unwrap(),
            vec![entity_id.clone()]
        );

        let audit = h.audit_log().query_by_entity(&billing, &entity_id).unwrap();
        assert_eq!(audit.len(), 2);
        assert_eq!(audit[1].collection, billing);
        assert_eq!(
            h.get_entity(GetEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
            })
            .unwrap()
            .entity
            .collection,
            billing
        );
    }

    #[test]
    fn index_maintenance_on_delete() {
        let mut h = setup_indexed_collection();

        h.delete_entity(DeleteEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            actor: None,
            audit_metadata: None,
            force: false,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .query_entities(QueryEntitiesRequest {
                collection: CollectionId::new("tasks"),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "status".into(),
                    op: FilterOp::Eq,
                    value: json!("pending"),
                })),
                sort: vec![],
                after_id: None,
                limit: None,
                count_only: false,
            })
            .unwrap();

        assert_eq!(resp.entities.len(), 1);
        assert_eq!(resp.entities[0].id.as_str(), "t-002");
    }

    #[test]
    fn schema_rejects_empty_index_field() {
        use axon_schema::schema::{IndexDef, IndexType};

        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        let schema = CollectionSchema {
            collection: CollectionId::new("bad"),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![IndexDef {
                field: "".into(),
                index_type: IndexType::String,
                unique: false,
            }],
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        let err = h.put_schema(schema).unwrap_err();
        assert!(
            matches!(err, AxonError::SchemaValidation(_)),
            "expected SchemaValidation, got: {err}"
        );
    }

    // ── Unique index enforcement tests (US-032) ─────────────────────────

    fn setup_unique_indexed_collection() -> AxonHandler<MemoryStorageAdapter> {
        use axon_schema::schema::{IndexDef, IndexType};

        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        let schema = CollectionSchema {
            collection: CollectionId::new("users"),
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "email": { "type": "string" },
                    "name": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: vec![IndexDef {
                field: "email".into(),
                index_type: IndexType::String,
                unique: true,
            }],
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        h.create_collection(CreateCollectionRequest {
            name: CollectionId::new("users"),
            schema,
            actor: Some("test".into()),
        })
        .unwrap();

        h
    }

    #[test]
    fn unique_index_rejects_duplicate_on_create() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com", "name": "Alice"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let err = h
            .create_entity(CreateEntityRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-002"),
                data: json!({"email": "alice@example.com", "name": "Bob"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();

        match &err {
            AxonError::UniqueViolation { field, value } => {
                assert_eq!(field, "email");
                assert!(value.contains("alice@example.com"), "value: {value}");
            }
            other => panic!("expected UniqueViolation, got: {other}"),
        }
    }

    #[test]
    fn unique_index_allows_different_values() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-002"),
            data: json!({"email": "bob@example.com"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    #[test]
    fn unique_index_allows_update_same_entity() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com", "name": "Alice"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Update name but keep same email — should succeed.
        h.update_entity(UpdateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com", "name": "Alice Smith"}),
            expected_version: 1,
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    #[test]
    fn unique_index_rejects_duplicate_on_update() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-002"),
            data: json!({"email": "bob@example.com"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // Try to update u-002 to have alice's email.
        let err = h
            .update_entity(UpdateEntityRequest {
                collection: CollectionId::new("users"),
                id: EntityId::new("u-002"),
                data: json!({"email": "alice@example.com"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();

        assert!(
            matches!(err, AxonError::UniqueViolation { .. }),
            "expected UniqueViolation, got: {err}"
        );
    }

    #[test]
    fn unique_index_freed_after_delete() {
        let mut h = setup_unique_indexed_collection();

        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.delete_entity(DeleteEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-001"),
            actor: None,
            audit_metadata: None,
            force: false,
            attribution: None,
        })
        .unwrap();

        // After delete, the email should be available.
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("users"),
            id: EntityId::new("u-002"),
            data: json!({"email": "alice@example.com"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }

    // ── List neighbors tests (US-071, FEAT-020) ─────────────────────────

    fn setup_neighbor_graph() -> AxonHandler<MemoryStorageAdapter> {
        let mut h = AxonHandler::new(MemoryStorageAdapter::default());

        // Create two collections.
        for name in &["tasks", "users"] {
            h.create_collection(CreateCollectionRequest {
                name: CollectionId::new(*name),
                schema: CollectionSchema::new(CollectionId::new(*name)),
                actor: Some("test".into()),
            })
            .unwrap();
        }

        // Create entities.
        for (col, id) in &[
            ("tasks", "t-001"),
            ("tasks", "t-002"),
            ("tasks", "t-003"),
            ("users", "u-001"),
        ] {
            h.create_entity(CreateEntityRequest {
                collection: CollectionId::new(*col),
                id: EntityId::new(*id),
                data: json!({"title": id}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        // Create links: t-001 --depends-on--> t-002, t-001 --depends-on--> t-003
        // u-001 --assigned-to--> t-001
        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("tasks"),
            source_id: EntityId::new("t-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-002"),
            link_type: "depends-on".into(),
            metadata: serde_json::Value::Null,
            actor: None,
            attribution: None,
        })
        .unwrap();

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("tasks"),
            source_id: EntityId::new("t-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-003"),
            link_type: "depends-on".into(),
            metadata: serde_json::Value::Null,
            actor: None,
            attribution: None,
        })
        .unwrap();

        h.create_link(CreateLinkRequest {
            source_collection: CollectionId::new("users"),
            source_id: EntityId::new("u-001"),
            target_collection: CollectionId::new("tasks"),
            target_id: EntityId::new("t-001"),
            link_type: "assigned-to".into(),
            metadata: serde_json::Value::Null,
            actor: None,
            attribution: None,
        })
        .unwrap();

        h
    }

    #[test]
    fn list_neighbors_returns_outbound_and_inbound() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();

        // t-001 has 2 outbound depends-on and 1 inbound assigned-to.
        assert_eq!(resp.total_count, 3);
        assert_eq!(resp.groups.len(), 2); // depends-on/outbound + assigned-to/inbound

        let outbound = resp
            .groups
            .iter()
            .find(|g| g.direction == "outbound" && g.link_type == "depends-on")
            .unwrap();
        assert_eq!(outbound.entities.len(), 2);

        let inbound = resp
            .groups
            .iter()
            .find(|g| g.direction == "inbound" && g.link_type == "assigned-to")
            .unwrap();
        assert_eq!(inbound.entities.len(), 1);
    }

    #[test]
    fn list_neighbors_filter_by_direction() {
        let h = setup_neighbor_graph();

        // Only outbound.
        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: None,
                direction: Some(TraverseDirection::Forward),
            })
            .unwrap();

        assert_eq!(resp.total_count, 2); // only outbound depends-on
        assert!(resp.groups.iter().all(|g| g.direction == "outbound"));
    }

    #[test]
    fn list_neighbors_filter_by_link_type() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: Some("assigned-to".into()),
                direction: None,
            })
            .unwrap();

        // Only the inbound assigned-to from u-001.
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.groups.len(), 1);
        assert_eq!(resp.groups[0].link_type, "assigned-to");
    }

    #[test]
    fn list_neighbors_entity_not_found() {
        let h = setup_neighbor_graph();

        let err = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("ghost"),
                link_type: None,
                direction: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn list_neighbors_entity_with_no_links() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-003"),
                link_type: None,
                direction: None,
            })
            .unwrap();

        // t-003 has 1 inbound depends-on from t-001.
        assert_eq!(resp.total_count, 1);
        assert_eq!(resp.groups[0].direction, "inbound");
    }

    #[test]
    fn list_neighbors_includes_entity_data() {
        let h = setup_neighbor_graph();

        let resp = h
            .list_neighbors(crate::request::ListNeighborsRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("t-001"),
                link_type: Some("depends-on".into()),
                direction: Some(TraverseDirection::Forward),
            })
            .unwrap();

        assert_eq!(resp.total_count, 2);
        for entity in &resp.groups[0].entities {
            assert!(
                entity.data.get("title").is_some(),
                "entity data should be included"
            );
        }
    }

    // ── Find link candidates tests (US-070, FEAT-020) ───────────────────

    #[test]
    fn find_link_candidates_returns_target_entities() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: None,
                limit: None,
            })
            .unwrap();

        // Target collection defaults to source collection (no schema link def).
        assert_eq!(resp.target_collection, "tasks");
        // t-001 has 2 existing depends-on links.
        assert_eq!(resp.existing_link_count, 2);
        // All 3 tasks are candidates (including t-001 itself).
        assert!(resp.candidates.len() >= 3);
    }

    #[test]
    fn find_link_candidates_marks_already_linked() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: None,
                limit: None,
            })
            .unwrap();

        let t002 = resp
            .candidates
            .iter()
            .find(|c| c.entity.id.as_str() == "t-002")
            .unwrap();
        assert!(t002.already_linked, "t-002 is linked");

        let t001 = resp
            .candidates
            .iter()
            .find(|c| c.entity.id.as_str() == "t-001")
            .unwrap();
        assert!(!t001.already_linked, "t-001 is not linked to itself");
    }

    #[test]
    fn find_link_candidates_with_filter() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: Some(FilterNode::Field(FieldFilter {
                    field: "title".into(),
                    op: FilterOp::Eq,
                    value: json!("t-003"),
                })),
                limit: None,
            })
            .unwrap();

        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].entity.id.as_str(), "t-003");
    }

    #[test]
    fn find_link_candidates_with_limit() {
        let h = setup_neighbor_graph();

        let resp = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("t-001"),
                link_type: "depends-on".into(),
                filter: None,
                limit: Some(1),
            })
            .unwrap();

        assert_eq!(resp.candidates.len(), 1);
    }

    #[test]
    fn find_link_candidates_source_not_found() {
        let h = setup_neighbor_graph();

        let err = h
            .find_link_candidates(crate::request::FindLinkCandidatesRequest {
                source_collection: CollectionId::new("tasks"),
                source_id: EntityId::new("ghost"),
                link_type: "depends-on".into(),
                filter: None,
                limit: None,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::NotFound(_)));
    }

    // ── Database isolation tests (US-035) ──────────────────────────────

    #[test]
    fn create_database() {
        use crate::request::CreateDatabaseRequest;
        let mut h = handler();
        let resp = h
            .create_database(CreateDatabaseRequest {
                name: "mydb".into(),
            })
            .unwrap();
        assert_eq!(resp.name, "mydb");
    }

    #[test]
    fn create_duplicate_database_fails() {
        use crate::request::CreateDatabaseRequest;
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "mydb".into(),
        })
        .unwrap();
        let err = h
            .create_database(CreateDatabaseRequest {
                name: "mydb".into(),
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::AlreadyExists(_)));
    }

    #[test]
    fn list_databases_includes_default() {
        use crate::request::ListDatabasesRequest;
        let h = handler();
        let resp = h.list_databases(ListDatabasesRequest {}).unwrap();
        assert!(resp.databases.contains(&"default".to_string()));
    }

    #[test]
    fn list_databases_after_create() {
        use crate::request::{CreateDatabaseRequest, ListDatabasesRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "analytics".into(),
        })
        .unwrap();
        let resp = h.list_databases(ListDatabasesRequest {}).unwrap();
        assert!(resp.databases.contains(&"analytics".to_string()));
        assert!(resp.databases.contains(&"default".to_string()));
    }

    #[test]
    fn drop_empty_database() {
        use crate::request::{CreateDatabaseRequest, DropDatabaseRequest, ListDatabasesRequest};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "temp".into(),
        })
        .unwrap();
        let resp = h
            .drop_database(DropDatabaseRequest {
                name: "temp".into(),
                force: false,
            })
            .unwrap();
        assert_eq!(resp.name, "temp");
        assert_eq!(resp.collections_removed, 0);

        let dbs = h.list_databases(ListDatabasesRequest {}).unwrap();
        assert!(!dbs.databases.contains(&"temp".to_string()));
    }

    #[test]
    fn drop_default_database_is_forbidden() {
        use crate::request::{DropDatabaseRequest, ListDatabasesRequest};

        let mut h = handler();
        let err = h
            .drop_database(DropDatabaseRequest {
                name: DEFAULT_DATABASE.into(),
                force: true,
            })
            .unwrap_err();

        assert!(matches!(err, AxonError::InvalidOperation(_)));
        assert!(err
            .to_string()
            .contains("database 'default' is implicit and cannot be dropped"));

        let dbs = h.list_databases(ListDatabasesRequest {}).unwrap();
        assert!(dbs.databases.contains(&DEFAULT_DATABASE.to_string()));
    }

    #[test]
    fn drop_nonexistent_database_fails() {
        use crate::request::DropDatabaseRequest;
        let mut h = handler();
        let err = h
            .drop_database(DropDatabaseRequest {
                name: "nope".into(),
                force: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn drop_nonempty_database_requires_force() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropDatabaseRequest};
        use axon_core::id::{CollectionId, Namespace};
        let mut h = handler();
        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        // Add a second schema namespace to the same database.
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("tasks"),
                &Namespace::new("prod", "default"),
            )
            .unwrap();

        let err = h
            .drop_database(DropDatabaseRequest {
                name: "prod".into(),
                force: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidOperation(_)));

        // Force drop succeeds and removes all schemas.
        let resp = h
            .drop_database(DropDatabaseRequest {
                name: "prod".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 1);
    }

    #[test]
    fn drop_database_with_force_records_collection_drop_audits() {
        use crate::request::{CreateDatabaseRequest, CreateNamespaceRequest, DropDatabaseRequest};
        use axon_core::id::Namespace;
        use axon_core::types::Entity;

        let mut h = handler();
        let analytics = Namespace::new("prod", "analytics");
        let orders = CollectionId::new("orders");
        let rollups = CollectionId::new("rollups");
        let keep = CollectionId::new("keep");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&orders, &Namespace::new("prod", "default"))
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&rollups, &analytics)
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&keep, &Namespace::default_ns())
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                orders.clone(),
                EntityId::new("ord-001"),
                json!({"title": "order"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                rollups.clone(),
                EntityId::new("sum-001"),
                json!({"title": "rollup"}),
            ))
            .unwrap();
        h.storage_mut()
            .put(Entity::new(
                keep.clone(),
                EntityId::new("keep-001"),
                json!({"title": "keep"}),
            ))
            .unwrap();

        let resp = h
            .drop_database(DropDatabaseRequest {
                name: "prod".into(),
                force: true,
            })
            .unwrap();
        assert_eq!(resp.collections_removed, 2);
        assert!(h
            .storage
            .get(&orders, &EntityId::new("ord-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&rollups, &EntityId::new("sum-001"))
            .unwrap()
            .is_none());
        assert!(h
            .storage
            .get(&keep, &EntityId::new("keep-001"))
            .unwrap()
            .is_some());

        let drops = h
            .audit_log()
            .query_by_operation(&axon_audit::entry::MutationType::CollectionDrop)
            .unwrap();
        assert_eq!(drops.len(), 2);
        let dropped: std::collections::BTreeSet<_> = drops
            .iter()
            .map(|entry| entry.collection.to_string())
            .collect();
        assert_eq!(
            dropped,
            std::collections::BTreeSet::from(["orders".to_string(), "rollups".to_string()])
        );
    }

    #[test]
    fn drop_database_with_force_removes_links_for_deleted_collections() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateLinkRequest, CreateNamespaceRequest,
            DropDatabaseRequest, ListNeighborsRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let orders = CollectionId::new("prod.default.orders");
        let rollups = CollectionId::new("prod.analytics.rollups");
        let keep = CollectionId::new("keep");
        let archive = CollectionId::new("archive");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("orders"),
                &Namespace::new("prod", "default"),
            )
            .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(
                &CollectionId::new("rollups"),
                &Namespace::new("prod", "analytics"),
            )
            .unwrap();
        h.storage_mut().register_collection(&keep).unwrap();
        h.storage_mut().register_collection(&archive).unwrap();

        for (collection, id) in [
            (orders.clone(), "ord-001"),
            (rollups.clone(), "sum-001"),
            (keep.clone(), "keep-001"),
            (archive.clone(), "arc-001"),
        ] {
            h.create_entity(CreateEntityRequest {
                collection,
                id: EntityId::new(id),
                data: json!({ "title": id }),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        for request in [
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: orders.clone(),
                target_id: EntityId::new("ord-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
                attribution: None,
            },
            CreateLinkRequest {
                source_collection: rollups.clone(),
                source_id: EntityId::new("sum-001"),
                target_collection: keep.clone(),
                target_id: EntityId::new("keep-001"),
                link_type: "feeds".into(),
                metadata: serde_json::Value::Null,
                actor: None,
                attribution: None,
            },
            CreateLinkRequest {
                source_collection: keep.clone(),
                source_id: EntityId::new("keep-001"),
                target_collection: archive.clone(),
                target_id: EntityId::new("arc-001"),
                link_type: "references".into(),
                metadata: serde_json::Value::Null,
                actor: None,
                attribution: None,
            },
        ] {
            h.create_link(request).unwrap();
        }

        h.drop_database(DropDatabaseRequest {
            name: "prod".into(),
            force: true,
        })
        .unwrap();

        let keep_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: keep.clone(),
                id: EntityId::new("keep-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(keep_neighbors.total_count, 1);
        assert_eq!(
            keep_neighbors.groups[0].entities[0].id,
            EntityId::new("arc-001")
        );

        let archive_neighbors = h
            .list_neighbors(ListNeighborsRequest {
                collection: archive,
                id: EntityId::new("arc-001"),
                link_type: None,
                direction: None,
            })
            .unwrap();
        assert_eq!(archive_neighbors.total_count, 1);
        assert_eq!(
            archive_neighbors.groups[0].entities[0].id,
            EntityId::new("keep-001")
        );
    }

    #[test]
    fn drop_database_with_force_clears_compiled_markdown_cache_for_removed_collections() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateNamespaceRequest, DropDatabaseRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let qualified = CollectionId::new("prod.analytics.reports");
        let bare = CollectionId::new("reports");
        let id = EntityId::new("report-001");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "analytics"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: qualified.clone(),
            id: id.clone(),
            data: json!({"title": "old", "status": "draft"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let initial_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(qualified.clone(), "# {{title}}"))
            .unwrap();
        assert_eq!(initial_view.version, 1);
        assert_rendered_markdown(h.get_entity_markdown(&qualified, &id).unwrap(), "# old");

        h.drop_database(DropDatabaseRequest {
            name: "prod".into(),
            force: true,
        })
        .unwrap();

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "analytics"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: qualified.clone(),
            id: id.clone(),
            data: json!({"title": "new", "status": "published"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let recreated_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(
                qualified.clone(),
                "Status: {{status}}",
            ))
            .unwrap();
        assert_eq!(recreated_view.version, 1);

        assert_rendered_markdown(
            h.get_entity_markdown(&qualified, &id).unwrap(),
            "Status: published",
        );
    }

    #[test]
    fn drop_database_with_force_clears_compiled_markdown_cache_for_ambiguous_bare_aliases() {
        use crate::request::{
            CreateDatabaseRequest, CreateEntityRequest, CreateNamespaceRequest, DropDatabaseRequest,
        };
        use axon_core::id::Namespace;

        let mut h = handler();
        let bare = CollectionId::new("reports");
        let prod = CollectionId::new("prod.analytics.reports");
        let stage = CollectionId::new("stage.analytics.reports");
        let id = EntityId::new("report-001");

        h.create_database(CreateDatabaseRequest {
            name: "prod".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "prod".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("prod", "analytics"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: prod.clone(),
            id: id.clone(),
            data: json!({"title": "old", "status": "draft"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let initial_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(prod.clone(), "# {{title}}"))
            .unwrap();
        assert_eq!(initial_view.version, 1);
        assert_rendered_markdown(h.get_entity_markdown(&bare, &id).unwrap(), "# old");

        h.create_database(CreateDatabaseRequest {
            name: "stage".into(),
        })
        .unwrap();
        h.create_namespace(CreateNamespaceRequest {
            database: "stage".into(),
            schema: "analytics".into(),
        })
        .unwrap();
        h.storage_mut()
            .register_collection_in_namespace(&bare, &Namespace::new("stage", "analytics"))
            .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: stage.clone(),
            id: id.clone(),
            data: json!({"title": "new", "status": "published"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        let sibling_view = h
            .storage_mut()
            .put_collection_view(&CollectionView::new(stage.clone(), "Status: {{status}}"))
            .unwrap();
        assert_eq!(sibling_view.version, 1);

        h.drop_database(DropDatabaseRequest {
            name: "prod".into(),
            force: true,
        })
        .unwrap();

        assert_rendered_markdown(
            h.get_entity_markdown(&bare, &id).unwrap(),
            "Status: published",
        );
    }

    // ── Audit metadata (US-009) ─────────────────────────────────────────────

    #[test]
    fn create_entity_passes_audit_metadata() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        let mut meta = std::collections::HashMap::new();
        meta.insert("reason".into(), "batch-import".into());
        meta.insert("session_id".into(), "sess-42".into());

        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: Some("agent-1".into()),
            audit_metadata: Some(meta),
            attribution: None,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(audit.entries.len(), 1);
        assert_eq!(audit.entries[0].metadata["reason"], "batch-import");
        assert_eq!(audit.entries[0].metadata["session_id"], "sess-42");
    }

    #[test]
    fn update_entity_passes_audit_metadata() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let mut meta = std::collections::HashMap::new();
        meta.insert("ticket".into(), "PROJ-123".into());
        h.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "updated"}),
            expected_version: 1,
            actor: None,
            audit_metadata: Some(meta),
            attribution: None,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        // Second entry is the update.
        let update_entry = &audit.entries[1];
        assert_eq!(update_entry.metadata["ticket"], "PROJ-123");
    }

    #[test]
    fn delete_entity_passes_audit_metadata() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let mut meta = std::collections::HashMap::new();
        meta.insert("reason".into(), "cleanup".into());
        h.delete_entity(DeleteEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            actor: None,
            force: false,
            audit_metadata: Some(meta),
            attribution: None,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        let delete_entry = &audit.entries[1];
        assert_eq!(delete_entry.metadata["reason"], "cleanup");
    }

    #[test]
    fn audit_metadata_is_optional() {
        let mut h = handler();
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(CollectionId::new("tasks")),
                ..Default::default()
            })
            .unwrap();
        assert!(audit.entries[0].metadata.is_empty());
    }

    // ── Drop collection confirmation (US-003) ───────────────────────────────

    #[test]
    fn drop_collection_requires_confirm() {
        use axon_schema::schema::CollectionSchema;
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: None,
        })
        .unwrap();

        // Without confirm=true, drop is rejected.
        let err = h
            .drop_collection(DropCollectionRequest {
                name: col.clone(),
                actor: None,
                confirm: false,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::InvalidArgument(_)));

        // With confirm=true, drop succeeds.
        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
            confirm: true,
        })
        .unwrap();

        // Collection is gone.
        assert!(h.storage.list_collections().unwrap().is_empty());
    }

    #[test]
    fn drop_collection_audit_includes_entity_count() {
        use axon_schema::schema::CollectionSchema;
        let mut h = handler();
        let col = CollectionId::new("widgets");
        h.create_collection(CreateCollectionRequest {
            name: col.clone(),
            schema: CollectionSchema {
                collection: col.clone(),
                description: None,
                version: 1,
                entity_schema: None,
                link_types: Default::default(),
                access_control: None,
                gates: Default::default(),
                validation_rules: Default::default(),
                indexes: Default::default(),
                compound_indexes: Default::default(),
                queries: Default::default(),
                lifecycles: Default::default(),
            },
            actor: None,
        })
        .unwrap();
        // Add 3 entities.
        for i in 0..3 {
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new(format!("w-{i}")),
                data: json!({"name": format!("widget-{i}")}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }

        h.drop_collection(DropCollectionRequest {
            name: col.clone(),
            actor: None,
            confirm: true,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        // Last entry is the drop.
        let drop_entry = audit.entries.last().unwrap();
        assert_eq!(drop_entry.metadata["entities_removed"], "3");
    }

    // ── Patch entity / merge-patch (US-012) ─────────────────────────────────

    #[test]
    fn patch_entity_merges_fields() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello", "status": "draft", "priority": 3}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"status": "active"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        // Status changed, title and priority preserved.
        assert_eq!(resp.entity.data["status"], "active");
        assert_eq!(resp.entity.data["title"], "hello");
        assert_eq!(resp.entity.data["priority"], 3);
        assert_eq!(resp.entity.version, 2);
    }

    #[test]
    fn patch_entity_removes_null_fields() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello", "notes": "some notes"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"notes": null}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.entity.data["title"], "hello");
        assert!(resp.entity.data.get("notes").is_none());
    }

    #[test]
    fn patch_entity_adds_new_fields() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let resp = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"assignee": "agent-1"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(resp.entity.data["title"], "hello");
        assert_eq!(resp.entity.data["assignee"], "agent-1");
    }

    #[test]
    fn patch_entity_occ_conflict() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let err = h
            .patch_entity(PatchEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                patch: json!({"title": "changed"}),
                expected_version: 99,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::ConflictingVersion { .. }));
    }

    #[test]
    fn patch_entity_not_found() {
        let mut h = handler();
        let err = h
            .patch_entity(PatchEntityRequest {
                collection: CollectionId::new("tasks"),
                id: EntityId::new("ghost"),
                patch: json!({"title": "changed"}),
                expected_version: 1,
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap_err();
        assert!(matches!(err, AxonError::NotFound(_)));
    }

    #[test]
    fn patch_entity_creates_audit_entry() {
        let mut h = handler();
        let col = CollectionId::new("tasks");
        h.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            data: json!({"title": "hello", "status": "draft"}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        h.patch_entity(PatchEntityRequest {
            collection: col.clone(),
            id: EntityId::new("t-001"),
            patch: json!({"status": "active"}),
            expected_version: 1,
            actor: Some("agent-1".into()),
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let audit = h
            .query_audit(crate::request::QueryAuditRequest {
                collection: Some(col),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(audit.entries.len(), 2);
        let patch_entry = &audit.entries[1];
        assert_eq!(patch_entry.actor, "agent-1");
        // Before had status=draft, after has status=active.
        assert_eq!(patch_entry.data_before.as_ref().unwrap()["status"], "draft");
        assert_eq!(patch_entry.data_after.as_ref().unwrap()["status"], "active");
    }

    #[test]
    fn namespaced_patch_preserves_qualified_collection_for_gate_results_and_audit() {
        use axon_schema::rules::{RequirementOp, RuleRequirement, ValidationRule};
        use axon_schema::schema::GateDef;

        let mut h = handler();
        let (billing, engineering) =
            register_prod_billing_and_engineering_collection(&mut h, "invoices");
        let entity_id = EntityId::new("inv-001");

        let gated_schema = |collection: CollectionId| CollectionSchema {
            collection,
            description: None,
            version: 1,
            entity_schema: Some(json!({
                "type": "object",
                "properties": {
                    "bead_type": { "type": "string" },
                    "description": { "type": "string" }
                }
            })),
            link_types: Default::default(),
            access_control: None,
            gates: std::collections::HashMap::from([(
                "complete".into(),
                GateDef {
                    description: Some("Ready for completion".into()),
                    includes: vec![],
                },
            )]),
            validation_rules: vec![
                ValidationRule {
                    name: "need-type".into(),
                    gate: Some("save".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "bead_type".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "bead_type is required".into(),
                    fix: None,
                },
                ValidationRule {
                    name: "need-desc".into(),
                    gate: Some("complete".into()),
                    advisory: false,
                    when: None,
                    require: RuleRequirement {
                        field: "description".into(),
                        op: RequirementOp::NotNull(true),
                    },
                    message: "description is required".into(),
                    fix: None,
                },
            ],
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles: Default::default(),
        };

        h.put_schema(gated_schema(billing.clone())).unwrap();
        h.create_entity(CreateEntityRequest {
            collection: billing.clone(),
            id: entity_id.clone(),
            data: json!({ "bead_type": "invoice" }),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
        h.create_entity(CreateEntityRequest {
            collection: engineering.clone(),
            id: entity_id.clone(),
            data: json!({ "bead_type": "invoice" }),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        let response = h
            .patch_entity(PatchEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
                patch: json!({ "description": "ready" }),
                expected_version: 1,
                actor: Some("agent-1".into()),
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();

        assert_eq!(response.entity.collection, billing);
        assert!(response.gates["complete"].pass);
        // Gate results live on the entity blob (FEAT-019): re-read and
        // assert the materialized verdict made it onto the entity.
        let billing_stored = h
            .get_entity(GetEntityRequest {
                collection: billing.clone(),
                id: entity_id.clone(),
            })
            .unwrap()
            .entity;
        assert_eq!(billing_stored.collection, billing);
        assert!(billing_stored.gate_results.get("complete").unwrap().pass);
        assert_eq!(
            h.audit_log().query_by_entity(&billing, &entity_id).unwrap()[1].collection,
            billing
        );
        // The sibling namespace must not see any gate results since its
        // entity was never patched with a description.
        let engineering_stored = h
            .get_entity(GetEntityRequest {
                collection: engineering.clone(),
                id: entity_id.clone(),
            })
            .unwrap()
            .entity;
        assert!(!engineering_stored
            .gate_results
            .get("complete")
            .map(|gr| gr.pass)
            .unwrap_or(false));
    }

    #[test]
    fn json_merge_patch_rfc7396_nested() {
        use serde_json::json;
        let mut target = json!({"a": {"b": 1, "c": 2}, "d": 3});
        let patch = json!({"a": {"b": null, "e": 4}});
        json_merge_patch(&mut target, &patch);
        assert_eq!(target, json!({"a": {"c": 2, "e": 4}, "d": 3}));
    }

    // ── Snapshot tests (US-080, FEAT-004) ────────────────────────────────────
    //
    // Multi-page snapshot consistency under concurrent writes is NOT
    // guaranteed by this implementation. `MemoryStorageAdapter` has no
    // storage-level snapshot isolation; concurrent mutations between paginated
    // requests can cause a multi-page snapshot to reflect mixed state. All
    // tests below are single-threaded and therefore fully consistent.

    fn seed_tasks_and_notes(h: &mut AxonHandler<MemoryStorageAdapter>) {
        let tasks = CollectionId::new("tasks");
        let notes = CollectionId::new("notes");
        h.create_collection(CreateCollectionRequest {
            name: tasks.clone(),
            schema: CollectionSchema::new(tasks.clone()),
            actor: None,
        })
        .unwrap();
        h.create_collection(CreateCollectionRequest {
            name: notes.clone(),
            schema: CollectionSchema::new(notes.clone()),
            actor: None,
        })
        .unwrap();

        for i in 0..3 {
            h.create_entity(CreateEntityRequest {
                collection: tasks.clone(),
                id: EntityId::new(format!("t-{i:03}")),
                data: json!({"n": i}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }
        for i in 0..2 {
            h.create_entity(CreateEntityRequest {
                collection: notes.clone(),
                id: EntityId::new(format!("n-{i:03}")),
                data: json!({"n": i}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            })
            .unwrap();
        }
    }

    #[test]
    fn snapshot_empty_handler_returns_zero_cursor() {
        let h = handler();
        let resp = h.snapshot_entities(SnapshotRequest::default()).unwrap();
        assert!(resp.entities.is_empty());
        assert_eq!(resp.audit_cursor, 0);
        assert!(resp.next_page_token.is_none());
    }

    #[test]
    fn snapshot_returns_all_entities_across_collections() {
        let mut h = handler();
        seed_tasks_and_notes(&mut h);

        let resp = h.snapshot_entities(SnapshotRequest::default()).unwrap();
        assert_eq!(resp.entities.len(), 5);
        assert!(resp.audit_cursor >= 5);
        assert!(resp.next_page_token.is_none());
    }

    #[test]
    fn snapshot_collections_filter_narrows_results() {
        let mut h = handler();
        seed_tasks_and_notes(&mut h);

        let resp = h
            .snapshot_entities(SnapshotRequest {
                collections: Some(vec![CollectionId::new("tasks")]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(resp.entities.len(), 3);
        for e in &resp.entities {
            assert_eq!(e.collection.as_str(), "tasks");
        }
    }

    #[test]
    fn snapshot_cursor_is_race_free_with_post_snapshot_writes() {
        let mut h = handler();
        seed_tasks_and_notes(&mut h);

        let snap = h.snapshot_entities(SnapshotRequest::default()).unwrap();
        let cursor = snap.audit_cursor;
        assert_eq!(snap.entities.len(), 5);

        // New create after the snapshot.
        h.create_entity(CreateEntityRequest {
            collection: CollectionId::new("tasks"),
            id: EntityId::new("t-late"),
            data: json!({"n": 99}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();

        // The audit tail starting at the captured cursor must include the
        // new entity and exclude the snapshotted ones.
        let tail = h
            .query_audit(QueryAuditRequest {
                database: None,
                collection: None,
                collection_ids: Vec::new(),
                entity_id: None,
                actor: None,
                operation: None,
                intent_id: None,
                approval_id: None,
                since_ns: None,
                until_ns: None,
                after_id: Some(cursor),
                limit: None,
            })
            .unwrap();

        let tail_ids: Vec<&str> = tail.entries.iter().map(|e| e.entity_id.as_str()).collect();
        assert!(tail_ids.contains(&"t-late"));
        assert!(!tail_ids.contains(&"t-000"));
        assert!(!tail_ids.contains(&"n-000"));
    }

    #[test]
    fn snapshot_pagination_returns_stable_pages() {
        let mut h = handler();
        seed_tasks_and_notes(&mut h);

        let p1 = h
            .snapshot_entities(SnapshotRequest {
                limit: Some(2),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(p1.entities.len(), 2);
        assert!(p1.next_page_token.is_some());
        let cursor = p1.audit_cursor;

        let p2 = h
            .snapshot_entities(SnapshotRequest {
                limit: Some(2),
                after_page_token: p1.next_page_token.clone(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(p2.entities.len(), 2);
        assert!(p2.next_page_token.is_some());
        assert_eq!(p2.audit_cursor, cursor);

        let p3 = h
            .snapshot_entities(SnapshotRequest {
                limit: Some(2),
                after_page_token: p2.next_page_token.clone(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(p3.entities.len(), 1);
        assert!(p3.next_page_token.is_none());
        assert_eq!(p3.audit_cursor, cursor);

        // Union of pages is exactly the 5 entities we seeded, without
        // duplication.
        let mut keys: Vec<(String, String)> = p1
            .entities
            .iter()
            .chain(p2.entities.iter())
            .chain(p3.entities.iter())
            .map(|e| (e.collection.to_string(), e.id.to_string()))
            .collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 5);
    }

    #[test]
    fn snapshot_page_token_round_trip() {
        let token = encode_snapshot_page_token("tasks", "t-001");
        let (c, i) = decode_snapshot_page_token(&token).unwrap();
        assert_eq!(c, "tasks");
        assert_eq!(i, "t-001");
    }

    #[test]
    fn snapshot_page_token_rejects_malformed_input() {
        // Missing length prefix.
        assert!(decode_snapshot_page_token("no-colon-here").is_err());
        // Invalid length.
        assert!(decode_snapshot_page_token("abc:xyz").is_err());
        // Length past payload end.
        assert!(decode_snapshot_page_token("99:abc").is_err());
    }

    // ── Lifecycle initial-state enforcement (FEAT-015) ───────────────────────

    /// Install `tasks` with a `status` lifecycle: `draft -> submitted -> approved`.
    fn register_tasks_with_status_lifecycle(
        h: &mut AxonHandler<MemoryStorageAdapter>,
    ) -> CollectionId {
        use axon_schema::schema::LifecycleDef;
        use std::collections::HashMap;

        let col = CollectionId::new("tasks");
        let mut transitions = HashMap::new();
        transitions.insert("draft".to_string(), vec!["submitted".to_string()]);
        transitions.insert("submitted".to_string(), vec!["approved".to_string()]);
        transitions.insert("approved".to_string(), vec![]);

        let mut lifecycles = HashMap::new();
        lifecycles.insert(
            "status".to_string(),
            LifecycleDef {
                field: "status".to_string(),
                initial: "draft".to_string(),
                transitions,
            },
        );

        let schema = CollectionSchema {
            collection: col.clone(),
            description: None,
            version: 1,
            entity_schema: None,
            link_types: Default::default(),
            access_control: None,
            gates: Default::default(),
            validation_rules: Default::default(),
            indexes: Default::default(),
            compound_indexes: Default::default(),
            queries: Default::default(),
            lifecycles,
        };

        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema,
                actor: None,
            }),
            "creating tasks collection with lifecycle",
        );
        col
    }

    #[test]
    fn create_auto_populates_lifecycle_initial_state() {
        let mut h = handler();
        let col = register_tasks_with_status_lifecycle(&mut h);

        let created = ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col,
                id: EntityId::new("t-001"),
                data: json!({"title": "design the thing"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity without explicit status",
        );

        assert_eq!(created.entity.data["status"], "draft");
        assert_eq!(created.entity.data["title"], "design the thing");
    }

    #[test]
    fn create_accepts_valid_lifecycle_state() {
        let mut h = handler();
        let col = register_tasks_with_status_lifecycle(&mut h);

        let created = ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col,
                id: EntityId::new("t-001"),
                data: json!({"title": "design", "status": "draft"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity with explicit draft state",
        );

        assert_eq!(created.entity.data["status"], "draft");
    }

    #[test]
    fn create_rejects_invalid_lifecycle_state() {
        let mut h = handler();
        let col = register_tasks_with_status_lifecycle(&mut h);

        let err = err_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col,
                id: EntityId::new("t-001"),
                data: json!({"title": "design", "status": "invalid"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity with unknown status",
        );

        match err {
            AxonError::LifecycleStateInvalid { field, actual } => {
                assert_eq!(field, "status");
                assert_eq!(actual, json!("invalid"));
            }
            other => panic!("expected LifecycleStateInvalid, got {other:?}"),
        }
    }

    #[test]
    fn create_rejects_non_string_lifecycle_value() {
        let mut h = handler();
        let col = register_tasks_with_status_lifecycle(&mut h);

        let err = err_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col,
                id: EntityId::new("t-001"),
                data: json!({"title": "design", "status": 42}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating entity with non-string status",
        );

        match err {
            AxonError::LifecycleStateInvalid { field, actual } => {
                assert_eq!(field, "status");
                assert_eq!(actual, json!(42));
            }
            other => panic!("expected LifecycleStateInvalid, got {other:?}"),
        }
    }

    #[test]
    fn update_requires_lifecycle_field() {
        let mut h = handler();
        let col = register_tasks_with_status_lifecycle(&mut h);

        // Seed with an entity in draft so we have something to update.
        let created = ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                data: json!({"title": "design"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "seeding entity",
        );
        assert_eq!(created.entity.data["status"], "draft");

        // Update without a status field must fail.
        let err = err_or_panic(
            h.update_entity(UpdateEntityRequest {
                collection: col,
                id: EntityId::new("t-001"),
                data: json!({"title": "design revised"}),
                expected_version: created.entity.version,
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "updating entity without status",
        );

        match err {
            AxonError::LifecycleFieldMissing { field } => {
                assert_eq!(field, "status");
            }
            other => panic!("expected LifecycleFieldMissing, got {other:?}"),
        }
    }

    #[test]
    fn update_rejects_invalid_state() {
        let mut h = handler();
        let col = register_tasks_with_status_lifecycle(&mut h);

        let created = ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new("t-001"),
                data: json!({"title": "design"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "seeding entity",
        );

        let err = err_or_panic(
            h.update_entity(UpdateEntityRequest {
                collection: col,
                id: EntityId::new("t-001"),
                data: json!({"title": "design", "status": "not-a-state"}),
                expected_version: created.entity.version,
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "updating entity with unknown status",
        );

        match err {
            AxonError::LifecycleStateInvalid { field, actual } => {
                assert_eq!(field, "status");
                assert_eq!(actual, json!("not-a-state"));
            }
            other => panic!("expected LifecycleStateInvalid, got {other:?}"),
        }
    }

    #[test]
    fn create_with_no_lifecycle_ignores_field_check() {
        let mut h = handler();
        let col = CollectionId::new("notes");

        // Collection with an empty lifecycles map.
        ok_or_panic(
            h.create_collection(CreateCollectionRequest {
                name: col.clone(),
                schema: CollectionSchema::new(col.clone()),
                actor: None,
            }),
            "creating notes collection",
        );

        // Create and update with arbitrary data must succeed: the lifecycle
        // enforcement check has nothing to enforce.
        let created = ok_or_panic(
            h.create_entity(CreateEntityRequest {
                collection: col.clone(),
                id: EntityId::new("n-001"),
                data: json!({"body": "hello", "status": "whatever"}),
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "creating note",
        );
        assert_eq!(created.entity.data["status"], "whatever");

        ok_or_panic(
            h.update_entity(UpdateEntityRequest {
                collection: col,
                id: EntityId::new("n-001"),
                data: json!({"body": "updated"}),
                expected_version: created.entity.version,
                actor: None,
                audit_metadata: None,
                attribution: None,
            }),
            "updating note",
        );
    }
}
