import { base } from '$app/paths';
import {
	type CollectionDetail,
	type CollectionSummary,
	type EffectiveCollectionPolicy,
	type EntityRecord,
	type PolicyExplainDiagnostic,
	type PolicyExplanation,
	explainPolicyDetailed,
	fetchCollection,
	fetchCollections,
	fetchEffectivePolicy,
	fetchEntities,
} from '$lib/api';
import {
	type EvaluationOperation,
	IMPACT_MATRIX_ENTITY_LIMIT,
	IMPACT_MATRIX_OPERATIONS,
	IMPACT_MATRIX_SUBJECT_LIMIT,
	type ImpactCell,
	type ImpactMatrixRequest,
	type SubjectOption,
	buildEffectiveConsolePreset,
	buildExplainConsolePreset,
	buildGraphqlDiagnostics,
	buildImpactMatrixInputs,
	buildMcpBridgePreset,
	buildMcpEnvelopeComparison,
	buildMcpEnvelopePreview,
	buildSchemaDiagnostics,
	dedupeDiagnostics,
	defaultCollectionName,
	defaultPatchFixture,
	defaultTransactionFixture,
	errorMessage,
	formatDiagnosticError,
	// biome-ignore lint/correctness/noUnusedImports: used only in the template.
	formatFields,
	formatMcpReproduction,
	operationRequiresEntity,
	operationRequiresExpectedVersion,
	prettyJson,
	resolveImpactCell,
	tryBuildExplainInput,
} from '$lib/policy-evaluator';
import { onMount } from 'svelte';
import type { PageData } from './$types';

const operationOptions: Array<{ value: EvaluationOperation; label: string }> = [
	{ value: 'read', label: 'Read' },
	{ value: 'create', label: 'Create' },
	{ value: 'update', label: 'Update' },
	{ value: 'patch', label: 'Patch' },
	{ value: 'delete', label: 'Delete' },
	{ value: 'transition', label: 'Transition' },
	{ value: 'rollback', label: 'Rollback' },
	{ value: 'transaction', label: 'Transaction' },
];

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const scopeLabel = $derived(`${data.tenant.db_name} / ${data.database.name}`);
const graphqlConsoleBaseHref = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}/graphql`,
);

let collections = $state<CollectionSummary[]>([]);
let subjects = $state<SubjectOption[]>([]);
let collectionDetail = $state<CollectionDetail | null>(null);
let collectionEntities = $state<EntityRecord[]>([]);
let selectedCollection = $state('');
let selectedSubject = $state('');
let selectedEntityId = $state('');
let selectedOperation = $state<EvaluationOperation>('read');
let effectivePolicy = $state<EffectiveCollectionPolicy | null>(null);
let explanation = $state<PolicyExplanation | null>(null);
let explanationDiagnostics = $state<PolicyExplainDiagnostic[]>([]);
let loadingShell = $state(true);
let loadingCollectionContext = $state(false);
let loadingEffectivePolicy = $state(false);
let loadingExplanation = $state(false);
let shellError = $state<string | null>(null);
let collectionContextError = $state<string | null>(null);
let policyError = $state<string | null>(null);
let explanationError = $state<string | null>(null);
let expectedVersionText = $state('');
let rollbackVersionText = $state('');
let lifecycleName = $state('status');
let targetState = $state('approved');
let dataFixtureText = $state('{}');
let patchFixtureText = $state('{}');
let impactMatrix = $state<ImpactCell[]>([]);
let impactMatrixSubjects = $state<SubjectOption[]>([]);
let impactMatrixEntities = $state<EntityRecord[]>([]);
let impactMatrixError = $state<string | null>(null);
let loadingImpactMatrix = $state(false);
let proposedImpactMatrix = $state<ImpactCell[]>([]);
let proposedImpactMatrixError = $state<string | null>(null);
let loadingProposedImpactMatrix = $state(false);
let transactionFixtureText = $state('[]');
let evaluationToken = 0;
let collectionContextToken = 0;
let impactMatrixToken = 0;

