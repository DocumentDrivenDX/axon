<script lang="ts">
import { afterNavigate } from '$app/navigation';
import { base } from '$app/paths';
import {
	type AuditEntry,
	type CollectionDetail,
	type CollectionView,
	type CommitMutationIntentOutcome,
	type EntityRecord,
	// biome-ignore lint/correctness/noUnusedImports: Used in template type cast.
	type FieldDiff,
	type LifecycleDef,
	type Link,
	type MutationPreviewResult,
	type RollbackPreview,
	type TraverseResult,
	applyEntityRollback,
	commitMutationIntent,
	createEntity,
	createLink,
	deleteCollectionTemplate,
	// biome-ignore lint/correctness/noUnusedImports: Used in template onclick handler.
	deleteEntity,
	deleteLink,
	fetchCollection,
	fetchCollectionTemplate,
	fetchEntities,
	fetchEntity,
	fetchEntityAudit,
	fetchRenderedEntity,
	lifecyclesFromSchema,
	previewEntityRollback,
	previewMutationIntent,
	putCollectionTemplate,
	revertAuditEntry,
	transitionLifecycle,
	traverseLinks,
	updateEntity,
} from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template for entity data tree.
import JsonTree from '$lib/components/JsonTree.svelte';
// biome-ignore lint/correctness/noUnusedImports: Used in template as the intent preview dialog.
import MutationIntentPreviewModal from '$lib/components/MutationIntentPreviewModal.svelte';
// biome-ignore lint/correctness/noUnusedImports: Used in template for casting entity data.
import type { JsonValue } from '$lib/components/json-tree-types';
import { validateEntityData } from '$lib/schema-validation';
import { onMount } from 'svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const basePath = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}`,
);
const schemasHref = $derived(`${basePath}/schemas`);

let collectionName = $state('');
const selectedSchemaHref = $derived(
	collectionName ? `${schemasHref}?collection=${encodeURIComponent(collectionName)}` : schemasHref,
);
let collection = $state<CollectionDetail | null>(null);
let entities = $state<EntityRecord[]>([]);
let selectedEntity = $state<EntityRecord | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let nextCursor = $state<string | null>(null);
let paginationHistory = $state<Array<string | null>>([null]);
let pageIndex = $state(0);

let createOpen = $state(false);
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createId = $state('');
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createJson = $state(`{
  "title": ""
}`);
let createErrors = $state<string[]>([]);
let createMessage = $state<string | null>(null);

let editMode = $state(false);
let editData = $state<Record<string, unknown> | null>(null);
let saveError = $state<string | null>(null);
let saveMessage = $state<string | null>(null);
let saving = $state(false);
let intentPreview = $state<MutationPreviewResult | null>(null);
let intentCommitOutcome = $state<CommitMutationIntentOutcome | null>(null);
let intentModalOpen = $state(false);
let previewingIntent = $state(false);
let committingIntent = $state(false);

// biome-ignore lint/style/useConst: Svelte template onclick handlers mutate this state.
let confirmDelete = $state(false);
// biome-ignore lint/style/useConst: Svelte template onclick handlers mutate this state.
let deleteMessage = $state<string | null>(null);

// ── Entity detail tab state ────────────────────────────────────────────────

type EntityTab = 'data' | 'audit' | 'links' | 'lifecycle' | 'markdown' | 'rollback';
// biome-ignore lint/style/useConst: Svelte event handlers reassign this state.
let activeTab = $state<EntityTab>('data');

// Audit
let auditEntries = $state<AuditEntry[]>([]);
let auditLoading = $state(false);
let auditError = $state<string | null>(null);

// Audit revert
let revertConfirmId = $state<number | null>(null);
let revertMessage = $state<string | null>(null);
let revertError = $state<string | null>(null);

// Rollback
let rollbackPreview = $state<RollbackPreview | null>(null);
let rollbackPreviewVersion = $state<number | null>(null);
let rollbackPreviewLoading = $state(false);
let rollbackPreviewError = $state<string | null>(null);
let rollbackApplying = $state(false);
let rollbackApplyError = $state<string | null>(null);
let rollbackApplyMessage = $state<string | null>(null);
let loadedTabKey = '';

// Links
let links = $state<Link[]>([]);
let traverse = $state<TraverseResult | null>(null);
let linksLoading = $state(false);
let linksError = $state<string | null>(null);
let showCreateLink = $state(false);
let newLinkType = $state('');
let newLinkTargetCollection = $state('');
let newLinkTargetId = $state('');
let createLinkError = $state<string | null>(null);

// Lifecycle
let lifecycleError = $state<string | null>(null);
let transitioning = $state(false);

// Markdown rendering
let renderedMarkdown = $state<string | null>(null);
let renderedError = $state<string | null>(null);

// Collection template (page-level section, not entity-scoped)
let template = $state<CollectionView | null>(null);
let templateDraft = $state('');
let templateEditMode = $state(false);
let templateError = $state<string | null>(null);
let templateSaving = $state(false);
let templateStatus = $state<string | null>(null);

const lifecycleDefs = $derived<Record<string, LifecycleDef>>(
	collection ? lifecyclesFromSchema(collection.schema) : {},
);

type EntitySchemaField = {
	name: string;
	type: string;
	required: boolean;
};

function resolveSchemaType(prop: Record<string, unknown> | null): string {
	if (!prop) return 'unknown';
	if (typeof prop.type === 'string') {
		if (prop.type === 'array' && prop.items && typeof prop.items === 'object') {
			return `array<${resolveSchemaType(prop.items as Record<string, unknown>)}>`;
		}
		return prop.type;
	}
	if (Array.isArray(prop.type)) return (prop.type as string[]).join(' | ');
	return 'unknown';
}

