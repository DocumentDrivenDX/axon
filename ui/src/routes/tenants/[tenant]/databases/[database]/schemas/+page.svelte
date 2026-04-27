<script lang="ts">
import { base } from '$app/paths';
import { page } from '$app/state';
import {
	type CollectionSchema,
	type CollectionSummary,
	type DryRunExplanation,
	type PolicyCompileReport,
	type SchemaDryRunExplainInput,
	type SchemaPolicyDryRunResult,
	type SchemaPreviewResult,
	createCollection,
	fetchCollections,
	fetchSchema,
	previewSchemaChange,
	previewSchemaWithExplain,
	updateSchema,
} from '$lib/api';
import { tick } from 'svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const basePath = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}`,
);
const collectionsHref = $derived(`${basePath}/collections`);

let collections = $state<CollectionSummary[]>([]);
let selectedCollection = $state('');
let selectedSchema = $state<CollectionSchema | null>(null);
let editMode = $state(false);
let editJson = $state('');
let validationError = $state<string | null>(null);
let statusMessage = $state<string | null>(null);
let error = $state<string | null>(null);
let createCollectionName = $state('');
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let viewMode = $state<'structured' | 'raw' | 'policy'>('structured');
let preview = $state<SchemaPreviewResult | null>(null);
let previewLoading = $state(false);

// ── Policy view state ─────────────────────────────────────────────
let policyJson = $state('{}');
let policyJsonError = $state<string | null>(null);
let policyReport = $state<PolicyCompileReport | null>(null);
let policyCompileLoading = $state(false);
let policyActivateError = $state<string | null>(null);
let policyActivateStatus = $state<string | null>(null);
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixtureSubject = $state('');
type FixtureOperation =
	| 'read'
	| 'create'
	| 'update'
	| 'patch'
	| 'delete'
	| 'transition'
	| 'rollback';
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixtureOperation = $state<FixtureOperation>('read');
let policyCompileGeneration = 0;
let fixtureGeneration = 0;
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixtureEntityId = $state('');
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixtureDataJson = $state('{}');
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixturePatchJson = $state('{}');
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixtureLifecycleName = $state('');
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixtureTargetState = $state('');
// biome-ignore lint/style/useConst: bind:value mutates this state.
let fixtureToVersion = $state('');
let fixtureResult = $state<DryRunExplanation | null>(null);
let fixtureError = $state<string | null>(null);
let fixtureLoading = $state(false);

const policyHasErrors = $derived(policyReport != null && (policyReport.errors?.length ?? 0) > 0);
const policyActivateDisabled = $derived(
	!selectedSchema || policyReport == null || policyHasErrors || policyJsonError != null,
);
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createSchemaJson = $state(`{
  "type": "object",
  "properties": {}
}`);

// ── Helpers for structured schema display ─────────────────────────

type SchemaProperty = {
	name: string;
	type: string;
	required: boolean;
	description: string | null;
	constraints: string[];
};

type LinkTypeInfo = {
	name: string;
	targetCollection: string;
	cardinality: string;
	required: boolean;
	hasMetadataSchema: boolean;
};

type GateInfo = {
	name: string;
	description: string | null;
	includes: string[];
};

type ValidationRuleInfo = {
	name: string;
	gate: string | null;
	advisory: boolean;
	message: string;
	field: string;
	fix: string | null;
};

type IndexInfo = {
	field: string;
	indexType: string;
	unique: boolean;
};

type CompoundIndexInfo = {
	fields: Array<{ field: string; indexType: string }>;
	unique: boolean;
};

function extractProperties(schema: CollectionSchema): SchemaProperty[] {
	const entitySchema = schema.entity_schema as Record<string, unknown> | null | undefined;
	if (!entitySchema || typeof entitySchema !== 'object') return [];

	const properties = entitySchema.properties as Record<string, unknown> | undefined;
	if (!properties || typeof properties !== 'object') return [];

	const requiredFields = Array.isArray(entitySchema.required)
		? (entitySchema.required as string[])
		: [];

	return Object.entries(properties).map(([name, def]) => {
		const prop = def as Record<string, unknown> | null;
		const constraints: string[] = [];

		if (prop) {
			if (prop.minimum !== undefined) constraints.push(`min: ${String(prop.minimum)}`);
			if (prop.maximum !== undefined) constraints.push(`max: ${String(prop.maximum)}`);
			if (prop.minLength !== undefined) constraints.push(`minLength: ${String(prop.minLength)}`);
			if (prop.maxLength !== undefined) constraints.push(`maxLength: ${String(prop.maxLength)}`);
			if (prop.pattern !== undefined) constraints.push(`pattern: ${String(prop.pattern)}`);
			if (Array.isArray(prop.enum))
				constraints.push(`enum: [${(prop.enum as unknown[]).map(String).join(', ')}]`);
			if (prop.format !== undefined) constraints.push(`format: ${String(prop.format)}`);
		}

		return {
			name,
			type: resolveType(prop),
			required: requiredFields.includes(name),
			description: prop && typeof prop.description === 'string' ? prop.description : null,
			constraints,
		};
	});
}

function resolveType(prop: Record<string, unknown> | null): string {
	if (!prop) return 'unknown';
	if (typeof prop.type === 'string') {
		if (prop.type === 'array' && prop.items && typeof prop.items === 'object') {
			const items = prop.items as Record<string, unknown>;
			return `array<${resolveType(items)}>`;
		}
		return prop.type;
	}
	if (Array.isArray(prop.type)) return (prop.type as string[]).join(' | ');
	return 'unknown';
}

