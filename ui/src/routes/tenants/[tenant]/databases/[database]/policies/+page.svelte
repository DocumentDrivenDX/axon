<script lang="ts">
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
	buildSchemaDiagnostics,
	dedupeDiagnostics,
	defaultCollectionName,
	defaultPatchFixture,
	defaultTransactionFixture,
	errorMessage,
	formatDiagnosticError,
	// biome-ignore lint/correctness/noUnusedImports: used only in the template.
	formatFields,
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
let transactionFixtureText = $state('[]');
let impactMatrix = $state<ImpactCell[]>([]);
let impactMatrixSubjects = $state<SubjectOption[]>([]);
let impactMatrixEntities = $state<EntityRecord[]>([]);
let impactMatrixError = $state<string | null>(null);
let loadingImpactMatrix = $state(false);
let collectionContextToken = 0;
let evaluationToken = 0;
let impactMatrixToken = 0;

const selectedCollectionSummary = $derived(
	collections.find((collection) => collection.name === selectedCollection) ?? null,
);
const selectedSubjectOption = $derived(
	subjects.find((subject) => subject.id === selectedSubject) ?? null,
);
const selectedEntity = $derived(
	collectionEntities.find((entity) => entity.id === selectedEntityId) ?? null,
);
const schemaVersionLabel = $derived(
	selectedCollectionSummary?.schema_version
		? `v${selectedCollectionSummary.schema_version}`
		: 'No schema',
);
const policyVersionLabel = $derived(
	effectivePolicy
		? `v${effectivePolicy.policyVersion}`
		: explanation
			? `v${explanation.policyVersion}`
			: 'Not loaded',
);
const sampleEntityLabel = $derived(
	selectedEntity ? `${selectedEntity.collection}/${selectedEntity.id}` : 'No sample entity',
);
const sampleRowJson = $derived(prettyJson(selectedEntity?.data ?? {}));
const requiresEntity = $derived(operationRequiresEntity(selectedOperation));
const requiresExpectedVersion = $derived(operationRequiresExpectedVersion(selectedOperation));
const consolePresetContext = $derived({
	baseHref: graphqlConsoleBaseHref,
	subject: selectedSubject,
});
const schemaDiagnostics = $derived(buildSchemaDiagnostics(collectionDetail, selectedOperation));
const matrixSchemaDiagnostics = $derived(buildSchemaDiagnostics(collectionDetail, 'read'));
const graphqlDiagnostics = $derived(buildGraphqlDiagnostics(explanationDiagnostics));
const evaluatorDiagnostics = $derived(
	dedupeDiagnostics([...schemaDiagnostics, ...graphqlDiagnostics]),
);
const explainConsolePreview = $derived(tryBuildExplainInput(currentExplainArgs(selectedEntity)));
const effectiveConsolePreset = $derived(
	buildEffectiveConsolePreset(consolePresetContext, selectedCollection, selectedEntity),
);
const explainConsolePreset = $derived(
	buildExplainConsolePreset(consolePresetContext, explainConsolePreview.input),
);

function defaultSubjectId(nextSubjects: SubjectOption[]): string {
	return (
		nextSubjects.find((subject) => subject.id === 'finance-agent')?.id ??
		nextSubjects.find((subject) => subject.id === 'requester')?.id ??
		nextSubjects.find((subject) => subject.id === 'contractor')?.id ??
		nextSubjects[0]?.id ??
		'guest'
	);
}

async function loadSubjectOptions(nextCollections: CollectionSummary[]): Promise<SubjectOption[]> {
	const subjectCollection = nextCollections.find((collection) =>
		['user', 'users'].includes(collection.name),
	)?.name;
	if (!subjectCollection) {
		return [{ id: 'guest', label: 'Current caller', detail: null }];
	}

	try {
		const result = await fetchEntities(subjectCollection, { limit: 100 }, scope);
		const options = result.entities
			.map((entity) => {
				const userId = typeof entity.data.user_id === 'string' ? entity.data.user_id : entity.id;
				const label =
					typeof entity.data.display_name === 'string' ? entity.data.display_name : userId;
				const detailParts = [
					typeof entity.data.approval_role === 'string' ? entity.data.approval_role : null,
					typeof entity.data.procurement_role === 'string' ? entity.data.procurement_role : null,
				].filter((value): value is string => Boolean(value));
				return {
					id: userId,
					label,
					detail: detailParts.length ? detailParts.join(' / ') : null,
				};
			})
			.sort((left, right) => left.label.localeCompare(right.label));

		return options.length ? options : [{ id: 'guest', label: 'Current caller', detail: null }];
	} catch {
		return [{ id: 'guest', label: 'Current caller', detail: null }];
	}
}

