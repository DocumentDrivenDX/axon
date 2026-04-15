<script lang="ts">
import {
	type CollectionSchema,
	type CollectionSummary,
	type SchemaPreviewResult,
	createCollection,
	fetchCollections,
	fetchSchema,
	previewSchemaChange,
	updateSchema,
} from '$lib/api';
import { onMount } from 'svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);

let collections: CollectionSummary[] = [];
let selectedCollection = '';
let selectedSchema: CollectionSchema | null = null;
let editMode = false;
let editJson = '';
let validationError: string | null = null;
let statusMessage: string | null = null;
let error: string | null = null;
let createCollectionName = '';
// biome-ignore lint/style/useConst: Svelte template reassigns this via on:click handlers.
let viewMode: 'structured' | 'raw' = 'structured';
let preview: SchemaPreviewResult | null = null;
let previewLoading = false;
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createSchemaJson = `{
  "type": "object",
  "properties": {}
}`;

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

async function loadCollections(preferredCollection?: string) {
	collections = await fetchCollections(scope);
	const nextSelection = preferredCollection ?? selectedCollection ?? collections[0]?.name;

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
		const entitySchema = createSchemaJson.trim() ? (JSON.parse(createSchemaJson) as unknown) : null;
		await createCollection(
			createCollectionName,
			{
				description: null,
				version: 1,
				entity_schema: entitySchema,
				link_types: {},
			},
			scope,
		);
		createCollectionName = '';
		statusMessage = 'Collection created.';
		error = null;
		await loadCollections();
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to create collection';
	}
}

onMount(() => {
	void loadCollections();
});
</script>

<div class="page-header">
	<div>
		<h1>Schemas</h1>
		<p class="muted">View and update collection schemas through the live HTTP endpoints.</p>
	</div>
</div>

{#if error}
	<p class="message error">{error}</p>
{/if}

{#if statusMessage}
	<p class="message success">{statusMessage}</p>
{/if}

<div class="two-column">
	<section class="panel">
		<div class="panel-header">
			<h2>Collections</h2>
			<span class="pill">{collections.length} registered</span>
		</div>
		<div class="panel-body stack">
			{#if collections.length === 0}
				<p class="muted">No collections registered yet.</p>
			{/if}
			{#each collections as collection}
				<button on:click={() => selectCollection(collection.name)}>
					{collection.name} · {collection.schema_version ? `v${collection.schema_version}` : 'No schema'}
				</button>
			{/each}
		</div>
	</section>

	<section class="stack">
		<section class="panel">
			<div class="panel-header">
				<h2>Create Collection</h2>
			</div>
			<div class="panel-body stack">
				<label>
					<span>Name</span>
					<input bind:value={createCollectionName} placeholder="tasks" />
				</label>
				<label>
					<span>Entity Schema JSON</span>
					<textarea bind:value={createSchemaJson}></textarea>
				</label>
				<div class="actions">
					<button
						class="primary"
						disabled={!createCollectionName.trim()}
						on:click={submitCreateCollection}
					>
						Create Collection
					</button>
				</div>
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>{selectedCollection || 'Schema Detail'}</h2>
				<div class="actions">
					{#if selectedSchema && !editMode}
						<button
							class:active={viewMode === 'structured'}
							on:click={() => (viewMode = 'structured')}
						>
							Structured
						</button>
						<button class:active={viewMode === 'raw'} on:click={() => (viewMode = 'raw')}>
							Raw JSON
						</button>
						<button on:click={() => (editMode = true)}>Edit</button>
					{/if}
				</div>
			</div>
			<div class="panel-body stack">
				{#if !selectedSchema}
					<p class="muted">Select a collection to inspect its schema.</p>
				{:else if editMode}
					<textarea bind:value={editJson} on:input={() => { validateJson(); preview = null; }}></textarea>
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
								<button on:click={() => { preview = null; }}>Back to Edit</button>
								{#if preview.compatibility === 'breaking'}
									<button class="danger" on:click={() => confirmSave(true)}>Force Save</button>
								{:else}
									<button class="primary" on:click={() => confirmSave(false)}>Save Schema</button>
								{/if}
							</div>
						</div>
					{/if}

					<div class="actions">
						<button
							on:click={() => {
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
							<button class="primary" disabled={!!validationError || previewLoading} on:click={requestPreview}>
								{previewLoading ? 'Checking...' : 'Preview Changes'}
							</button>
						{/if}
					</div>
				{:else if viewMode === 'raw'}
					<pre>{JSON.stringify(selectedSchema, null, 2)}</pre>
				{:else}
					{@const properties = extractProperties(selectedSchema)}
					{@const linkTypes = extractLinkTypes(selectedSchema)}
					{@const gates = extractGates(selectedSchema)}
					{@const rules = extractValidationRules(selectedSchema)}
					{@const indexes = extractIndexes(selectedSchema)}
					{@const compoundIndexes = extractCompoundIndexes(selectedSchema)}

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
	</section>
</div>

<style>
	button.active {
		border-color: var(--accent-strong);
		background: #253041;
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
</style>
