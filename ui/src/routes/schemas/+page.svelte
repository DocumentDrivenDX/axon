<script lang="ts">
import {
	type CollectionSchema,
	type CollectionSummary,
	createCollection,
	fetchCollections,
	fetchSchema,
	updateSchema,
} from '$lib/api';
import { onMount } from 'svelte';

let collections: CollectionSummary[] = [];
let selectedCollection = '';
let selectedSchema: CollectionSchema | null = null;
let editMode = false;
let editJson = '';
let validationError: string | null = null;
let statusMessage: string | null = null;
let error: string | null = null;
let createCollectionName = '';
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createSchemaJson = `{
  "type": "object",
  "properties": {}
}`;

async function loadCollections(preferredCollection?: string) {
	collections = await fetchCollections();
	const nextSelection = preferredCollection ?? selectedCollection ?? collections[0]?.name;

	if (nextSelection) {
		await selectCollection(nextSelection);
	}
}

async function selectCollection(collectionName: string) {
	selectedCollection = collectionName;
	selectedSchema = await fetchSchema(collectionName);
	editJson = JSON.stringify(selectedSchema, null, 2);
	editMode = false;
	validationError = null;
	statusMessage = null;
	error = null;
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

async function saveSchema() {
	if (!selectedCollection || !validateJson()) {
		return;
	}

	try {
		selectedSchema = await updateSchema(
			selectedCollection,
			JSON.parse(editJson) as CollectionSchema,
		);
		editJson = JSON.stringify(selectedSchema, null, 2);
		editMode = false;
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
		await createCollection(createCollectionName, {
			description: null,
			version: 1,
			entity_schema: entitySchema,
			link_types: {},
		});
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
						<button on:click={() => (editMode = true)}>Edit</button>
					{/if}
				</div>
			</div>
			<div class="panel-body stack">
				{#if !selectedSchema}
					<p class="muted">Select a collection to inspect its schema.</p>
				{:else if editMode}
					<textarea bind:value={editJson} on:input={validateJson}></textarea>
					{#if validationError}
						<p class="message error">{validationError}</p>
					{/if}
					{#if statusMessage && !validationError}
						<p class="message">{statusMessage}</p>
					{/if}
					<div class="actions">
						<button
							on:click={() => {
								editMode = false;
								editJson = JSON.stringify(selectedSchema, null, 2);
								validationError = null;
								statusMessage = null;
							}}
						>
							Cancel
						</button>
						<button class="primary" disabled={!!validationError} on:click={saveSchema}>
							Save Schema
						</button>
					</div>
				{:else}
					<pre>{JSON.stringify(selectedSchema, null, 2)}</pre>
				{/if}
			</div>
		</section>
	</section>
</div>