function entitySchemaFields(schema: unknown): EntitySchemaField[] {
	if (!schema || typeof schema !== 'object') return [];
	const entitySchema = schema as Record<string, unknown>;
	const properties = entitySchema.properties as Record<string, unknown> | undefined;
	if (!properties || typeof properties !== 'object') return [];
	const required = new Set(
		Array.isArray(entitySchema.required) ? (entitySchema.required as string[]) : [],
	);
	return Object.entries(properties).map(([name, definition]) => ({
		name,
		type: resolveSchemaType(definition as Record<string, unknown> | null),
		required: required.has(name),
	}));
}

const schemaFields = $derived(entitySchemaFields(collection?.schema?.entity_schema));
const requiredSchemaFields = $derived(schemaFields.filter((field) => field.required));

function currentLifecycleState(def: LifecycleDef): string | null {
	if (!selectedEntity) return null;
	const value = selectedEntity.data[def.field];
	return typeof value === 'string' ? value : null;
}

function allowedTransitions(def: LifecycleDef): string[] {
	const state = currentLifecycleState(def);
	if (!state) return [];
	return def.transitions[state] ?? [];
}

async function loadAuditTab() {
	if (!selectedEntity || !collectionName) return;
	auditLoading = true;
	auditError = null;
	try {
		const result = await fetchEntityAudit(collectionName, selectedEntity.id, scope);
		auditEntries = result.entries;
	} catch (e: unknown) {
		auditError = e instanceof Error ? e.message : 'Failed to load audit';
	} finally {
		auditLoading = false;
	}
}

async function loadLinksTab() {
	if (!selectedEntity || !collectionName) return;
	linksLoading = true;
	linksError = null;
	try {
		traverse = await traverseLinks(collectionName, selectedEntity.id, {}, scope);
		// The server returns path hops when available; each hop describes a
		// single link. Dedup by (target_collection, target_id, link_type).
		const paths = traverse.paths ?? [];
		const seen = new Set<string>();
		links = [];
		for (const p of paths) {
			const key = `${p.link_type}:${p.target_collection}/${p.target_id}`;
			if (seen.has(key)) continue;
			seen.add(key);
			links.push({
				source_collection: p.source_collection,
				source_id: p.source_id,
				target_collection: p.target_collection,
				target_id: p.target_id,
				link_type: p.link_type,
			});
		}
	} catch (e: unknown) {
		linksError = e instanceof Error ? e.message : 'Failed to load links';
	} finally {
		linksLoading = false;
	}
}

async function loadMarkdownTab() {
	if (!selectedEntity || !collectionName) return;
	renderedError = null;
	renderedMarkdown = null;
	try {
		renderedMarkdown = await fetchRenderedEntity(collectionName, selectedEntity.id, scope);
	} catch (e: unknown) {
		renderedError = e instanceof Error ? e.message : 'Failed to render markdown';
	}
}

async function ensureAuditLoaded() {
	if (auditEntries.length === 0 && !auditLoading) {
		await loadAuditTab();
	}
}

async function doRevertAuditEntry(entryId: number) {
	if (!scope) return;
	try {
		await revertAuditEntry(entryId, scope);
		revertMessage = `Entry #${entryId} reverted successfully.`;
		revertError = null;
		revertConfirmId = null;
		await loadAuditTab();
	} catch (e: unknown) {
		revertError = e instanceof Error ? e.message : 'Revert failed';
		revertConfirmId = null;
	}
}

async function doPreviewRollback(toVersion: number) {
	if (!selectedEntity || !collectionName) return;
	rollbackPreviewLoading = true;
	rollbackPreviewError = null;
	rollbackPreview = null;
	rollbackPreviewVersion = toVersion;
	rollbackApplyError = null;
	rollbackApplyMessage = null;
	try {
		rollbackPreview = await previewEntityRollback(
			collectionName,
			selectedEntity.id,
			toVersion,
			scope,
		);
	} catch (e: unknown) {
		rollbackPreviewError = e instanceof Error ? e.message : 'Failed to preview rollback';
	} finally {
		rollbackPreviewLoading = false;
	}
}

async function doApplyRollback() {
	if (!selectedEntity || !collectionName || rollbackPreviewVersion === null) return;
	rollbackApplying = true;
	rollbackApplyError = null;
	rollbackApplyMessage = null;
	try {
		const result = await applyEntityRollback(
			collectionName,
			selectedEntity.id,
			rollbackPreviewVersion,
			selectedEntity.version,
			scope,
		);
		selectedEntity = result.entity;
		const idx = entities.findIndex((e) => e.id === result.entity.id);
		if (idx >= 0) entities[idx] = result.entity;
		rollbackApplyMessage = `Rolled back to v${rollbackPreviewVersion}. Now at v${result.entity.version}.`;
		rollbackPreview = null;
		rollbackPreviewVersion = null;
		// Reload audit entries to show the rollback entry
		await loadAuditTab();
	} catch (e: unknown) {
		rollbackApplyError = e instanceof Error ? e.message : 'Failed to apply rollback';
	} finally {
		rollbackApplying = false;
	}
}

async function submitCreateLink() {
	if (!selectedEntity || !collectionName) return;
	if (!newLinkType.trim() || !newLinkTargetCollection.trim() || !newLinkTargetId.trim()) {
		createLinkError = 'link_type, target collection, and target id are all required';
		return;
	}
	createLinkError = null;
	try {
		await createLink(
			{
				source_collection: collectionName,
				source_id: selectedEntity.id,
				target_collection: newLinkTargetCollection.trim(),
				target_id: newLinkTargetId.trim(),
				link_type: newLinkType.trim(),
			},
			scope,
		);
		newLinkType = '';
		newLinkTargetCollection = '';
		newLinkTargetId = '';
		showCreateLink = false;
		await loadLinksTab();
	} catch (e: unknown) {
		createLinkError = e instanceof Error ? e.message : 'Failed to create link';
	}
}

async function removeLink(link: Link) {
	try {
		await deleteLink(
			{
				source_collection: link.source_collection,
				source_id: link.source_id,
				target_collection: link.target_collection,
				target_id: link.target_id,
				link_type: link.link_type,
			},
			scope,
		);
		await loadLinksTab();
	} catch (e: unknown) {
		linksError = e instanceof Error ? e.message : 'Failed to delete link';
	}
}