function seedEditorFixtures(entity: EntityRecord | null) {
	expectedVersionText = entity ? String(entity.version) : '';
	rollbackVersionText = entity ? String(Math.max(0, entity.version - 1)) : '';
	lifecycleName =
		entity && typeof entity.data.status === 'string'
			? 'status'
			: entity && typeof entity.data.state === 'string'
				? 'state'
				: 'status';
	targetState = 'approved';
	dataFixtureText = prettyJson(entity?.data ?? {});
	patchFixtureText = prettyJson(defaultPatchFixture(entity));
	transactionFixtureText = prettyJson(defaultTransactionFixture(entity));
}

function actorHeaders() {
	return selectedSubject ? { headers: { 'x-axon-actor': selectedSubject } } : {};
}

function actorOptions() {
	return selectedSubject ? { actor: selectedSubject } : {};
}

function currentExplainArgs(entity: EntityRecord | null) {
	return {
		operation: selectedOperation,
		collection: selectedCollection,
		entity,
		expectedVersionText,
		rollbackVersionText,
		lifecycleName,
		targetState,
		dataFixtureText,
		patchFixtureText,
		transactionFixtureText,
	};
}

async function runPolicyEvaluation(entity: EntityRecord | null = selectedEntity) {
	if (!selectedCollection) return;

	const token = ++evaluationToken;
	loadingEffectivePolicy = true;
	loadingExplanation = true;
	policyError = null;
	explanationError = null;
	effectivePolicy = null;
	explanation = null;
	explanationDiagnostics = [];

	const built = tryBuildExplainInput(currentExplainArgs(entity));
	if (built.error) {
		explanationError = built.error;
	}
	const nextInput = built.input;

	const [effectiveResult, explanationResult] = await Promise.allSettled([
		fetchEffectivePolicy(selectedCollection, scope, {
			entityId: entity?.id ?? null,
			...actorOptions(),
		}),
		nextInput ? explainPolicyDetailed(nextInput, scope, actorOptions()) : Promise.resolve(null),
	]);

	if (token !== evaluationToken) return;

	if (effectiveResult.status === 'fulfilled') {
		effectivePolicy = effectiveResult.value;
	} else {
		policyError = errorMessage(effectiveResult.reason, 'Failed to load effective policy');
	}

	if (nextInput && explanationResult.status === 'fulfilled' && explanationResult.value) {
		explanation = explanationResult.value.explanation;
		explanationDiagnostics = explanationResult.value.diagnostics;
		if (!explanation && explanationDiagnostics.length) {
			explanationError = explanationDiagnostics.map(formatDiagnosticError).join(', ');
		}
	} else if (nextInput && explanationResult.status === 'rejected') {
		explanationError = errorMessage(explanationResult.reason, 'Failed to explain policy');
	}

	loadingEffectivePolicy = false;
	loadingExplanation = false;
}

async function loadCollectionContext(resetEditors = true) {
	if (!selectedCollection) return;

	const token = ++collectionContextToken;
	loadingCollectionContext = true;
	collectionContextError = null;

	const [detailResult, entitiesResult] = await Promise.allSettled([
		fetchCollection(selectedCollection, scope),
		fetchEntities(selectedCollection, { limit: 25 }, scope, actorHeaders()),
	]);

	if (token !== collectionContextToken) return;

	collectionDetail = detailResult.status === 'fulfilled' ? detailResult.value : null;
	collectionEntities = entitiesResult.status === 'fulfilled' ? entitiesResult.value.entities : [];

	if (detailResult.status === 'rejected') {
		collectionContextError = errorMessage(detailResult.reason, 'Failed to load collection schema');
	}
	if (entitiesResult.status === 'rejected') {
		collectionContextError = collectionContextError
			? `${collectionContextError}; ${errorMessage(entitiesResult.reason, 'Failed to load entities')}`
			: errorMessage(entitiesResult.reason, 'Failed to load entities');
	}

	const nextEntity =
		collectionEntities.find((entity) => entity.id === selectedEntityId) ??
		collectionEntities[0] ??
		null;
	selectedEntityId = nextEntity?.id ?? '';
	if (resetEditors) {
		seedEditorFixtures(nextEntity);
	}

	loadingCollectionContext = false;
	await runPolicyEvaluation(nextEntity);
	void loadImpactMatrix();
}

