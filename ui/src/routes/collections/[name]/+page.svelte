<script lang="ts">
import { afterNavigate } from '$app/navigation';
import {
	type CollectionDetail,
	type EntityRecord,
	createEntity,
	fetchCollection,
	fetchEntities,
	fetchEntity,
} from '$lib/api';
import { validateEntityData } from '$lib/schema-validation';
import { onMount } from 'svelte';

let collectionName = '';
let collection: CollectionDetail | null = null;
let entities: EntityRecord[] = [];
let selectedEntity: EntityRecord | null = null;
let loading = true;
let error: string | null = null;
let nextCursor: string | null = null;
let paginationHistory: Array<string | null> = [null];
let pageIndex = 0;

let createOpen = false;
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createId = '';
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createJson = `{
  "title": ""
}`;
let createErrors: string[] = [];
let createMessage: string | null = null;

async function loadCollection(targetCollection: string, afterId: string | null) {
	loading = true;
	try {
		collection = await fetchCollection(targetCollection);
		const result = await fetchEntities(targetCollection, {
			limit: 50,
			afterId,
		});
		entities = result.entities;
		nextCursor = result.next_cursor;
		selectedEntity = entities[0] ? await fetchEntity(targetCollection, entities[0].id) : null;
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
		selectedEntity = await fetchEntity(collectionName, id);
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load entity';
	}
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
		const entity = await createEntity(collectionName, createId.trim(), parsedData);
		createMessage = `Created ${entity.id}.`;
		createErrors = [];
		createOpen = false;
		paginationHistory = [null];
		pageIndex = 0;
		await loadCollection(collectionName, null);
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
		<p class="muted">Entity browser with 50-row pagination and JSON detail.</p>
	</div>
	<div class="actions">
		<button on:click={() => (createOpen = !createOpen)}>
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
				<button class="primary" on:click={submitCreateEntity}>Create Entity</button>
			</div>
		</div>
	</section>
{/if}

<div class="two-column">
	<section class="panel">
		<div class="panel-header">
			<h2>Entities</h2>
			<div class="actions">
				<button disabled={pageIndex === 0} on:click={previousPage}>Previous</button>
				<button disabled={!nextCursor} on:click={nextPage}>Next</button>
			</div>
		</div>
		<div class="panel-body">
			{#if loading}
				<p class="message">Loading entities…</p>
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
							<tr on:click={() => openEntity(entity.id)}>
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
				<span class="pill">v{selectedEntity.version}</span>
			{/if}
		</div>
		<div class="panel-body stack">
			{#if collection}
				<p class="muted">
					{collection.entity_count} entities · {collection.schema ? `schema v${collection.schema.version}` : 'no schema'}
				</p>
			{/if}
			{#if selectedEntity}
				<pre>{JSON.stringify(selectedEntity, null, 2)}</pre>
			{:else}
				<p class="muted">Select an entity row to inspect its full JSON payload.</p>
			{/if}
		</div>
	</section>
</div>