async function doTransition(lifecycleName: string, targetState: string) {
	if (!selectedEntity || !collectionName) return;
	transitioning = true;
	lifecycleError = null;
	try {
		const response = await transitionLifecycle(
			collectionName,
			selectedEntity.id,
			{
				lifecycle_name: lifecycleName,
				target_state: targetState,
				expected_version: selectedEntity.version,
			},
			scope,
		);
		selectedEntity = response.entity;
		const idx = entities.findIndex((e) => e.id === response.entity.id);
		if (idx >= 0) entities[idx] = response.entity;
	} catch (e: unknown) {
		lifecycleError = e instanceof Error ? e.message : 'Transition failed';
	} finally {
		transitioning = false;
	}
}

async function loadTemplate() {
	if (!collectionName) return;
	templateError = null;
	try {
		template = await fetchCollectionTemplate(collectionName, scope);
		templateDraft = template.template;
	} catch (e: unknown) {
		// 404 is normal when no template is set yet.
		const msg = e instanceof Error ? e.message : String(e);
		if (/not[_ ]?found|404/i.test(msg)) {
			template = null;
			templateDraft = '';
		} else {
			templateError = msg;
		}
	}
}

async function saveTemplate() {
	if (!collectionName) return;
	templateSaving = true;
	templateError = null;
	templateStatus = null;
	try {
		const result = await putCollectionTemplate(collectionName, templateDraft, scope);
		template = {
			collection: result.collection,
			template: result.template,
			version: result.version,
			updated_at_ns: result.updated_at_ns ?? null,
			updated_by: result.updated_by ?? null,
		};
		templateDraft = result.template;
		templateEditMode = false;
		templateStatus =
			(result.warnings?.length ?? 0) > 0
				? `Saved with warnings: ${(result.warnings ?? []).join(', ')}`
				: 'Template saved.';
	} catch (e: unknown) {
		templateError = e instanceof Error ? e.message : 'Failed to save template';
	} finally {
		templateSaving = false;
	}
}

async function deleteTemplate() {
	if (!collectionName) return;
	try {
		await deleteCollectionTemplate(collectionName, scope);
		template = null;
		templateDraft = '';
		templateEditMode = false;
		templateStatus = 'Template deleted.';
	} catch (e: unknown) {
		templateError = e instanceof Error ? e.message : 'Failed to delete template';
	}
}

// Load tab content lazily when the user switches tabs or when selection changes.
$effect(() => {
	if (!selectedEntity) return;
	const tabKey = `${collectionName ?? ''}:${selectedEntity.id}:${activeTab}`;
	if (tabKey === loadedTabKey) return;
	loadedTabKey = tabKey;
	// Reset tab caches so old data doesn't flash.
	auditEntries = [];
	links = [];
	traverse = null;
	renderedMarkdown = null;
	rollbackPreview = null;
	rollbackPreviewVersion = null;
	rollbackPreviewError = null;
	rollbackApplyError = null;
	rollbackApplyMessage = null;
	if (activeTab === 'audit') void loadAuditTab();
	else if (activeTab === 'links') void loadLinksTab();
	else if (activeTab === 'markdown') void loadMarkdownTab();
	else if (activeTab === 'rollback') void loadAuditTab();
});

async function loadCollection(targetCollection: string, afterId: string | null) {
	loading = true;
	try {
		collection = await fetchCollection(targetCollection, scope);
		const result = await fetchEntities(
			targetCollection,
			{
				limit: 50,
				afterId,
			},
			scope,
		);
		entities = result.entities;
		nextCursor = result.next_cursor;
		selectedEntity = entities[0]
			? await fetchEntity(targetCollection, entities[0].id, scope)
			: null;
		editMode = false;
		editData = null;
		intentModalOpen = false;
		intentPreview = null;
		intentCommitOutcome = null;
		error = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load collection';
	} finally {
		loading = false;
	}
}

async function openEntity(id: string) {
	if (!collectionName) {
		return;
	}

	try {
		selectedEntity = await fetchEntity(collectionName, id, scope);
		editMode = false;
		editData = null;
		intentModalOpen = false;
		intentPreview = null;
		intentCommitOutcome = null;
		saveError = null;
		saveMessage = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load entity';
	}
}

function startEdit() {
	if (!selectedEntity) return;
	// Use JSON round-trip instead of structuredClone: Svelte 5 deep-reactive proxies can
	// cause structuredClone to throw (DataCloneError), which silently aborts the function
	// before editMode is set. Entity data is always plain JSON, so this is safe.
	editData = JSON.parse(JSON.stringify(selectedEntity.data)) as Record<string, unknown>;
	editMode = true;
	intentModalOpen = false;
	intentPreview = null;
	intentCommitOutcome = null;
	saveError = null;
	saveMessage = null;
}

function cancelEdit() {
	editMode = false;
	editData = null;
	intentModalOpen = false;
	intentPreview = null;
	intentCommitOutcome = null;
	saveError = null;
}

function validateEditData(): boolean {
	if (collection?.schema?.entity_schema && editData) {
		const issues = validateEntityData(collection.schema.entity_schema, editData);
		if (issues.length > 0) {
			saveError = issues.join('; ');
			return false;
		}
	}

	return true;
}

async function saveEntity() {
	if (!selectedEntity || !editData || !collectionName) return;
	saving = true;
	saveError = null;
	saveMessage = null;

	if (!validateEditData()) {
		saving = false;
		return;
	}

	try {
		const updated = await updateEntity(
			collectionName,
			selectedEntity.id,
			editData,
			selectedEntity.version,
			scope,
		);
		selectedEntity = updated;
		editMode = false;
		editData = null;
		saveMessage = `Saved v${updated.version}.`;
		const idx = entities.findIndex((e) => e.id === updated.id);
		if (idx >= 0) {
			entities[idx] = updated;
		}
	} catch (errorValue: unknown) {
		saveError = errorValue instanceof Error ? errorValue.message : 'Failed to save entity';
	} finally {
		saving = false;
	}
}

