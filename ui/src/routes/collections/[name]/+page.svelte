<script lang="ts">
import { afterNavigate } from '$app/navigation';
import {
	type CollectionDetail,
	type EntityRecord,
	createEntity,
	deleteEntity,
	fetchCollection,
	fetchEntities,
	fetchEntity,
	updateEntity,
} from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template for entity data tree.
import JsonTree from '$lib/components/JsonTree.svelte';
// biome-ignore lint/correctness/noUnusedImports: Used in template for casting entity data.
import type { JsonValue } from '$lib/components/json-tree-types';
import { validateEntityData } from '$lib/schema-validation';
import { getSelectedTenant } from '$lib/stores.svelte';
import { onMount } from 'svelte';

let collectionName = $state('');
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

let confirmDelete = $state(false);
let deleteMessage = $state<string | null>(null);

async function loadCollection(targetCollection: string, afterId: string | null, dbName?: string) {
	loading = true;
	try {
		collection = await fetchCollection(targetCollection, dbName);
		const result = await fetchEntities(targetCollection, {
			limit: 50,
			afterId,
		}, dbName);
		entities = result.entities;
		nextCursor = result.next_cursor;
		selectedEntity = entities[0] ? await fetchEntity(targetCollection, entities[0].id, dbName) : null;
		editMode = false;
		editData = null;
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
		selectedEntity = await fetchEntity(collectionName, id, getSelectedTenant()?.db_name);
		editMode = false;
		editData = null;
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
	saveError = null;
	saveMessage = null;
}

function cancelEdit() {
	editMode = false;
	editData = null;
	saveError = null;
}

async function saveEntity() {
	if (!selectedEntity || !editData || !collectionName) return;
	saving = true;
	saveError = null;
	saveMessage = null;

	if (collection?.schema?.entity_schema) {
		const issues = validateEntityData(collection.schema.entity_schema, editData);
		if (issues.length > 0) {
			saveError = issues.join('; ');
			saving = false;
			return;
		}
	}

	try {
		const updated = await updateEntity(
			collectionName,
			selectedEntity.id,
			editData,
			selectedEntity.version,
			getSelectedTenant()?.db_name,
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
		const entity = await createEntity(collectionName, createId.trim(), parsedData, getSelectedTenant()?.db_name);
		createMessage = `Created ${entity.id}.`;
		createErrors = [];
		createOpen = false;
		paginationHistory = [null];
		pageIndex = 0;
		await loadCollection(collectionName, null, getSelectedTenant()?.db_name);
		selectedEntity = entity;
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
	await loadCollection(collectionName, nextCursor, getSelectedTenant()?.db_name);
}

async function previousPage() {
	if (pageIndex === 0) {
		return;
	}

	pageIndex -= 1;
	await loadCollection(collectionName, paginationHistory[pageIndex] ?? null, getSelectedTenant()?.db_name);
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
	await loadCollection(routeCollectionName, null, getSelectedTenant()?.db_name);
}

onMount(() => {
	void syncRoute();
});

afterNavigate(() => {
	void syncRoute();
});

let lastLoadedDbName: string | undefined | null = null;
$effect(() => {
	const tenant = getSelectedTenant();
	const dbName = tenant?.db_name;
	// Skip the first run (syncRoute on mount already handles the initial load),
	// and only reload when the tenant actually changes.
	if (lastLoadedDbName === null) {
		lastLoadedDbName = dbName;
		return;
	}
	if (dbName === lastLoadedDbName) {
		return;
	}
	lastLoadedDbName = dbName;
	if (collectionName) {
		paginationHistory = [null];
		pageIndex = 0;
		selectedEntity = null;
		void loadCollection(collectionName, null, dbName);
	}
});
</script>

<div class="page-header">
	<div>
		<h1>{collectionName}</h1>
		<p class="muted">Entity browser with 50-row pagination and tree-style JSON detail.</p>
	</div>
	<div class="actions">
		<button onclick={() => (createOpen = !createOpen)}>
			{createOpen ? 'Hide Create Entity' : 'Create Entity'}
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

{#if createOpen || entities.length === 0}
	<section class="panel">
		<div class="panel-header">
			<h2>Create Entity</h2>
			{#if collection?.schema?.version}
				<span class="pill">Schema v{collection.schema.version}</span>
			{/if}
		</div>
		<div class="panel-body stack">
			{#if entities.length === 0}
				<p class="muted">
					This collection is empty. Create the first entity to populate the browser.
				</p>
			{/if}
			<label>
				<span>Entity ID</span>
				<input bind:value={createId} placeholder="task-001" />
			</label>
			<label>
				<span>Entity JSON</span>
				<textarea bind:value={createJson}></textarea>
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
		</div>
	</section>
{/if}

<div class="two-column">
	<section class="panel">
		<div class="panel-header">
			<h2>Entities</h2>
			<div class="actions">
				<button disabled={pageIndex === 0} onclick={previousPage}>Previous</button>
				<button disabled={!nextCursor} onclick={nextPage}>Next</button>
			</div>
		</div>
		<div class="panel-body">
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

	<section class="panel">
		<div class="panel-header">
			<h2>{selectedEntity ? selectedEntity.id : 'Entity Detail'}</h2>
			{#if selectedEntity}
				<div class="actions">
					<span class="pill">v{selectedEntity.version}</span>
					{#if editMode}
						<button onclick={cancelEdit}>Cancel</button>
						<button class="primary" disabled={saving} onclick={saveEntity}>
							{saving ? 'Saving...' : 'Save'}
						</button>
					{:else}
						<button onclick={startEdit}>Edit</button>
						{#if confirmDelete}
							<span class="muted" style="font-size:0.85rem">Delete?</span>
							<button class="danger" onclick={async () => {
								if (selectedEntity && collectionName) {
									try {
										await deleteEntity(collectionName, selectedEntity.id, getSelectedTenant()?.db_name);
										deleteMessage = `Deleted ${selectedEntity.id}.`;
										confirmDelete = false;
										selectedEntity = null;
										await loadCollection(collectionName, null, getSelectedTenant()?.db_name);
									} catch (e: unknown) {
										error = e instanceof Error ? e.message : 'Failed to delete';
										confirmDelete = false;
									}
								}
							}}>Confirm</button>
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

				<div class="tree-container">
					<div class="tree-header">
						<span class="tree-title">Data</span>
						<span class="type-badge">object{'{' + Object.keys(editMode && editData ? editData : selectedEntity.data).length + '}'}</span>
					</div>
					{#if editMode && editData}
						<JsonTree data={editData as unknown as JsonValue} editing={true} onupdate={handleTreeUpdate} />
					{:else}
						<JsonTree data={selectedEntity.data as unknown as JsonValue} />
					{/if}
				</div>
			{:else}
				<p class="muted">Select an entity row to inspect its data.</p>
			{/if}
		</div>
	</section>
</div>

<style>
	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
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
</style>