function extractLinkTypes(schema: CollectionSchema): LinkTypeInfo[] {
	const linkTypes = schema.link_types as Record<string, unknown> | undefined;
	if (!linkTypes || typeof linkTypes !== 'object') return [];

	return Object.entries(linkTypes).map(([name, def]) => {
		const lt = def as Record<string, unknown>;
		return {
			name,
			targetCollection: typeof lt.target_collection === 'string' ? lt.target_collection : '?',
			cardinality: typeof lt.cardinality === 'string' ? lt.cardinality : '?',
			required: lt.required === true,
			hasMetadataSchema: lt.metadata_schema != null,
		};
	});
}

function extractGates(schema: CollectionSchema): GateInfo[] {
	const raw = schema as Record<string, unknown>;
	const gates = raw.gates as Record<string, unknown> | undefined;
	if (!gates || typeof gates !== 'object') return [];

	return Object.entries(gates).map(([name, def]) => {
		const g = def as Record<string, unknown>;
		return {
			name,
			description: typeof g.description === 'string' ? g.description : null,
			includes: Array.isArray(g.includes) ? (g.includes as string[]) : [],
		};
	});
}

function extractValidationRules(schema: CollectionSchema): ValidationRuleInfo[] {
	const raw = schema as Record<string, unknown>;
	const rules = raw.validation_rules as Array<Record<string, unknown>> | undefined;
	if (!Array.isArray(rules)) return [];

	return rules.map((r) => {
		const require = r.require as Record<string, unknown> | undefined;
		return {
			name: typeof r.name === 'string' ? r.name : '?',
			gate: typeof r.gate === 'string' ? r.gate : null,
			advisory: r.advisory === true,
			message: typeof r.message === 'string' ? r.message : '',
			field: require && typeof require.field === 'string' ? require.field : '?',
			fix: typeof r.fix === 'string' ? r.fix : null,
		};
	});
}

function extractIndexes(schema: CollectionSchema): IndexInfo[] {
	const raw = schema as Record<string, unknown>;
	const indexes = raw.indexes as Array<Record<string, unknown>> | undefined;
	if (!Array.isArray(indexes)) return [];

	return indexes.map((idx) => ({
		field: typeof idx.field === 'string' ? idx.field : '?',
		indexType: typeof idx.type === 'string' ? idx.type : '?',
		unique: idx.unique === true,
	}));
}

function extractCompoundIndexes(schema: CollectionSchema): CompoundIndexInfo[] {
	const raw = schema as Record<string, unknown>;
	const indexes = raw.compound_indexes as Array<Record<string, unknown>> | undefined;
	if (!Array.isArray(indexes)) return [];

	return indexes.map((idx) => ({
		fields: Array.isArray(idx.fields)
			? (idx.fields as Array<Record<string, unknown>>).map((f) => ({
					field: typeof f.field === 'string' ? f.field : '?',
					indexType: typeof f.type === 'string' ? f.type : '?',
				}))
			: [],
		unique: idx.unique === true,
	}));
}

function collectionHref(name: string): string {
	return `${collectionsHref}/${encodeURIComponent(name)}`;
}

function schemaVersionLabel(collection: CollectionSummary): string {
	return collection.schema_version ? `v${collection.schema_version}` : 'No schema';
}

async function loadCollections(preferredCollection?: string) {
	collections = await fetchCollections(scope);
	const querySelection = page.url.searchParams.get('collection') ?? undefined;
	const requestedSelection =
		preferredCollection ??
		querySelection ??
		(selectedCollection || undefined) ??
		collections[0]?.name;
	const nextSelection = collections.some((collection) => collection.name === requestedSelection)
		? requestedSelection
		: collections[0]?.name;

	if (nextSelection) {
		await selectCollection(nextSelection);
	}
}

async function selectCollection(collectionName: string) {
	selectedCollection = collectionName;
	selectedSchema = await fetchSchema(collectionName, scope);
	editJson = JSON.stringify(selectedSchema, null, 2);
	editMode = false;
	validationError = null;
	statusMessage = null;
	error = null;
	preview = null;
	previewLoading = false;
	resetPolicyView(selectedSchema);
}

function resetPolicyView(schema: CollectionSchema | null) {
	policyJson = JSON.stringify(schema?.access_control ?? {}, null, 2);
	policyJsonError = null;
	policyReport = null;
	policyActivateError = null;
	policyActivateStatus = null;
	fixtureResult = null;
	fixtureError = null;
}

function parsePolicyJson(): unknown | null {
	try {
		const parsed = JSON.parse(policyJson);
		policyJsonError = null;
		return parsed;
	} catch (err) {
		policyJsonError = err instanceof Error ? err.message : 'Invalid JSON';
		return null;
	}
}

function buildProposedSchema(accessControl: unknown): CollectionSchema | null {
	if (!selectedSchema) return null;
	return { ...selectedSchema, access_control: accessControl };
}

async function runPolicyCompile() {
	if (!selectedSchema) return;
	const parsed = parsePolicyJson();
	if (parsed == null) {
		policyReport = null;
		return;
	}
	const proposed = buildProposedSchema(parsed);
	if (!proposed) return;
	const compiledJsonSnapshot = policyJson;
	policyCompileGeneration += 1;
	const generation = policyCompileGeneration;
	policyCompileLoading = true;
	policyActivateError = null;
	policyActivateStatus = null;
	fixtureResult = null;
	fixtureError = null;
	try {
		const result = await previewSchemaChange(selectedCollection, proposed, scope);
		// Discard the response if the editor changed (or another compile started)
		// while this request was in flight: otherwise a stale report could
		// re-enable activation against a different policy than was compiled.
		if (generation !== policyCompileGeneration || policyJson !== compiledJsonSnapshot) {
			return;
		}
		policyReport = result.policy_compile_report ?? {
			errors: [],
			warnings: [],
			required_link_indexes: [],
			nullable_fields: [],
			denied_write_fields: [],
			envelope_summaries: [],
		};
		await tick();
		const firstError = document.querySelector<HTMLElement>(
			'[data-testid="schema-policy-error-row-0"]',
		);
		firstError?.focus();
	} catch (err) {
		if (generation !== policyCompileGeneration) return;
		policyReport = null;
		policyJsonError = err instanceof Error ? err.message : 'Compile preview failed';
	} finally {
		if (generation === policyCompileGeneration) policyCompileLoading = false;
	}
}