async function previewEntityIntent() {
	if (!selectedEntity || !editData || !collectionName) return;
	previewingIntent = true;
	saveError = null;
	saveMessage = null;
	intentCommitOutcome = null;

	if (!validateEditData()) {
		previewingIntent = false;
		return;
	}

	try {
		intentPreview = await previewMutationIntent(scope, {
			operation: {
				operationKind: 'update_entity',
				operation: {
					collection: collectionName,
					id: selectedEntity.id,
					expected_version: selectedEntity.version,
					data: editData,
				},
			},
			expiresInSeconds: 600,
		});
		intentModalOpen = true;
	} catch (errorValue: unknown) {
		saveError = errorValue instanceof Error ? errorValue.message : 'Failed to preview intent';
	} finally {
		previewingIntent = false;
	}
}

async function commitPreviewIntent() {
	if (!intentPreview?.intentToken || !intentPreview.intent || !selectedEntity || !collectionName) {
		return;
	}
	committingIntent = true;
	intentCommitOutcome = null;

	try {
		const outcome = await commitMutationIntent(scope, {
			intentToken: intentPreview.intentToken,
			intentId: intentPreview.intent.id,
		});
		intentCommitOutcome = outcome;
		if (outcome.ok) {
			const updated = await fetchEntity(collectionName, selectedEntity.id, scope);
			selectedEntity = updated;
			editMode = false;
			editData = null;
			saveMessage = `Saved v${updated.version}.`;
			const idx = entities.findIndex((e) => e.id === updated.id);
			if (idx >= 0) {
				entities[idx] = updated;
			}
		}
	} catch (errorValue: unknown) {
		saveError = errorValue instanceof Error ? errorValue.message : 'Failed to commit intent';
	} finally {
		committingIntent = false;
	}
}

function handleTreeUpdate(value: unknown) {
	editData = value as Record<string, unknown>;
}

function validateCreateForm(): Record<string, unknown> | null {
	createErrors = [];
	if (!createId.trim()) {
		createErrors.push('Entity ID is required.');
	}

	let parsedData: Record<string, unknown>;
	try {
		parsedData = JSON.parse(createJson) as Record<string, unknown>;
	} catch (errorValue: unknown) {
		createErrors.push(
			errorValue instanceof Error ? errorValue.message : 'Entity JSON must be valid',
		);
		return null;
	}

	if (collection?.schema?.entity_schema) {
		createErrors.push(...validateEntityData(collection.schema.entity_schema, parsedData));
	}

	return createErrors.length === 0 ? parsedData : null;
}

async function submitCreateEntity() {
	const parsedData = validateCreateForm();
	if (!parsedData || !collectionName) {
		return;
	}

	try {
		const entity = await createEntity(collectionName, createId.trim(), parsedData, scope);
		createErrors = [];
		createOpen = false;
		paginationHistory = [null];
		pageIndex = 0;
		await loadCollection(collectionName, null);
		const existingIndex = entities.findIndex((e) => e.id === entity.id);
		if (existingIndex >= 0) {
			entities[existingIndex] = entity;
		} else {
			entities = [entity, ...entities];
		}
		selectedEntity = entity;
		createMessage = `Created ${entity.id}.`;
	} catch (errorValue: unknown) {
		createErrors = [errorValue instanceof Error ? errorValue.message : 'Failed to create entity'];
	}
}

async function nextPage() {
	if (!nextCursor) {
		return;
	}

	pageIndex += 1;
	paginationHistory = [...paginationHistory, nextCursor];
	await loadCollection(collectionName, nextCursor);
}

async function previousPage() {
	if (pageIndex === 0) {
		return;
	}

	pageIndex -= 1;
	await loadCollection(collectionName, paginationHistory[pageIndex] ?? null);
}

async function syncRoute() {
	const routeCollectionName = decodeURIComponent(
		window.location.pathname.split('/').filter(Boolean).at(-1) ?? '',
	);
	if (!routeCollectionName || routeCollectionName === collectionName) {
		return;
	}

	collectionName = routeCollectionName;
	paginationHistory = [null];
	pageIndex = 0;
	selectedEntity = null;
	await loadCollection(routeCollectionName, null);
	await loadTemplate();
}

onMount(() => {
	void syncRoute();
});

afterNavigate(() => {
	void syncRoute();
});
</script>