async function loadImpactMatrix() {
	if (!selectedCollection) {
		impactMatrix = [];
		impactMatrixSubjects = [];
		impactMatrixEntities = [];
		return;
	}

	const matrixSubjects = subjects.slice(0, IMPACT_MATRIX_SUBJECT_LIMIT);
	const matrixEntities = collectionEntities.slice(0, IMPACT_MATRIX_ENTITY_LIMIT);
	if (matrixSubjects.length === 0 || matrixEntities.length === 0) {
		impactMatrix = [];
		impactMatrixSubjects = matrixSubjects;
		impactMatrixEntities = matrixEntities;
		return;
	}

	const token = ++impactMatrixToken;
	loadingImpactMatrix = true;
	impactMatrixError = null;
	impactMatrixSubjects = matrixSubjects;
	impactMatrixEntities = matrixEntities;

	const requests: ImpactMatrixRequest[] = buildImpactMatrixInputs(
		selectedCollection,
		matrixEntities,
		matrixSubjects,
		IMPACT_MATRIX_OPERATIONS,
	);

	const explainResults = await Promise.allSettled(
		requests.map((request) =>
			explainPolicyDetailed(request.explainInput, scope, { actor: request.subjectId }),
		),
	);

	const effectiveByKey = new Map<string, EffectiveCollectionPolicy>();
	const effectiveKeys: Array<{ subjectId: string; entityId: string; key: string }> = [];
	for (const subject of matrixSubjects) {
		for (const entity of matrixEntities) {
			effectiveKeys.push({
				subjectId: subject.id,
				entityId: entity.id,
				key: `${subject.id}|${entity.id}`,
			});
		}
	}
	const effectiveResults = await Promise.allSettled(
		effectiveKeys.map(({ subjectId, entityId }) =>
			fetchEffectivePolicy(selectedCollection, scope, {
				entityId,
				actor: subjectId,
			}),
		),
	);
	effectiveResults.forEach((result, index) => {
		const meta = effectiveKeys[index];
		if (!meta) return;
		if (result.status === 'fulfilled') {
			effectiveByKey.set(meta.key, result.value);
		}
	});

	if (token !== impactMatrixToken) return;

	const cells: ImpactCell[] = requests.map((request, index) => {
		const explainPromise = explainResults[index];
		const explainResult =
			explainPromise && explainPromise.status === 'fulfilled' ? explainPromise.value : null;
		const effective = effectiveByKey.get(`${request.subjectId}|${request.entity.id}`) ?? null;
		return resolveImpactCell({
			request,
			explainResult,
			effective,
			presetCtxForSubject: {
				baseHref: graphqlConsoleBaseHref,
				subject: request.subjectId,
			},
		});
	});

	const failureCount = explainResults.filter((r) => r.status === 'rejected').length;
	if (failureCount === explainResults.length && explainResults.length > 0) {
		impactMatrixError = 'Failed to load any impact matrix cells.';
	} else if (failureCount > 0) {
		impactMatrixError = `Failed to load ${failureCount} of ${explainResults.length} matrix cells.`;
	}

	impactMatrix = cells;
	loadingImpactMatrix = false;
}

async function loadRouteShell() {
	loadingShell = true;
	shellError = null;

	try {
		const nextCollections = await fetchCollections(scope);
		const nextSubjects = await loadSubjectOptions(nextCollections);
		collections = nextCollections;
		subjects = nextSubjects;
		selectedCollection = defaultCollectionName(nextCollections);
		selectedSubject = defaultSubjectId(nextSubjects);
	} catch (error) {
		shellError = errorMessage(error, 'Failed to load policy route shell');
	} finally {
		loadingShell = false;
	}

	if (selectedCollection) {
		await loadCollectionContext(true);
	}
}