async function runFixtureDryRun() {
	if (!selectedSchema || policyHasErrors || !scope) return;
	const parsed = parsePolicyJson();
	if (parsed == null) return;
	const proposed = buildProposedSchema(parsed);
	if (!proposed) return;
	let parsedData: unknown | undefined;
	let parsedPatch: unknown | undefined;
	try {
		const trimmedData = fixtureDataJson.trim();
		const trimmedPatch = fixturePatchJson.trim();
		// `read` accepts either entity_id or an ad-hoc data payload; the
		// other write ops require their own data/patch shapes.
		const dataOpUsesPayload =
			fixtureOperation === 'read' || fixtureOperation === 'create' || fixtureOperation === 'update';
		if (dataOpUsesPayload && trimmedData) {
			parsedData = JSON.parse(trimmedData);
		}
		if (fixtureOperation === 'patch' && trimmedPatch) {
			parsedPatch = JSON.parse(trimmedPatch);
		}
	} catch (err) {
		fixtureError = err instanceof Error ? err.message : 'Fixture JSON invalid';
		fixtureResult = null;
		return;
	}
	const input: SchemaDryRunExplainInput = { operation: fixtureOperation };
	if (fixtureEntityId.trim()) input.entityId = fixtureEntityId.trim();
	if (parsedData !== undefined) input.data = parsedData;
	if (parsedPatch !== undefined) input.patch = parsedPatch;
	if (fixtureOperation === 'transition') {
		const lifecycle = fixtureLifecycleName.trim();
		const target = fixtureTargetState.trim();
		if (!lifecycle || !target) {
			fixtureError = 'transition fixtures require lifecycleName and targetState';
			fixtureResult = null;
			return;
		}
		input.lifecycleName = lifecycle;
		input.targetState = target;
	}
	if (fixtureOperation === 'rollback') {
		const trimmed = fixtureToVersion.trim();
		const parsedVersion = trimmed ? Number.parseInt(trimmed, 10) : Number.NaN;
		if (!Number.isInteger(parsedVersion) || parsedVersion < 0) {
			fixtureError = 'rollback fixtures require a non-negative toVersion integer';
			fixtureResult = null;
			return;
		}
		input.toVersion = parsedVersion;
	}
	fixtureGeneration += 1;
	const generation = fixtureGeneration;
	fixtureLoading = true;
	fixtureError = null;
	try {
		const subjectActor = fixtureSubject.trim();
		const result: SchemaPolicyDryRunResult = await previewSchemaWithExplain(
			selectedCollection,
			proposed,
			[input],
			scope,
			subjectActor ? { actor: subjectActor } : undefined,
		);
		// Drop the result if the editor was changed (or another fixture run
		// started) while this request was in flight.
		if (generation !== fixtureGeneration) return;
		fixtureResult = result.explanations[0] ?? null;
		if (!fixtureResult) {
			fixtureError = 'No explanation returned (proposed policy may be empty).';
		}
	} catch (err) {
		if (generation !== fixtureGeneration) return;
		fixtureResult = null;
		fixtureError = err instanceof Error ? err.message : 'Dry-run failed';
	} finally {
		if (generation === fixtureGeneration) fixtureLoading = false;
	}
}

async function activatePolicy() {
	if (!selectedSchema || policyHasErrors) return;
	const parsed = parsePolicyJson();
	if (parsed == null) return;
	const proposed = buildProposedSchema(parsed);
	if (!proposed) return;
	// Activation must advance the schema version so audit metadata and
	// downstream policy_version consumers can distinguish the new policy
	// from the previous one.
	proposed.version = selectedSchema.version + 1;
	policyActivateError = null;
	policyActivateStatus = null;
	try {
		const activated = await updateSchema(selectedCollection, proposed, { force: false }, scope);
		// Refresh the rail and the persisted schema. selectCollection() resets
		// the policy view, so we re-apply the activation status afterwards.
		await loadCollections(selectedCollection);
		policyActivateStatus = `Activated policy version v${activated.version} for ${selectedCollection}. Audit entry recorded.`;
	} catch (err) {
		policyActivateError = err instanceof Error ? err.message : 'Activate failed';
	}
}

function validateJson() {
	try {
		JSON.parse(editJson);
		validationError = null;
		statusMessage = 'Local JSON validation passed. Saving will apply this schema immediately.';
		return true;
	} catch (errorValue: unknown) {
		validationError = errorValue instanceof Error ? errorValue.message : 'Invalid JSON';
		statusMessage = null;
		return false;
	}
}

async function requestPreview() {
	if (!selectedCollection || !validateJson()) {
		return;
	}

	previewLoading = true;
	preview = null;
	error = null;

	try {
		preview = await previewSchemaChange(
			selectedCollection,
			JSON.parse(editJson) as CollectionSchema,
			scope,
		);
		statusMessage = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to preview schema change';
	} finally {
		previewLoading = false;
	}
}

async function confirmSave(force: boolean) {
	if (!selectedCollection || !validateJson()) {
		return;
	}

	try {
		selectedSchema = await updateSchema(
			selectedCollection,
			JSON.parse(editJson) as CollectionSchema,
			{ force },
			scope,
		);
		editJson = JSON.stringify(selectedSchema, null, 2);
		editMode = false;
		preview = null;
		statusMessage = `Saved schema for ${selectedCollection}.`;
		error = null;
		await loadCollections(selectedCollection);
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to save schema';
	}
}