<div class="page-header">
	<div>
		<h1>{collectionName}</h1>
		<p class="muted">
			{collection?.entity_count ?? entities.length} entities
			{#if collection?.schema}
				· schema v{collection.schema.version}
			{:else}
				· no schema
			{/if}
		</p>
	</div>
	<div class="actions">
		<a class="button-link" href={selectedSchemaHref}>Schema</a>
		<button onclick={() => (createOpen = !createOpen)}>
			{createOpen ? 'Hide Form' : 'New Entity'}
		</button>
	</div>
</div>

{#if error}
	<p class="message error">{error}</p>
{/if}

{#if createMessage}
	<p class="message success">{createMessage}</p>
{/if}

{#if deleteMessage}
	<p class="message success">{deleteMessage}</p>
{/if}

<div class="entity-workspace">
	<section class="panel entity-rail">
		<div class="panel-header">
			<h2>Entities</h2>
			<div class="actions">
				<button disabled={pageIndex === 0} onclick={previousPage}>Previous</button>
				<button disabled={!nextCursor} onclick={nextPage}>Next</button>
			</div>
		</div>
		<div class="panel-body stack">
			{#if collection?.schema}
				<div class="schema-context">
					<div class="schema-context-row">
						<span class="meta-label">Schema</span>
						<a href={selectedSchemaHref}>v{collection.schema.version}</a>
						<span class="muted">{schemaFields.length} fields</span>
					</div>
					{#if schemaFields.length > 0}
						<div class="field-chip-row">
							{#each schemaFields.slice(0, 8) as field}
								<span class="field-chip" class:required={field.required}>
									{field.name}
									<span>{field.type}</span>
								</span>
							{/each}
							{#if schemaFields.length > 8}
								<span class="field-chip">+{schemaFields.length - 8}</span>
							{/if}
						</div>
					{/if}
				</div>
			{/if}

			{#if createOpen || entities.length === 0}
				<section class="create-entity-inline" aria-labelledby="create-entity-title">
					<div class="create-entity-head">
						<h3 id="create-entity-title">Create Entity</h3>
						{#if collection?.schema?.version}
							<span class="pill">Schema v{collection.schema.version}</span>
						{/if}
					</div>
					{#if requiredSchemaFields.length > 0}
						<div class="required-fields">
							<span class="meta-label">Required</span>
							{#each requiredSchemaFields as field}
								<span class="field-chip required">{field.name}</span>
							{/each}
						</div>
					{/if}
					<label>
						<span>Entity ID</span>
						<input bind:value={createId} placeholder="task-001" />
					</label>
					<label>
						<span>Entity JSON</span>
						<textarea bind:value={createJson} rows="8"></textarea>
					</label>
					{#if createErrors.length > 0}
						<div class="message error">
							{#each createErrors as issue}
								<p>{issue}</p>
							{/each}
						</div>
					{/if}
					<div class="actions">
						<button class="primary" onclick={submitCreateEntity}>Create Entity</button>
					</div>
				</section>
			{/if}

			{#if loading}
				<p class="message">Loading entities...</p>
			{:else if entities.length === 0}
				<p class="muted">No entities yet.</p>
			{:else}
				<table>
					<thead>
						<tr>
							<th>ID</th>
							<th>Version</th>
							<th>Preview</th>
						</tr>
					</thead>
					<tbody>
						{#each entities as entity}
							<tr
								class:selected={selectedEntity?.id === entity.id}
								onclick={() => openEntity(entity.id)}
							>
								<td>{entity.id}</td>
								<td>{entity.version}</td>
								<td><code>{JSON.stringify(entity.data).slice(0, 80)}</code></td>
							</tr>
						{/each}
					</tbody>
				</table>
			{/if}
		</div>
	</section>

	<section class="panel entity-detail-panel">
		<div class="panel-header">
			<h2>{selectedEntity ? selectedEntity.id : 'Entity Detail'}</h2>
			{#if selectedEntity}
				<div class="actions">
					<span class="pill">v{selectedEntity.version}</span>
					{#if editMode}
						<button onclick={cancelEdit}>Cancel</button>
						<button disabled={previewingIntent || saving} onclick={previewEntityIntent}>
							{previewingIntent ? 'Previewing...' : 'Preview'}
						</button>
						<button class="primary" disabled={saving} onclick={saveEntity}>
							{saving ? 'Saving...' : 'Save'}
						</button>
					{:else}
						<button onclick={startEdit}>Edit</button>
						{#if confirmDelete}
							<span class="muted" style="font-size:0.85rem">Delete?</span>
							<button
								class="danger"
								onclick={async () => {
									if (selectedEntity && collectionName) {
										try {
											await deleteEntity(collectionName, selectedEntity.id, scope);
											deleteMessage = `Deleted ${selectedEntity.id}.`;
											confirmDelete = false;
											selectedEntity = null;
											await loadCollection(collectionName, null);
										} catch (e: unknown) {
											error = e instanceof Error ? e.message : 'Failed to delete';
											confirmDelete = false;
										}
									}
								}}
							>
								Confirm
							</button>
							<button onclick={() => (confirmDelete = false)}>Cancel</button>
						{:else}
							<button class="danger" onclick={() => (confirmDelete = true)}>Delete</button>
						{/if}
					{/if}
				</div>
			{/if}
		</div>
		<div class="panel-body stack">
			{#if collection}
				<p class="muted">
					{collection.entity_count} entities · {collection.schema
						? `schema v${collection.schema.version}`
						: 'no schema'}
				</p>
			{/if}

			{#if saveMessage}
				<p class="message success">{saveMessage}</p>
			{/if}
			{#if saveError}
				<p class="message error">{saveError}</p>
			{/if}

			{#if selectedEntity}
				<div class="entity-meta">
					<div class="meta-row">
						<span class="meta-label">ID</span>
						<span class="meta-value"><code>{selectedEntity.id}</code></span>
					</div>
					<div class="meta-row">
						<span class="meta-label">Collection</span>
						<span class="meta-value"><code>{selectedEntity.collection}</code></span>
					</div>
					<div class="meta-row">
						<span class="meta-label">Version</span>
						<span class="meta-value">{selectedEntity.version}</span>
					</div>
					{#if selectedEntity.schema_version != null}
						<div class="meta-row">
							<span class="meta-label">Schema Version</span>
							<span class="meta-value">{selectedEntity.schema_version}</span>
						</div>
					{/if}
				</div>

				<div class="tab-bar" role="tablist">
					<button
						class:active={activeTab === 'data'}
						role="tab"
						data-testid="entity-tab-data"
						onclick={() => (activeTab = 'data')}
					>
						Data
					</button>
					<button
						class:active={activeTab === 'audit'}
						role="tab"
						data-testid="entity-tab-audit"
						onclick={() => (activeTab = 'audit')}
					>
						History
					</button>
					<button
						class:active={activeTab === 'links'}
						role="tab"
						data-testid="entity-tab-links"
						onclick={() => (activeTab = 'links')}
					>
						Links
					</button>
					{#if Object.keys(lifecycleDefs).length > 0}
						<button
							class:active={activeTab === 'lifecycle'}
							role="tab"
							data-testid="entity-tab-lifecycle"
							onclick={() => (activeTab = 'lifecycle')}
						>
							Lifecycle
						</button>
					{/if}
					<button
						class:active={activeTab === 'markdown'}
						role="tab"
						data-testid="entity-tab-markdown"
						onclick={() => (activeTab = 'markdown')}
					>
						Markdown
					</button>
					<button
						class:active={activeTab === 'rollback'}
						role="tab"
						data-testid="entity-tab-rollback"
						onclick={() => {
							activeTab = 'rollback';
							void ensureAuditLoaded();
						}}
					>
						Rollback
					</button>
				</div>

				{#if activeTab === 'data'}
					<div class="tree-container">
						<div class="tree-header">
							<span class="tree-title">Data</span>
							<span class="type-badge">
								object{'{' +
									Object.keys(editMode && editData ? editData : selectedEntity.data).length +
									'}'}
							</span>
						</div>
						{#if editMode && editData}
							<JsonTree
								data={editData as unknown as JsonValue}
								editing={true}
								onupdate={handleTreeUpdate}
							/>
						{:else}
							<JsonTree data={selectedEntity.data as unknown as JsonValue} />
						{/if}
					</div>
				{:else if activeTab === 'audit'}
					<div class="tab-pane">
						{#if auditLoading}
							<p class="muted">Loading history…</p>
						{:else if auditError}
							<p class="message error">{auditError}</p>
						{:else if auditEntries.length === 0}
							<p class="muted">No audit entries for this entity.</p>
						{:else}
							{#if revertMessage}
								<p class="message success">{revertMessage}</p>
							{/if}
							{#if revertError}
								<p class="message error">{revertError}</p>
							{/if}
							<ol class="audit-timeline" data-testid="entity-audit-timeline">
								{#each auditEntries as entry}
									<li class="audit-entry">
										<div class="audit-head">
											<strong>v{entry.version}</strong>
											<span class="pill">{entry.mutation}</span>
											<span class="muted">
												{new Date(entry.timestamp_ns / 1_000_000).toLocaleString()}
											</span>
											<span class="muted">· {entry.actor ?? 'system'}</span>
											{#if entry.data_before !== null}
												{#if revertConfirmId === entry.id}
													<span>Revert entry #{entry.id}?</span>
													<button class="danger" onclick={() => void doRevertAuditEntry(entry.id)}>Yes</button>
													<button onclick={() => (revertConfirmId = null)}>No</button>
												{:else}
													<button onclick={() => { revertConfirmId = entry.id; revertMessage = null; revertError = null; }}>
														Revert
													</button>
												{/if}
											{/if}
										</div>
										{#if entry.data_after}
											<details>
												<summary>After</summary>
												<pre>{JSON.stringify(entry.data_after, null, 2)}</pre>
											</details>
										{/if}
										{#if entry.data_before}
											<details>
												<summary>Before</summary>
												<pre>{JSON.stringify(entry.data_before, null, 2)}</pre>
											</details>
										{/if}
									</li>
								{/each}
							</ol>
						{/if}
					</div>
				{:else if activeTab === 'links'}
					<div class="tab-pane stack">
						{#if linksError}
							<p class="message error">{linksError}</p>
						{/if}
						<div class="links-header">
							<span class="muted">{links.length} outbound link{links.length === 1 ? '' : 's'}</span>
							<button onclick={() => (showCreateLink = !showCreateLink)}>
								{showCreateLink ? 'Cancel' : 'Add Link'}
							</button>
						</div>
						{#if showCreateLink}
							<form
								class="create-link-form stack"
								onsubmit={(e) => {
									e.preventDefault();
									void submitCreateLink();
								}}
							>
								<label>
									<span>Link type</span>
									<input
										bind:value={newLinkType}
										placeholder="depends-on"
										data-testid="link-type-input"
									/>
								</label>
								<label>
									<span>Target collection</span>
									<input
										bind:value={newLinkTargetCollection}
										placeholder="tasks"
										data-testid="link-target-collection-input"
									/>
								</label>
								<label>
									<span>Target entity ID</span>
									<input
										bind:value={newLinkTargetId}
										placeholder="t-002"
										data-testid="link-target-id-input"
									/>
								</label>
								{#if createLinkError}
									<p class="message error">{createLinkError}</p>
								{/if}
								<div class="actions">
									<button type="submit" class="primary" data-testid="link-submit">
										Create Link
									</button>
								</div>
							</form>
						{/if}
						{#if linksLoading}
							<p class="muted">Loading links…</p>
						{:else if links.length === 0}
							<p class="muted">No outbound links. Use "Add Link" to create one.</p>
						{:else}
							<table data-testid="entity-links-table">
								<thead>
									<tr>
										<th>Type</th>
										<th>Target</th>
										<th></th>
									</tr>
								</thead>
								<tbody>
									{#each links as link}
										<tr>
											<td><code>{link.link_type}</code></td>
											<td>
												<code>{link.target_collection}/{link.target_id}</code>
											</td>
											<td>
												<button class="danger" onclick={() => void removeLink(link)}>
													Remove
												</button>
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						{/if}
					</div>
				{:else if activeTab === 'lifecycle'}
					<div class="tab-pane stack">
						{#if lifecycleError}
							<p class="message error">{lifecycleError}</p>
						{/if}
						{#each Object.entries(lifecycleDefs) as [name, def]}
							{@const state = currentLifecycleState(def)}
							{@const next = allowedTransitions(def)}
							<div class="lifecycle-card">
								<div class="lifecycle-head">
									<strong>{name}</strong>
									<span class="muted">field: <code>{def.field}</code></span>
								</div>
								<div class="lifecycle-state">
									Current: <span class="pill" data-testid="lifecycle-current-state">
										{state ?? '(unset)'}
									</span>
								</div>
								{#if next.length === 0}
									<p class="muted">No allowed transitions from this state.</p>
								{:else}
									<div class="lifecycle-actions">
										{#each next as target}
											<button
												disabled={transitioning}
												data-testid={`lifecycle-transition-${target}`}
												onclick={() => void doTransition(name, target)}
											>
												→ {target}
											</button>
										{/each}
									</div>
								{/if}
							</div>
						{/each}
					</div>
				{:else if activeTab === 'markdown'}
					<div class="tab-pane stack">
						{#if renderedError}
							<p class="message error">{renderedError}</p>
						{:else if renderedMarkdown === null}
							<p class="muted">Loading rendered markdown…</p>
						{:else if renderedMarkdown.trim() === ''}
							<p class="muted">
								No markdown rendered. Set a template in the Collection Template section above.
							</p>
						{:else}
							<pre data-testid="entity-markdown-output">{renderedMarkdown}</pre>
						{/if}
					</div>
				{:else if activeTab === 'rollback'}
					<div class="tab-pane stack" data-testid="entity-rollback-pane">
						{#if rollbackApplyMessage}
							<p class="message success">{rollbackApplyMessage}</p>
						{/if}
						{#if rollbackApplyError}
							<p class="message error">{rollbackApplyError}</p>
						{/if}
						<h3 style="font-size:0.9rem;margin:0">Version History</h3>
						{#if auditLoading}
							<p class="muted">Loading history…</p>
						{:else if auditError}
							<p class="message error">{auditError}</p>
						{:else if auditEntries.length <= 1}
							<p class="muted">No prior versions to roll back to.</p>
						{:else}
							<table data-testid="entity-rollback-table">
								<thead>
									<tr>
										<th>Version</th>
										<th>Operation</th>
										<th>Actor</th>
										<th>Timestamp</th>
										<th>Data Preview</th>
										<th></th>
									</tr>
								</thead>
								<tbody>
									{#each auditEntries.filter((e) => e.version < (selectedEntity?.version ?? 0)) as entry}
										<tr>
											<td><strong>v{entry.version}</strong></td>
											<td><span class="pill">{entry.mutation}</span></td>
											<td>{entry.actor ?? 'system'}</td>
											<td class="muted" style="font-size:0.78rem">
												{new Date(entry.timestamp_ns / 1_000_000).toLocaleString()}
											</td>
											<td>
												<code style="font-size:0.75rem">
													{JSON.stringify(entry.data_after).slice(0, 60)}
												</code>
											</td>
											<td>
												<button
													onclick={() => void doPreviewRollback(entry.version)}
													data-testid={`rollback-preview-v${entry.version}`}
												>
													Preview
												</button>
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						{/if}

						{#if rollbackPreviewLoading}
							<p class="muted">Loading preview…</p>
						{/if}
						{#if rollbackPreviewError}
							<p class="message error">{rollbackPreviewError}</p>
						{/if}
						{#if rollbackPreview !== null && rollbackPreviewVersion !== null}
							<div class="rollback-preview" data-testid="entity-rollback-preview">
								<div class="rollback-preview-header">
									<h3 style="font-size:0.9rem;margin:0">
										Preview: Roll back to v{rollbackPreviewVersion}
									</h3>
									<button
										class="primary"
										disabled={rollbackApplying}
										onclick={() => void doApplyRollback()}
										data-testid="rollback-apply-button"
									>
										{rollbackApplying ? 'Applying…' : 'Apply Rollback'}
									</button>
								</div>
								<h4 style="font-size:0.82rem;margin:0.5rem 0 0.25rem">Target data</h4>
								<pre style="font-size:0.78rem">{JSON.stringify(rollbackPreview.target.data, null, 2)}</pre>
								{#if Object.keys(rollbackPreview.diff).length > 0}
									<h4 style="font-size:0.82rem;margin:0.5rem 0 0.25rem">Field changes</h4>
									<table data-testid="entity-rollback-diff-table">
										<thead>
											<tr>
												<th>Field</th>
												<th>Kind</th>
												<th>Description</th>
											</tr>
										</thead>
										<tbody>
											{#each Object.entries(rollbackPreview.diff) as [field, d]}
												{@const diff = d as FieldDiff}
												<tr>
													<td><code>{field}</code></td>
													<td><span class="pill">{diff.kind}</span></td>
													<td>{diff.description}</td>
												</tr>
											{/each}
										</tbody>
									</table>
								{:else}
									<p class="muted">No field-level diff returned.</p>
								{/if}
							</div>
						{/if}
					</div>
				{/if}
			{:else}
				<p class="muted">Select an entity row to inspect its data.</p>
			{/if}
		</div>
	</section>
</div>

<section class="panel template-editor" data-testid="collection-template-section">
	<div class="panel-header">
		<h2>Markdown Template</h2>
		<div class="actions">
			{#if templateEditMode}
				<button
					onclick={() => {
						templateEditMode = false;
						templateDraft = template?.template ?? '';
						templateError = null;
					}}
				>
					Cancel
				</button>
				<button class="primary" disabled={templateSaving} onclick={() => void saveTemplate()}>
					{templateSaving ? 'Saving...' : 'Save'}
				</button>
			{:else}
				<button onclick={() => (templateEditMode = true)} data-testid="template-edit-button">
					{template ? 'Edit' : 'Create'}
				</button>
				{#if template}
					<button class="danger" onclick={() => void deleteTemplate()}>Delete</button>
				{/if}
			{/if}
		</div>
	</div>
	<div class="panel-body stack">
		{#if templateError}
			<p class="message error">{templateError}</p>
		{/if}
		{#if templateStatus && !templateError}
			<p class="message success">{templateStatus}</p>
		{/if}
		{#if templateEditMode}
			<textarea
				bind:value={templateDraft}
				data-testid="template-editor-textarea"
				placeholder={'# {{title}}\n\n{{description}}'}
			></textarea>
			<p class="muted">
				Mustache-style syntax. Use <code>{'{{field}}'}</code> to interpolate entity fields.
			</p>
		{:else if template}
			<pre data-testid="template-display">{template.template}</pre>
			<p class="muted">
				v{template.version}
				{#if template.updated_at_ns}
					· updated {new Date(template.updated_at_ns / 1_000_000).toLocaleString()}
				{/if}
			</p>
		{:else}
			<p class="muted">
				No markdown template set for this collection. Click <strong>Create</strong> to add one.
			</p>
		{/if}
	</div>
</section>

<MutationIntentPreviewModal
	open={intentModalOpen}
	preview={intentPreview}
	commitOutcome={intentCommitOutcome}
	committing={committingIntent}
	onClose={() => {
		intentModalOpen = false;
	}}
	onCommit={commitPreviewIntent}
/>

<style>
	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}

	.entity-workspace {
		display: grid;
		grid-template-columns: minmax(22rem, 0.85fr) minmax(0, 1.35fr);
		gap: 1rem;
		align-items: start;
		margin-bottom: 1rem;
	}

	.entity-rail {
		position: sticky;
		top: 1rem;
		max-height: calc(100vh - 7.5rem);
		overflow: hidden;
	}

	.entity-rail .panel-body {
		max-height: calc(100vh - 12rem);
		overflow: auto;
	}

	.entity-detail-panel {
		min-width: 0;
	}

	.schema-context,
	.create-entity-inline {
		border: 1px solid rgba(47, 55, 66, 0.8);
		border-radius: 0.5rem;
		background: rgba(15, 23, 32, 0.45);
	}

	.schema-context {
		display: flex;
		flex-direction: column;
		gap: 0.55rem;
		padding: 0.75rem;
	}

	.schema-context-row,
	.required-fields,
	.field-chip-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.45rem;
	}

	.schema-context-row a {
		color: var(--accent);
		font-weight: 700;
		text-decoration: none;
	}

	.field-chip {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		max-width: 100%;
		border: 1px solid rgba(148, 163, 184, 0.28);
		border-radius: 0.45rem;
		padding: 0.18rem 0.45rem;
		color: var(--text);
		font-size: 0.78rem;
		background: rgba(15, 23, 32, 0.65);
	}

	.field-chip span {
		color: var(--muted);
	}

	.field-chip.required {
		border-color: rgba(125, 211, 252, 0.45);
		color: var(--accent);
	}

	.create-entity-inline {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		padding: 0.85rem;
	}

	.create-entity-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.75rem;
	}

	.create-entity-head h3 {
		margin: 0;
		font-size: 0.95rem;
	}

	.create-entity-inline label {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	.create-entity-inline label span {
		color: var(--muted);
		font-size: 0.78rem;
		font-weight: 700;
		text-transform: uppercase;
	}

	.create-entity-inline textarea {
		min-height: 8rem;
	}

	tr {
		cursor: pointer;
		transition: background 80ms ease;
	}

	tr:hover {
		background: rgba(125, 211, 252, 0.06);
	}

	tr.selected {
		background: rgba(125, 211, 252, 0.1);
		border-left: 2px solid var(--accent-strong);
	}

	.entity-meta {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem 1.5rem;
		padding: 0.75rem 0;
		border-bottom: 1px solid rgba(47, 55, 66, 0.5);
	}

	.meta-row {
		display: flex;
		align-items: center;
		gap: 0.4rem;
	}

	.meta-label {
		color: var(--muted);
		font-size: 0.82rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.meta-value {
		font-size: 0.88rem;
	}

	.meta-value code {
		font-size: 0.85rem;
	}

	.tree-container {
		padding: 0.5rem 0;
	}

	.tree-header {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding-bottom: 0.4rem;
		border-bottom: 1px solid rgba(47, 55, 66, 0.4);
		margin-bottom: 0.3rem;
	}

	.tree-title {
		font-weight: 600;
		font-size: 0.9rem;
	}

	.type-badge {
		display: inline-flex;
		align-items: center;
		border: 1px solid rgba(125, 211, 252, 0.2);
		border-radius: 999px;
		padding: 0.05rem 0.45rem;
		color: var(--muted);
		font-size: 0.72rem;
		font-weight: 500;
	}

	/* ── Entity detail tabs ─────────────────────────────────────────── */

	.tab-bar {
		display: flex;
		gap: 0.25rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
		margin-bottom: 0.75rem;
	}

	.tab-bar button {
		background: none;
		border: none;
		padding: 0.4rem 0.8rem;
		color: var(--muted);
		font-size: 0.88rem;
		cursor: pointer;
		border-bottom: 2px solid transparent;
	}

	.tab-bar button:hover {
		color: var(--text);
	}

	.tab-bar button.active {
		color: var(--text);
		border-bottom-color: var(--accent-strong, #7dd3fc);
	}

	.tab-pane {
		padding: 0.25rem 0;
	}

	.audit-timeline {
		list-style: none;
		padding: 0;
		margin: 0;
	}

	.audit-entry {
		border-left: 2px solid rgba(125, 211, 252, 0.25);
		padding: 0.4rem 0 0.4rem 0.75rem;
		margin-bottom: 0.5rem;
	}

	.audit-head {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		font-size: 0.88rem;
	}

	.audit-entry pre {
		font-size: 0.78rem;
		background: rgba(0, 0, 0, 0.2);
		padding: 0.5rem;
		border-radius: 0.3rem;
		margin: 0.3rem 0 0;
		white-space: pre-wrap;
	}

	.links-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
	}

	.create-link-form {
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
		padding: 0.75rem;
	}

	.lifecycle-card {
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
		padding: 0.75rem;
	}

	.lifecycle-head {
		display: flex;
		gap: 0.75rem;
		align-items: baseline;
		margin-bottom: 0.35rem;
	}

	.lifecycle-state {
		font-size: 0.88rem;
		margin-bottom: 0.5rem;
	}

	.lifecycle-actions {
		display: flex;
		gap: 0.35rem;
		flex-wrap: wrap;
	}

	.template-editor textarea {
		width: 100%;
		min-height: 8rem;
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.85rem;
	}

	.rollback-preview {
		border: 1px solid rgba(125, 211, 252, 0.2);
		border-radius: 0.5rem;
		padding: 0.75rem;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.rollback-preview-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
	}

	.rollback-preview pre {
		font-size: 0.78rem;
		background: rgba(0, 0, 0, 0.2);
		padding: 0.5rem;
		border-radius: 0.3rem;
		white-space: pre-wrap;
		margin: 0;
	}

	@media (max-width: 1150px) {
		.entity-workspace {
			grid-template-columns: 1fr;
		}

		.entity-rail {
			position: static;
			max-height: none;
		}

		.entity-rail .panel-body {
			max-height: none;
		}
	}
</style>