function handleCollectionChange(event: Event) {
	selectedCollection = (event.currentTarget as HTMLSelectElement).value;
	void loadCollectionContext(true);
}

function handleSubjectChange(event: Event) {
	selectedSubject = (event.currentTarget as HTMLSelectElement).value;
	void loadCollectionContext(true);
}

function handleEntityChange(event: Event) {
	selectedEntityId = (event.currentTarget as HTMLSelectElement).value;
	seedEditorFixtures(selectedEntity);
	void runPolicyEvaluation(selectedEntity);
}

function handleOperationChange(event: Event) {
	selectedOperation = (event.currentTarget as HTMLSelectElement).value as EvaluationOperation;
	void runPolicyEvaluation(selectedEntity);
}

function resetFixtureFromSampleRow() {
	seedEditorFixtures(selectedEntity);
	void runPolicyEvaluation(selectedEntity);
}

onMount(() => {
	void loadRouteShell();
});
</script>

<div class="page-header">
	<div>
		<h1>Policies</h1>
		<p class="muted" data-testid="policy-scope">
			Inspect live GraphQL policy outcomes for <strong>{scopeLabel}</strong>.
		</p>
	</div>
</div>

{#if loadingShell}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">Loading policy collections and subject options…</p>
		</div>
	</section>
{:else if shellError}
	<section class="panel">
		<div class="panel-body">
			<p class="message error">{shellError}</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Scope</h2>
			{#if loadingCollectionContext}
				<span class="muted">Refreshing entity scope…</span>
			{/if}
		</div>
		<div class="panel-body stack">
			{#if collectionContextError}
				<p class="message error">{collectionContextError}</p>
			{/if}

			<div class="controls-grid">
				<label class="control">
					<span>Collection</span>
					<select
						id="policy-collection"
						data-testid="policy-collection-picker"
						bind:value={selectedCollection}
						onchange={handleCollectionChange}
					>
						{#each collections as collection}
							<option value={collection.name}>{collection.name}</option>
						{/each}
					</select>
				</label>

				<label class="control">
					<span>Subject</span>
					<select
						id="policy-subject"
						data-testid="policy-subject-picker"
						bind:value={selectedSubject}
						onchange={handleSubjectChange}
					>
						{#each subjects as subject}
							<option value={subject.id}>
								{subject.label}{subject.detail ? ` · ${subject.detail}` : ''}
							</option>
						{/each}
					</select>
				</label>

				<label class="control">
					<span>Selected entity</span>
					<select
						id="policy-entity"
						data-testid="policy-entity-picker"
						bind:value={selectedEntityId}
						onchange={handleEntityChange}
					>
						<option value="">Collection scope</option>
						{#each collectionEntities as entity}
							<option value={entity.id}>{entity.id} · v{entity.version}</option>
						{/each}
					</select>
				</label>

				<div class="control static">
					<span>Sample entity</span>
					<div class="value" data-testid="policy-sample-entity">{sampleEntityLabel}</div>
				</div>

				<div class="control static">
					<span>Active schema version</span>
					<div class="value" data-testid="policy-schema-version">{schemaVersionLabel}</div>
				</div>

				<div class="control static">
					<span>Active policy version</span>
					<div class="value" data-testid="policy-version">{policyVersionLabel}</div>
				</div>

				<div class="control static">
					<span>Subject detail</span>
					<div class="value">{selectedSubjectOption?.detail ?? 'Header-scoped actor'}</div>
				</div>
			</div>
		</div>
	</section>

	<section class="panel">
		<div class="panel-header">
			<h2>Evaluator</h2>
			<div class="actions">
				{#if effectiveConsolePreset}
					<a
						class="console-link"
						data-testid="policy-open-effective-graphql"
						href={effectiveConsolePreset.href}
					>
						Open effectivePolicy in GraphQL
					</a>
				{/if}
				{#if explainConsolePreset}
					<a
						class="console-link"
						data-testid="policy-open-explain-graphql"
						href={explainConsolePreset.href}
					>
						Open explainPolicy in GraphQL
					</a>
				{/if}
				<button type="button" data-testid="policy-reset-fixture" onclick={resetFixtureFromSampleRow}>
					Reset from sample row
				</button>
				<button
					type="button"
					class="primary"
					data-testid="policy-run-evaluator"
					onclick={() => void runPolicyEvaluation(selectedEntity)}
				>
					Run evaluator
				</button>
			</div>
		</div>
		<div class="panel-body stack">
			{#if !explainConsolePreset && explainConsolePreview.error}
				<p class="muted" data-testid="policy-graphql-link-hint">
					{explainConsolePreview.error}
				</p>
			{/if}
			<div class="controls-grid">
				<label class="control">
					<span>Operation</span>
					<select
						id="policy-operation"
						data-testid="policy-operation-picker"
						bind:value={selectedOperation}
						onchange={handleOperationChange}
					>
						{#each operationOptions as option}
							<option value={option.value}>{option.label}</option>
						{/each}
					</select>
				</label>

				{#if requiresExpectedVersion}
					<label class="control">
						<span>Expected version</span>
						<input
							type="text"
							inputmode="numeric"
							data-testid="policy-expected-version"
							bind:value={expectedVersionText}
						/>
					</label>
				{/if}

				{#if selectedOperation === 'rollback'}
					<label class="control">
						<span>Rollback version</span>
						<input
							type="text"
							inputmode="numeric"
							data-testid="policy-rollback-version"
							bind:value={rollbackVersionText}
						/>
					</label>
				{/if}

				{#if selectedOperation === 'transition'}
					<label class="control">
						<span>Lifecycle</span>
						<input type="text" data-testid="policy-lifecycle-name" bind:value={lifecycleName} />
					</label>

					<label class="control">
						<span>Target state</span>
						<input type="text" data-testid="policy-target-state" bind:value={targetState} />
					</label>
				{/if}

				<div class="control static">
					<span>JSON fixture mode</span>
					<div class="value">Selected entity + sample row editor</div>
				</div>
			</div>

			<div class="fixture-grid">
				<section class="fixture-card">
					<div class="fixture-header">
						<div>
							<h3>Sample Row</h3>
							<p class="muted">Live entity payload copied into the fixture editors.</p>
						</div>
					</div>
					<pre data-testid="policy-sample-row">{sampleRowJson}</pre>
				</section>

				{#if selectedOperation === 'create' || selectedOperation === 'update'}
					<section class="fixture-card">
						<div class="fixture-header">
							<div>
								<h3>JSON Fixture</h3>
								<p class="muted">Edit the full entity body used for create/update evaluation.</p>
							</div>
						</div>
						<textarea
							class="fixture-editor"
							data-testid="policy-data-fixture"
							rows="14"
							spellcheck="false"
							bind:value={dataFixtureText}
						></textarea>
					</section>
				{/if}

				{#if selectedOperation === 'patch'}
					<section class="fixture-card">
						<div class="fixture-header">
							<div>
								<h3>Patch Fixture</h3>
								<p class="muted">Edit the JSON patch applied to the selected entity.</p>
							</div>
						</div>
						<textarea
							class="fixture-editor"
							data-testid="policy-patch-fixture"
							rows="14"
							spellcheck="false"
							bind:value={patchFixtureText}
						></textarea>
					</section>
				{/if}

				{#if selectedOperation === 'transaction'}
					<section class="fixture-card fixture-card-wide">
						<div class="fixture-header">
							<div>
								<h3>Transaction Fixture</h3>
								<p class="muted">Provide the operations array exactly as the GraphQL evaluator expects it.</p>
							</div>
						</div>
						<textarea
							class="fixture-editor"
							data-testid="policy-transaction-fixture"
							rows="16"
							spellcheck="false"
							bind:value={transactionFixtureText}
						></textarea>
					</section>
				{/if}
			</div>
		</div>
	</section>

	<div class="policy-grid">
		<section class="panel">
			<div class="panel-header">
				<h2>Effective Policy</h2>
			</div>
			<div class="panel-body stack">
				{#if loadingEffectivePolicy}
					<p class="muted">Loading effective policy…</p>
				{:else if policyError}
					<p class="message error">{policyError}</p>
				{:else if effectivePolicy}
					<div class="capability-grid" data-testid="policy-capabilities">
						<div class="capability">
							<span class="label">Read</span>
							<strong>{effectivePolicy.canRead ? 'Allowed' : 'Denied'}</strong>
						</div>
						<div class="capability">
							<span class="label">Create</span>
							<strong>{effectivePolicy.canCreate ? 'Allowed' : 'Denied'}</strong>
						</div>
						<div class="capability">
							<span class="label">Update</span>
							<strong>{effectivePolicy.canUpdate ? 'Allowed' : 'Denied'}</strong>
						</div>
						<div class="capability">
							<span class="label">Delete</span>
							<strong>{effectivePolicy.canDelete ? 'Allowed' : 'Denied'}</strong>
						</div>
					</div>

					<div class="field-list">
						<div>
							<span class="label">Redacted fields</span>
							<p data-testid="policy-redacted-fields">{formatFields(effectivePolicy.redactedFields)}</p>
						</div>
						<div>
							<span class="label">Denied fields</span>
							<p data-testid="policy-denied-fields">{formatFields(effectivePolicy.deniedFields)}</p>
						</div>
					</div>
				{:else}
					<p class="muted">Choose a collection to inspect policy capabilities.</p>
				{/if}
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Policy Explanation</h2>
			</div>
			<div class="panel-body stack">
				{#if loadingExplanation}
					<p class="muted">Explaining policy for the selected subject…</p>
				{:else if explanationError}
					<p class="message error">{explanationError}</p>
				{/if}

				{#if explanation}
					<div class="stack" data-testid="policy-explanation">
						<div class="explanation-row">
							<span class="label">Decision</span>
							<strong>{explanation.decision}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Reason Code</span>
							<strong data-testid="policy-reason-code">{explanation.reason}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Stable Rule IDs</span>
							<strong data-testid="policy-rule-ids">{formatFields(explanation.ruleIds)}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Matching rules</span>
							<strong>{explanation.rules.map((rule) => rule.name).join(', ') || 'None'}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Field paths</span>
							<strong data-testid="policy-field-paths">{formatFields(explanation.fieldPaths)}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Denied fields</span>
							<strong>{formatFields(explanation.deniedFields)}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Redacted fields</span>
							<strong>{formatFields(effectivePolicy?.redactedFields ?? [])}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Policy version</span>
							<strong>v{explanation.policyVersion}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Required approver role</span>
							<strong data-testid="policy-approval-role">
								{explanation.approval?.role ?? 'No approval route'}
							</strong>
						</div>

						{#if explanation.operations.length}
							<div class="explanation-row">
								<span class="label">Transaction operations</span>
								<div class="operation-list" data-testid="policy-transaction-operations">
									{#each explanation.operations as child}
										<div class="operation-card">
											<strong>
												{child.operation}#{child.operationIndex ?? 0} · {child.decision}
											</strong>
											<span>{child.reason}</span>
											<span>{formatFields(child.ruleIds)}</span>
										</div>
									{/each}
								</div>
							</div>
						{/if}
					</div>
				{:else if !loadingExplanation && !explanationError}
					<p class="muted">Choose an operation and run the evaluator.</p>
				{/if}
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Diagnostics</h2>
			</div>
			<div class="panel-body stack">
				{#if evaluatorDiagnostics.length}
					<div class="stack" data-testid="policy-diagnostics">
						{#each evaluatorDiagnostics as diagnostic}
							<div class="diagnostic-card">
								<div class="diagnostic-header">
									<strong>{diagnostic.code}</strong>
									<span class="pill">{diagnostic.source}</span>
								</div>
								<p>{diagnostic.summary}</p>
								<p class="muted">{diagnostic.remediation}</p>
							</div>
						{/each}
					</div>
				{:else}
					<p class="muted" data-testid="policy-diagnostics-empty">
						No missing-index or dry-run diagnostics for the current evaluator state.
					</p>
				{/if}
			</div>
		</section>
	</div>

	<section class="panel" data-testid="policy-impact-matrix">
		<div class="panel-header">
			<h2>Impact Matrix</h2>
			<div class="actions">
				{#if loadingImpactMatrix}
					<span class="muted">Loading impact matrix…</span>
				{/if}
				<button
					type="button"
					data-testid="policy-impact-matrix-refresh"
					onclick={() => void loadImpactMatrix()}
				>
					Refresh
				</button>
			</div>
		</div>
		<div class="panel-body stack">
			<p class="muted">
				Active-policy outcomes for the first {impactMatrixEntities.length} sample row(s) across
				up to {impactMatrixSubjects.length} subjects and {IMPACT_MATRIX_OPERATIONS.length} operations.
			</p>
			{#if impactMatrixError}
				<p class="message error" data-testid="policy-impact-matrix-error">{impactMatrixError}</p>
			{/if}
			{#if matrixSchemaDiagnostics.length}
				<div class="stack">
					{#each matrixSchemaDiagnostics as diagnostic}
						<div class="impact-diagnostic" data-testid="policy-impact-matrix-diagnostic">
							<strong>{diagnostic.code}</strong>
							<p>{diagnostic.summary}</p>
							<p class="muted">{diagnostic.remediation}</p>
						</div>
					{/each}
				</div>
			{/if}
			{#if impactMatrixEntities.length === 0}
				<p class="muted" data-testid="policy-impact-matrix-empty">
					No sample rows available for the selected collection.
				</p>
			{:else}
				{#each impactMatrixEntities as entity (entity.id)}
					<div
						class="impact-matrix-entity"
						data-testid="policy-impact-matrix-entity"
						data-entity-id={entity.id}
					>
						<h3 data-testid="policy-impact-matrix-entity-label">
							{entity.collection}/{entity.id} · v{entity.version}
						</h3>
						<table class="impact-matrix-table">
							<thead>
								<tr>
									<th scope="col">Operation</th>
									{#each impactMatrixSubjects as subject}
										<th scope="col" data-testid="policy-impact-matrix-subject-header">
											{subject.label}
										</th>
									{/each}
								</tr>
							</thead>
							<tbody>
								{#each IMPACT_MATRIX_OPERATIONS as operation}
									<tr>
										<th scope="row">{operation}</th>
										{#each impactMatrixSubjects as subject}
											{@const cell = impactMatrix.find(
												(c) =>
													c.entityId === entity.id &&
													c.subjectId === subject.id &&
													c.operation === operation,
											)}
											<td
												data-testid="policy-impact-matrix-cell"
												data-entity-id={entity.id}
												data-subject-id={subject.id}
												data-operation={operation}
												data-decision={cell?.decision ?? 'pending'}
											>
												{#if cell}
													<div
														class="impact-decision impact-decision-{cell.decision}"
														data-testid="policy-impact-matrix-decision"
													>
														{cell.decision}
													</div>
													<div
														class="muted impact-reason"
														data-testid="policy-impact-matrix-reason"
													>
														{cell.reason}
													</div>
													{#if cell.approvalRole}
														<div
															class="muted impact-approval"
															data-testid="policy-impact-matrix-approval-role"
														>
															approver: {cell.approvalRole}
														</div>
													{/if}
													{#if cell.redactedFields.length}
														<div
															class="impact-fields"
															data-testid="policy-impact-matrix-redacted-fields"
														>
															redacted: {formatFields(cell.redactedFields)}
														</div>
													{/if}
													{#if cell.deniedFields.length}
														<div
															class="impact-fields"
															data-testid="policy-impact-matrix-denied-fields"
														>
															denied: {formatFields(cell.deniedFields)}
														</div>
													{/if}
													{#if cell.diagnostic}
														<div
															class="impact-diagnostic"
															data-testid="policy-impact-matrix-diagnostic"
														>
															<strong>{cell.diagnostic.code}</strong>
															<p>{cell.diagnostic.remediation}</p>
														</div>
													{/if}
													{#if cell.explainHref}
														<a
															class="impact-link"
															href={cell.explainHref}
															data-testid="policy-impact-matrix-open-graphql"
														>
															Open explainPolicy
														</a>
													{/if}
												{:else}
													<span class="muted">…</span>
												{/if}
											</td>
										{/each}
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/each}
			{/if}
		</div>
	</section>
{/if}

<style>
	.controls-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
		gap: 0.9rem;
	}

	.control {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.control span,
	.label {
		font-size: 0.78rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted);
	}

	.control select,
	.control input,
	.control .value,
	.fixture-editor {
		min-height: 2.5rem;
		padding: 0.6rem 0.75rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.5rem;
		background: rgba(6, 10, 18, 0.8);
		color: var(--text);
		font-size: 0.92rem;
	}

	.control.static .value {
		display: flex;
		align-items: center;
	}

	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
	}

	.console-link {
		display: inline-flex;
		align-items: center;
		padding: 0.55rem 0.8rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.5rem;
		background: rgba(255, 255, 255, 0.03);
		color: var(--text);
		text-decoration: none;
		font-size: 0.9rem;
	}

	.fixture-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr));
		gap: 1rem;
	}

	.fixture-card {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		padding: 1rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.75rem;
		background: rgba(255, 255, 255, 0.02);
	}

	.fixture-card-wide {
		grid-column: 1 / -1;
	}

	.fixture-header {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
	}

	.fixture-header h3,
	.diagnostic-card p,
	pre {
		margin: 0;
	}

	.fixture-editor {
		width: 100%;
		min-height: 12rem;
		font-family: 'SFMono-Regular', 'SF Mono', 'Consolas', monospace;
		resize: vertical;
	}

	pre {
		padding: 0.85rem;
		border-radius: 0.5rem;
		background: rgba(6, 10, 18, 0.8);
		overflow-x: auto;
		font-size: 0.82rem;
		line-height: 1.45;
	}

	.policy-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(20rem, 1fr));
		gap: 1rem;
	}

	.stack {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	.capability-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 0.75rem;
	}

	.capability {
		padding: 0.85rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
		background: rgba(255, 255, 255, 0.02);
	}

	.capability strong,
	.explanation-row strong {
		font-size: 0.95rem;
	}

	.field-list {
		display: grid;
		gap: 0.85rem;
	}

	.field-list p {
		margin: 0.35rem 0 0;
		font-size: 0.92rem;
	}

	.explanation-row {
		display: grid;
		gap: 0.25rem;
		padding: 0.75rem 0;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
	}

	.explanation-row:first-child {
		padding-top: 0;
		border-top: 0;
	}

	.operation-list {
		display: grid;
		gap: 0.75rem;
	}

	.operation-card,
	.diagnostic-card {
		display: grid;
		gap: 0.35rem;
		padding: 0.85rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
		background: rgba(255, 255, 255, 0.02);
	}

	.diagnostic-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 0.75rem;
	}

	.pill {
		padding: 0.2rem 0.55rem;
		border-radius: 999px;
		background: rgba(255, 255, 255, 0.08);
		font-size: 0.78rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.impact-matrix-entity {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.impact-matrix-entity h3 {
		margin: 0;
		font-size: 0.95rem;
	}

	.impact-matrix-table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.85rem;
	}

	.impact-matrix-table th,
	.impact-matrix-table td {
		padding: 0.55rem 0.65rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		vertical-align: top;
		text-align: left;
	}

	.impact-matrix-table thead th {
		background: rgba(255, 255, 255, 0.04);
		font-size: 0.78rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.impact-decision {
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		font-size: 0.78rem;
		margin-bottom: 0.4rem;
	}

	.impact-decision-allowed {
		color: #4ade80;
	}

	.impact-decision-denied {
		color: #f87171;
	}

	.impact-decision-needs_approval {
		color: #fbbf24;
	}

	.impact-decision-error {
		color: #cbd5f5;
	}

	.impact-approval,
	.impact-fields,
	.impact-reason {
		font-size: 0.78rem;
		margin-top: 0.25rem;
	}

	.impact-diagnostic {
		margin-top: 0.4rem;
		padding: 0.45rem 0.55rem;
		border-radius: 0.4rem;
		background: rgba(248, 113, 113, 0.12);
	}

	.impact-diagnostic strong {
		font-size: 0.78rem;
		display: block;
		margin-bottom: 0.2rem;
	}

	.impact-diagnostic p {
		margin: 0;
		font-size: 0.78rem;
	}

	.impact-link {
		display: inline-block;
		margin-top: 0.4rem;
		font-size: 0.8rem;
		color: var(--text);
	}

	@media (max-width: 720px) {
		.capability-grid {
			grid-template-columns: 1fr;
		}

		.fixture-header {
			flex-direction: column;
		}

		.impact-matrix-table {
			font-size: 0.78rem;
		}
	}
</style>