async function submitCreateCollection() {
	try {
		const collectionName = createCollectionName.trim();
		const entitySchema = createSchemaJson.trim() ? (JSON.parse(createSchemaJson) as unknown) : null;
		await createCollection(
			collectionName,
			{
				description: null,
				version: 1,
				entity_schema: entitySchema,
				link_types: {},
			},
			scope,
		);
		createCollectionName = '';
		error = null;
		await loadCollections(collectionName);
		statusMessage = 'Collection created.';
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to create collection';
	}
}

$effect(() => {
	// Re-run when scope changes.
	void scope;
	void loadCollections();
});
</script>

<div class="page-header">
	<div>
		<h1>Schemas</h1>
		<p class="muted">Manage collection shape, validation, links, gates, and indexes.</p>
	</div>
	<div class="actions">
		<a class="button-link" href={collectionsHref}>Collections</a>
	</div>
</div>

{#if error}
	<p class="message error">{error}</p>
{/if}

{#if statusMessage}
	<p class="message success">{statusMessage}</p>
{/if}

<div class="schema-workspace">
	<section class="panel collection-rail">
		<div class="panel-header">
			<h2>Collections</h2>
			<span class="pill">{collections.length} registered</span>
		</div>
		<div class="panel-body stack">
			<section class="create-collection-form" aria-labelledby="create-collection-title">
				<h3 id="create-collection-title">Create Collection</h3>
				<label>
					<span>Name</span>
					<input bind:value={createCollectionName} placeholder="tasks" />
				</label>
				<label>
					<span>Entity Schema JSON</span>
					<textarea bind:value={createSchemaJson} rows="6"></textarea>
				</label>
				<button
					class="primary"
					disabled={!createCollectionName.trim()}
					onclick={submitCreateCollection}
				>
					Create Collection
				</button>
			</section>

			<div class="collection-list" aria-label="Registered collections">
				{#if collections.length === 0}
					<p class="muted">No collections registered yet.</p>
				{/if}
				{#each collections as collection}
					<button
						class="collection-option"
						class:selected={selectedCollection === collection.name}
						onclick={() => selectCollection(collection.name)}
					>
						<span class="collection-option-main">
							<strong>{collection.name}</strong>
							<span class="muted">{schemaVersionLabel(collection)}</span>
						</span>
					</button>
				{/each}
			</div>
		</div>
	</section>

	<section class="panel schema-detail-panel">
			<div class="panel-header">
				<h2>{selectedCollection || 'Schema Detail'}</h2>
				<div class="actions">
					{#if selectedSchema && !editMode}
						<a class="button-link" href={collectionHref(selectedSchema.collection)}>Entities</a>
						<button
							class:active={viewMode === 'structured'}
							onclick={() => (viewMode = 'structured')}
						>
							Structured
						</button>
						<button class:active={viewMode === 'raw'} onclick={() => (viewMode = 'raw')}>
							Raw JSON
						</button>
						<button
							data-testid="schema-policy-view-toggle"
							class:active={viewMode === 'policy'}
							onclick={() => (viewMode = 'policy')}
						>
							Policy
						</button>
						<button onclick={() => (editMode = true)}>Edit</button>
					{/if}
				</div>
			</div>
			<div class="panel-body stack">
				{#if !selectedSchema}
					<p class="muted">Select a collection to inspect its schema.</p>
				{:else if editMode}
					<textarea bind:value={editJson} oninput={() => { validateJson(); preview = null; }}></textarea>
					{#if validationError}
						<p class="message error">{validationError}</p>
					{/if}
					{#if statusMessage && !validationError && !preview}
						<p class="message">{statusMessage}</p>
					{/if}

					{#if preview}
						<div class="preview-panel" class:preview-breaking={preview.compatibility === 'breaking'} class:preview-compatible={preview.compatibility === 'compatible'} class:preview-metadata={preview.compatibility === 'metadata_only'}>
							<div class="preview-header">
								<strong>Schema Change Preview</strong>
								{#if preview.compatibility === 'breaking'}
									<span class="pill preview-pill-breaking">Breaking</span>
								{:else if preview.compatibility === 'compatible'}
									<span class="pill preview-pill-compatible">Compatible</span>
								{:else if preview.compatibility === 'metadata_only'}
									<span class="pill preview-pill-metadata">Metadata Only</span>
								{/if}
							</div>

							{#if preview.diff && preview.diff.changes.length > 0}
								<ul class="preview-changes">
									{#each preview.diff.changes as change}
										<li>
											<code>{change.path}</code>
											<span class="pill preview-kind-pill">{change.kind}</span>
											<span class="muted">{change.description}</span>
										</li>
									{/each}
								</ul>
							{:else}
								<p class="muted">No field-level changes detected.</p>
							{/if}

							{#if preview.compatibility === 'breaking'}
								<p class="preview-warning">This change is breaking and may invalidate existing entities. Force save to apply.</p>
							{/if}

							<div class="actions">
								<button onclick={() => { preview = null; }}>Back to Edit</button>
								{#if preview.compatibility === 'breaking'}
									<button class="danger" onclick={() => confirmSave(true)}>Force Save</button>
								{:else}
									<button class="primary" onclick={() => confirmSave(false)}>Save Schema</button>
								{/if}
							</div>
						</div>
					{/if}

					<div class="actions">
						<button
							onclick={() => {
								editMode = false;
								editJson = JSON.stringify(selectedSchema, null, 2);
								validationError = null;
								statusMessage = null;
								preview = null;
							}}
						>
							Cancel
						</button>
						{#if !preview}
							<button class="primary" disabled={!!validationError || previewLoading} onclick={requestPreview}>
								{previewLoading ? 'Checking...' : 'Preview Changes'}
							</button>
						{/if}
					</div>
				{:else if viewMode === 'raw'}
					<pre>{JSON.stringify(selectedSchema, null, 2)}</pre>
				{:else if viewMode === 'policy'}
					<section class="policy-view" data-testid="schema-policy-view">
						<header class="policy-header">
							<h3>Access control policy</h3>
							<span class="muted">Edit, compile, and dry-run before activation.</span>
						</header>

						<label class="policy-editor-label">
							<span>access_control</span>
							<textarea
								data-testid="schema-policy-editor"
								class="policy-editor"
								bind:value={policyJson}
								oninput={() => {
									policyJsonError = null;
									policyReport = null;
									fixtureResult = null;
									// Bump generations so any in-flight compile or fixture
									// response for the previous JSON is dropped on arrival.
									policyCompileGeneration += 1;
									fixtureGeneration += 1;
									// Clear the spinner — the in-flight request will be
									// discarded, so its `finally` won't unset this state.
									policyCompileLoading = false;
									fixtureLoading = false;
								}}
								spellcheck="false"
							></textarea>
						</label>
						{#if policyJsonError}
							<p class="message error" data-testid="schema-policy-json-error">
								{policyJsonError}
							</p>
						{/if}

						<div class="policy-actions">
							<button
								class="primary"
								data-testid="schema-policy-run-compile"
								disabled={policyCompileLoading}
								onclick={runPolicyCompile}
							>
								{policyCompileLoading ? 'Compiling…' : 'Run compile'}
							</button>
							<button
								data-testid="schema-policy-cancel"
								onclick={() => resetPolicyView(selectedSchema)}
							>
								Reset
							</button>
							<button
								class="primary"
								data-testid="schema-policy-activate"
								disabled={policyActivateDisabled}
								onclick={activatePolicy}
							>
								Activate policy
							</button>
						</div>

						{#if policyActivateError}
							<p class="message error" data-testid="schema-policy-activate-error">
								{policyActivateError}
							</p>
						{/if}
						{#if policyActivateStatus}
							<p
								class="message success"
								data-testid="schema-policy-activation-status"
							>
								{policyActivateStatus}
							</p>
						{/if}

						{#if policyReport}
							{#if (policyReport.errors?.length ?? 0) > 0}
								<div class="policy-panel policy-errors" data-testid="schema-policy-errors">
									<strong>Compile errors</strong>
									<ul>
										{#each policyReport.errors ?? [] as diag, idx}
											<!-- biome-ignore lint/a11y/noNoninteractiveTabindex: focus first error for keyboard users. -->
											<li
												data-testid={`schema-policy-error-row-${idx}`}
												tabindex="-1"
											>
												<span class="pill error-code">{diag.code}</span>
												{#if diag.path}<code>{diag.path}</code>{/if}
												{#if diag.rule_id}
													<span class="muted">· rule {diag.rule_id}</span>
												{/if}
												{#if diag.field}
													<span class="muted">· field {diag.field}</span>
												{/if}
												<span>: {diag.message}</span>
											</li>
										{/each}
									</ul>
								</div>
							{/if}

							{#if (policyReport.warnings?.length ?? 0) > 0}
								<div
									class="policy-panel policy-warnings"
									data-testid="schema-policy-warnings"
								>
									<strong>Warnings</strong>
									<ul>
										{#each policyReport.warnings ?? [] as diag}
											<li>{diag.message}</li>
										{/each}
									</ul>
								</div>
							{/if}

							{#if (policyReport.nullable_fields?.length ?? 0) > 0}
								<div
									class="policy-panel"
									data-testid="schema-policy-nullable-fields"
								>
									<strong>GraphQL nullability changes</strong>
									<ul>
										{#each policyReport.nullable_fields ?? [] as nf}
											<li>
												<code>{nf.collection}.{nf.field}</code>
												{#if nf.required_by_schema}
													<span class="pill">was required</span>
												{/if}
											</li>
										{/each}
									</ul>
								</div>
							{/if}

							{#if (policyReport.denied_write_fields?.length ?? 0) > 0}
								<div
									class="policy-panel"
									data-testid="schema-policy-denied-writes"
								>
									<strong>Denied-write fields</strong>
									<ul>
										{#each policyReport.denied_write_fields ?? [] as df}
											<li><code>{df.collection}.{df.field}</code></li>
										{/each}
									</ul>
								</div>
							{/if}

							{#if (policyReport.envelope_summaries?.length ?? 0) > 0}
								<div
									class="policy-panel"
									data-testid="schema-policy-envelopes"
								>
									<strong>MCP envelopes</strong>
									<table>
										<thead>
											<tr>
												<th>Collection</th>
												<th>Operation</th>
												<th>Decision</th>
												<th>Approval role</th>
											</tr>
										</thead>
										<tbody>
											{#each policyReport.envelope_summaries ?? [] as env}
												<tr>
													<td><code>{env.collection}</code></td>
													<td>{env.operation}</td>
													<td>{env.decision}</td>
													<td>{env.approval?.role ?? '—'}</td>
												</tr>
											{/each}
										</tbody>
									</table>
								</div>
							{/if}

							{#if (policyReport.required_link_indexes?.length ?? 0) > 0}
								<div
									class="policy-panel"
									data-testid="schema-policy-required-indexes"
								>
									<strong>Required link indexes</strong>
									<ul>
										{#each policyReport.required_link_indexes ?? [] as idx}
											<li>
												<code>{idx.name}</code>
												<span class="muted">
													{idx.source_collection} → {idx.target_collection} ({idx.direction})
												</span>
											</li>
										{/each}
									</ul>
								</div>
							{/if}

							{#if !policyHasErrors}
								<section class="policy-fixture" data-testid="schema-policy-fixture">
									<header><strong>Fixture dry-run</strong></header>
									<p class="muted small">
										Evaluates the proposed policy. Leaving Subject empty uses the current
										GraphQL caller; setting it routes the dry-run as that actor via
										<code>x-axon-actor</code>.
									</p>
									<div class="fixture-grid">
										<label>
											<span>Subject</span>
											<input
												data-testid="schema-policy-fixture-subject"
												bind:value={fixtureSubject}
												placeholder="finance-agent"
											/>
										</label>
										<label>
											<span>Operation</span>
											<select
												data-testid="schema-policy-fixture-operation"
												bind:value={fixtureOperation}
											>
												<option value="read">read</option>
												<option value="create">create</option>
												<option value="update">update</option>
												<option value="patch">patch</option>
												<option value="delete">delete</option>
												<option value="transition">transition</option>
												<option value="rollback">rollback</option>
											</select>
										</label>
										<label>
											<span>Entity ID (optional)</span>
											<input
												data-testid="schema-policy-fixture-entity"
												bind:value={fixtureEntityId}
												placeholder="invoices/abc"
											/>
										</label>
										{#if fixtureOperation === 'read' || fixtureOperation === 'create' || fixtureOperation === 'update'}
											<label class="fixture-textarea">
												<span>data (JSON)</span>
												<textarea
													data-testid="schema-policy-fixture-data"
													bind:value={fixtureDataJson}
													spellcheck="false"
												></textarea>
											</label>
										{/if}
										{#if fixtureOperation === 'patch'}
											<label class="fixture-textarea">
												<span>patch (JSON)</span>
												<textarea
													data-testid="schema-policy-fixture-patch"
													bind:value={fixturePatchJson}
													spellcheck="false"
												></textarea>
											</label>
										{/if}
										{#if fixtureOperation === 'transition'}
											<label>
												<span>lifecycleName</span>
												<input
													data-testid="schema-policy-fixture-lifecycle-name"
													bind:value={fixtureLifecycleName}
													placeholder="status_lifecycle"
												/>
											</label>
											<label>
												<span>targetState</span>
												<input
													data-testid="schema-policy-fixture-target-state"
													bind:value={fixtureTargetState}
													placeholder="approved"
												/>
											</label>
										{/if}
										{#if fixtureOperation === 'rollback'}
											<label>
												<span>toVersion</span>
												<input
													data-testid="schema-policy-fixture-to-version"
													bind:value={fixtureToVersion}
													inputmode="numeric"
													placeholder="3"
												/>
											</label>
										{/if}
									</div>
									<button
										data-testid="schema-policy-fixture-run"
										class="primary"
										disabled={fixtureLoading}
										onclick={runFixtureDryRun}
									>
										{fixtureLoading ? 'Evaluating…' : 'Run fixture dry-run'}
									</button>
									{#if fixtureError}
										<p class="message error" data-testid="schema-policy-fixture-error">
											{fixtureError}
										</p>
									{/if}
									{#if fixtureResult}
										<div class="fixture-result">
											<div>
												<span class="muted">Decision:</span>
												<strong data-testid="schema-policy-fixture-decision">
													{fixtureResult.decision}
												</strong>
											</div>
											<div>
												<span class="muted">Reason:</span>
												<code data-testid="schema-policy-fixture-reason-code">
													{fixtureResult.reason}
												</code>
											</div>
											{#if (fixtureResult.rule_ids?.length ?? 0) > 0}
												<div>
													<span class="muted">Rule IDs:</span>
													<code data-testid="schema-policy-fixture-rule-ids">
														{fixtureResult.rule_ids?.join(', ')}
													</code>
												</div>
											{/if}
											{#if fixtureResult.approval?.role}
												<div>
													<span class="muted">Approval role:</span>
													<code data-testid="schema-policy-fixture-approval-role">
														{fixtureResult.approval.role}
													</code>
												</div>
											{/if}
										</div>
									{/if}
								</section>
							{/if}
						{/if}
					</section>
				{:else}
					{@const properties = extractProperties(selectedSchema)}
					{@const linkTypes = extractLinkTypes(selectedSchema)}
					{@const gates = extractGates(selectedSchema)}
					{@const rules = extractValidationRules(selectedSchema)}
					{@const indexes = extractIndexes(selectedSchema)}
					{@const compoundIndexes = extractCompoundIndexes(selectedSchema)}
					{@const requiredFields = properties.filter((property) => property.required).length}

					<div class="schema-overview">
						<div>
							<span>Fields</span>
							<strong>{properties.length}</strong>
						</div>
						<div>
							<span>Required</span>
							<strong>{requiredFields}</strong>
						</div>
						<div>
							<span>Links</span>
							<strong>{linkTypes.length}</strong>
						</div>
						<div>
							<span>Rules</span>
							<strong>{rules.length}</strong>
						</div>
						<div>
							<span>Indexes</span>
							<strong>{indexes.length + compoundIndexes.length}</strong>
						</div>
					</div>

					<div class="schema-meta">
						<div><strong>Collection</strong> <span class="muted">{selectedSchema.collection}</span></div>
						{#if selectedSchema.description}
							<div><strong>Description</strong> <span class="muted">{selectedSchema.description}</span></div>
						{/if}
						<div><strong>Version</strong> <span class="pill">v{selectedSchema.version}</span></div>
					</div>

					{#if properties.length > 0}
						<div class="schema-section">
							<h3>Entity Fields <span class="pill">{properties.length}</span></h3>
							<table>
								<thead>
									<tr>
										<th>Field</th>
										<th>Type</th>
										<th>Required</th>
										<th>Constraints</th>
									</tr>
								</thead>
								<tbody>
									{#each properties as prop}
										<tr>
											<td>
												<code>{prop.name}</code>
												{#if prop.description}
													<br /><span class="muted small">{prop.description}</span>
												{/if}
											</td>
											<td><code class="type-label">{prop.type}</code></td>
											<td>{prop.required ? 'Yes' : 'No'}</td>
											<td>
												{#if prop.constraints.length > 0}
													{#each prop.constraints as c}
														<span class="constraint-tag">{c}</span>
													{/each}
												{:else}
													<span class="muted">--</span>
												{/if}
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{:else}
						<div class="schema-section">
							<h3>Entity Fields</h3>
							<p class="muted">No entity schema defined. All entity bodies are accepted.</p>
						</div>
					{/if}

					{#if linkTypes.length > 0}
						<div class="schema-section">
							<h3>Link Types <span class="pill">{linkTypes.length}</span></h3>
							<table>
								<thead>
									<tr>
										<th>Name</th>
										<th>Target</th>
										<th>Cardinality</th>
										<th>Required</th>
										<th>Metadata</th>
									</tr>
								</thead>
								<tbody>
									{#each linkTypes as lt}
										<tr>
											<td><code>{lt.name}</code></td>
											<td><code>{lt.targetCollection}</code></td>
											<td>{lt.cardinality}</td>
											<td>{lt.required ? 'Yes' : 'No'}</td>
											<td>{lt.hasMetadataSchema ? 'Schema defined' : '--'}</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/if}

					{#if gates.length > 0}
						<div class="schema-section">
							<h3>Gates <span class="pill">{gates.length}</span></h3>
							<table>
								<thead>
									<tr>
										<th>Gate</th>
										<th>Description</th>
										<th>Includes</th>
									</tr>
								</thead>
								<tbody>
									{#each gates as gate}
										<tr>
											<td><code>{gate.name}</code></td>
											<td>{gate.description ?? '--'}</td>
											<td>
												{#if gate.includes.length > 0}
													{#each gate.includes as inc}
														<span class="constraint-tag">{inc}</span>
													{/each}
												{:else}
													<span class="muted">--</span>
												{/if}
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/if}

					{#if rules.length > 0}
						<div class="schema-section">
							<h3>Validation Rules <span class="pill">{rules.length}</span></h3>
							<table>
								<thead>
									<tr>
										<th>Rule</th>
										<th>Field</th>
										<th>Gate</th>
										<th>Message</th>
									</tr>
								</thead>
								<tbody>
									{#each rules as rule}
										<tr>
											<td>
												<code>{rule.name}</code>
												{#if rule.advisory}
													<span class="pill advisory-pill">advisory</span>
												{/if}
											</td>
											<td><code>{rule.field}</code></td>
											<td>{rule.gate ?? '--'}</td>
											<td>
												{rule.message}
												{#if rule.fix}
													<br /><span class="muted small">Fix: {rule.fix}</span>
												{/if}
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/if}

					{#if indexes.length > 0}
						<div class="schema-section">
							<h3>Indexes <span class="pill">{indexes.length}</span></h3>
							<table>
								<thead>
									<tr>
										<th>Field</th>
										<th>Type</th>
										<th>Unique</th>
									</tr>
								</thead>
								<tbody>
									{#each indexes as idx}
										<tr>
											<td><code>{idx.field}</code></td>
											<td>{idx.indexType}</td>
											<td>{idx.unique ? 'Yes' : 'No'}</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/if}

					{#if compoundIndexes.length > 0}
						<div class="schema-section">
							<h3>Compound Indexes <span class="pill">{compoundIndexes.length}</span></h3>
							{#each compoundIndexes as cidx, i}
								<div class="compound-index-card">
									<strong>Index #{i + 1}</strong>
									{#if cidx.unique}
										<span class="pill">unique</span>
									{/if}
									<table>
										<thead>
											<tr>
												<th>Field</th>
												<th>Type</th>
											</tr>
										</thead>
										<tbody>
											{#each cidx.fields as f}
												<tr>
													<td><code>{f.field}</code></td>
													<td>{f.indexType}</td>
												</tr>
											{/each}
										</tbody>
									</table>
								</div>
							{/each}
						</div>
					{/if}

					{#if properties.length === 0 && linkTypes.length === 0 && gates.length === 0 && rules.length === 0 && indexes.length === 0 && compoundIndexes.length === 0}
						<p class="muted">This schema has no structured definitions. Switch to Raw JSON to see the full payload.</p>
					{/if}
				{/if}
			</div>
	</section>
</div>

<style>
	button.active {
		border-color: var(--accent-strong);
		background: #253041;
	}

	.schema-workspace {
		display: grid;
		grid-template-columns: minmax(18rem, 22rem) minmax(0, 1fr);
		gap: 1rem;
		align-items: start;
	}

	.collection-rail {
		position: sticky;
		top: 1rem;
		max-height: calc(100vh - 7.5rem);
		overflow: hidden;
	}

	.collection-rail .panel-body {
		max-height: calc(100vh - 12rem);
		overflow: auto;
	}

	.create-collection-form {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		padding: 0.85rem;
		border: 1px solid rgba(47, 55, 66, 0.8);
		border-radius: 0.5rem;
		background: rgba(15, 23, 32, 0.45);
	}

	.create-collection-form h3 {
		margin: 0;
		font-size: 0.95rem;
	}

	.create-collection-form label {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	.create-collection-form label span {
		color: var(--muted);
		font-size: 0.78rem;
		font-weight: 700;
		text-transform: uppercase;
	}

	.create-collection-form textarea {
		min-height: 7rem;
	}

	.collection-list {
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
	}

	.collection-option {
		width: 100%;
		justify-content: space-between;
		gap: 0.8rem;
		border-radius: 0.5rem;
		padding: 0.65rem 0.75rem;
		text-align: left;
	}

	.collection-option.selected {
		border-color: rgba(125, 211, 252, 0.65);
		background: rgba(125, 211, 252, 0.14);
	}

	.collection-option-main {
		display: flex;
		min-width: 0;
		flex-direction: column;
		gap: 0.15rem;
	}

	.collection-option-main strong,
	.collection-option-main span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.schema-detail-panel {
		min-width: 0;
	}

	.schema-overview {
		display: grid;
		grid-template-columns: repeat(5, minmax(6rem, 1fr));
		gap: 0.5rem;
	}

	.schema-overview div {
		display: flex;
		min-width: 0;
		flex-direction: column;
		gap: 0.25rem;
		padding: 0.7rem;
		border: 1px solid rgba(47, 55, 66, 0.8);
		border-radius: 0.5rem;
		background: rgba(15, 23, 32, 0.45);
	}

	.schema-overview span {
		color: var(--muted);
		font-size: 0.74rem;
		font-weight: 700;
		text-transform: uppercase;
	}

	.schema-overview strong {
		font-size: 1.15rem;
	}

	.schema-meta {
		display: flex;
		flex-wrap: wrap;
		gap: 1.5rem;
	}

	.schema-meta div {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.schema-section {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.schema-section h3 {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		margin: 0;
		font-size: 1rem;
		color: var(--accent);
	}

	.type-label {
		color: var(--accent);
	}

	.constraint-tag {
		display: inline-block;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
		padding: 0.15rem 0.5rem;
		margin: 0.1rem 0.2rem;
		font-size: 0.82rem;
		color: var(--muted);
		background: rgba(15, 23, 32, 0.6);
	}

	.advisory-pill {
		border-color: rgba(250, 204, 21, 0.4);
		color: #fde68a;
		font-size: 0.75rem;
		margin-left: 0.35rem;
	}

	.compound-index-card {
		border: 1px solid var(--border);
		border-radius: 1rem;
		padding: 0.75rem;
		margin-top: 0.5rem;
		background: rgba(15, 23, 32, 0.5);
	}

	.small {
		font-size: 0.82rem;
	}

	/* ── Schema preview panel ─────────────────────────────────────── */

	.preview-panel {
		border: 1px solid var(--border);
		border-radius: 1rem;
		padding: 1rem;
		background: rgba(15, 23, 32, 0.6);
	}

	.preview-breaking {
		border-color: var(--danger);
	}

	.preview-compatible {
		border-color: var(--success);
	}

	.preview-metadata {
		border-color: var(--accent);
	}

	.preview-header {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		margin-bottom: 0.75rem;
	}

	.preview-pill-breaking {
		border-color: var(--danger);
		color: var(--danger);
	}

	.preview-pill-compatible {
		border-color: var(--success);
		color: var(--success);
	}

	.preview-pill-metadata {
		border-color: var(--accent);
		color: var(--accent);
	}

	.preview-changes {
		list-style: none;
		padding: 0;
		margin: 0 0 0.75rem 0;
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	.preview-changes li {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.9rem;
	}

	.preview-kind-pill {
		display: inline-block;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
		padding: 0.1rem 0.4rem;
		font-size: 0.78rem;
		color: var(--muted);
		background: rgba(15, 23, 32, 0.6);
	}

	.preview-warning {
		color: var(--danger);
		font-size: 0.9rem;
		margin: 0.5rem 0;
	}

	button.danger {
		background: rgba(251, 113, 133, 0.15);
		border-color: var(--danger);
		color: var(--danger);
	}

	button.danger:hover {
		background: rgba(251, 113, 133, 0.25);
	}

	.policy-view {
		display: flex;
		flex-direction: column;
		gap: 0.85rem;
	}

	.policy-header h3 {
		margin: 0 0 0.25rem 0;
		font-size: 1rem;
		color: var(--accent);
	}

	.policy-editor-label {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.policy-editor-label > span {
		color: var(--muted);
		font-size: 0.78rem;
		font-weight: 700;
		text-transform: uppercase;
	}

	.policy-editor {
		min-height: 14rem;
		font-family: monospace;
	}

	.policy-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.policy-panel {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
		padding: 0.85rem;
		border: 1px solid var(--border);
		border-radius: 0.6rem;
		background: rgba(15, 23, 32, 0.5);
	}

	.policy-panel ul {
		margin: 0;
		padding-left: 1.1rem;
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	.policy-errors {
		border-color: var(--danger);
	}

	.policy-errors li:focus {
		outline: 2px solid var(--danger);
		outline-offset: 2px;
		background: rgba(251, 113, 133, 0.08);
	}

	.policy-warnings {
		border-color: rgba(250, 204, 21, 0.4);
	}

	.error-code {
		border-color: var(--danger);
		color: var(--danger);
	}

	.policy-fixture {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
		padding: 0.85rem;
		border: 1px solid var(--border);
		border-radius: 0.6rem;
		background: rgba(15, 23, 32, 0.55);
	}

	.fixture-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 0.5rem;
	}

	.fixture-grid label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	.fixture-grid label > span {
		color: var(--muted);
		font-size: 0.78rem;
		font-weight: 700;
		text-transform: uppercase;
	}

	.fixture-textarea {
		grid-column: 1 / -1;
	}

	.fixture-textarea textarea {
		min-height: 6rem;
		font-family: monospace;
	}

	.fixture-result {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		padding: 0.6rem 0.85rem;
		border-radius: 0.5rem;
		background: rgba(15, 23, 32, 0.7);
	}

	@media (max-width: 1100px) {
		.schema-workspace {
			grid-template-columns: 1fr;
		}

		.collection-rail {
			position: static;
			max-height: none;
		}

		.collection-rail .panel-body {
			max-height: none;
		}

		.schema-overview {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
	}
</style>
