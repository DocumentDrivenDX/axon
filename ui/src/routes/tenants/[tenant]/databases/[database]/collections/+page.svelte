<script lang="ts">
import { base } from '$app/paths';
import {
	type CollectionSummary,
	createCollection,
	// biome-ignore lint/correctness/noUnusedImports: Used in template onclick handler.
	dropCollection,
	fetchCollections,
} from '$lib/api';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();

const scope = $derived(data.scope);
const basePath = $derived(
	`${base}/tenants/${encodeURIComponent(data.tenant.db_name)}/databases/${encodeURIComponent(data.database.name)}`,
);
const schemasHref = $derived(`${basePath}/schemas`);

let collections = $state<CollectionSummary[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
let createMessage = $state<string | null>(null);
let createError = $state<string | null>(null);
let creating = $state(false);
let createCollectionName = $state('');
// biome-ignore lint/style/useConst: Svelte bind:value mutates this state.
let createSchemaJson = $state(`{
  "type": "object",
  "properties": {}
}`);
// biome-ignore lint/style/useConst: Svelte template onclick handlers mutate this state.
let dropping = $state<string | null>(null);

function collectionHref(name: string): string {
	return `${basePath}/collections/${encodeURIComponent(name)}`;
}

function schemaHref(name: string): string {
	return `${schemasHref}?collection=${encodeURIComponent(name)}`;
}

function formatTimestamp(ns: number | null | undefined): string {
	if (!ns) {
		return '\u2014';
	}
	return new Date(ns / 1_000_000).toLocaleDateString();
}

async function loadCollections() {
	loading = true;
	try {
		collections = await fetchCollections(scope);
		error = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load collections';
	} finally {
		loading = false;
	}
}

async function submitCreateCollection() {
	const collectionName = createCollectionName.trim();
	createError = null;
	createMessage = null;

	if (!collectionName) {
		createError = 'Collection name is required.';
		return;
	}

	let entitySchema: unknown = null;
	try {
		entitySchema = createSchemaJson.trim() ? (JSON.parse(createSchemaJson) as unknown) : null;
	} catch (errorValue: unknown) {
		createError =
			errorValue instanceof Error ? errorValue.message : 'Entity schema JSON is invalid.';
		return;
	}

	creating = true;
	try {
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
		createMessage = `Created ${collectionName}.`;
		error = null;
		await loadCollections();
	} catch (errorValue: unknown) {
		createError = errorValue instanceof Error ? errorValue.message : 'Failed to create collection';
	} finally {
		creating = false;
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
		<h1>Collections</h1>
		<p class="muted">Browse registered collections with entity counts and schema versions.</p>
	</div>
	<div class="actions">
		<button onclick={() => loadCollections()}>Refresh</button>
		<a class="button-link" href={schemasHref}>Schema Workspace</a>
	</div>
</div>

{#if loading}
	<p class="message">Loading collections...</p>
{:else if error}
	<p class="message error">{error}</p>
{:else}
	<div class="collections-workspace">
		<section class="panel collection-create-panel">
			<div class="panel-header">
				<h2>Create Collection</h2>
				<a class="button-link" href={schemasHref}>Edit Schemas</a>
			</div>
			<form
				class="panel-body stack"
				data-testid="collections-create-form"
				onsubmit={(event) => {
					event.preventDefault();
					void submitCreateCollection();
				}}
			>
				<p class="muted">
					Register a collection in this database with an initial entity schema. Use the
					schema workspace for compatibility previews and later edits.
				</p>
				<label>
					<span>Collection name</span>
					<input bind:value={createCollectionName} placeholder="tasks" autocomplete="off" />
				</label>
				<label>
					<span>Entity Schema JSON</span>
					<textarea bind:value={createSchemaJson} rows="8"></textarea>
				</label>
				{#if createError}
					<p class="message error">{createError}</p>
				{/if}
				{#if createMessage}
					<p class="message success">{createMessage}</p>
				{/if}
				<div class="actions">
					<button class="primary" disabled={creating || !createCollectionName.trim()} type="submit">
						{creating ? 'Creating...' : 'Create Collection'}
					</button>
				</div>
			</form>
		</section>

		<section class="panel collection-list-panel">
			<div class="panel-header">
				<h2>Registered Collections</h2>
				<div class="actions">
					<span class="pill">{collections.length} collections</span>
				</div>
			</div>
			<div class="panel-body">
				{#if collections.length === 0}
					<div class="empty-collections stack">
						<h3>No collections yet</h3>
						<p class="muted">Create one here to start browsing entities and audit history.</p>
					</div>
				{:else}
					<table data-testid="collections-table">
						<thead>
							<tr>
								<th>Collection</th>
								<th>Schema</th>
								<th>Created</th>
								<th>Updated</th>
								<th>Actions</th>
							</tr>
						</thead>
						<tbody>
							{#each collections as collection}
								<tr data-testid="collection-row">
									<td>
										<a href={collectionHref(collection.name)}>
											<strong>{collection.name}</strong>
										</a>
									</td>
									<td>
										{collection.schema_version ? `v${collection.schema_version}` : 'No schema'}
									</td>
									<td class="muted">{formatTimestamp(collection.created_at_ns)}</td>
									<td class="muted">{formatTimestamp(collection.updated_at_ns)}</td>
									<td>
										<div class="actions">
											<a class="button-link" href={collectionHref(collection.name)}>Browse</a>
											<a class="button-link" href={schemaHref(collection.name)}>Schema</a>
											{#if dropping === collection.name}
												<span class="muted" style="font-size:0.85rem">Drop {collection.name}?</span>
												<button
													class="danger"
													onclick={async () => {
														try {
															await dropCollection(collection.name, scope);
															dropping = null;
															await loadCollections();
														} catch (e: unknown) {
															error = e instanceof Error ? e.message : 'Failed to drop collection';
															dropping = null;
														}
													}}
												>
													Confirm
												</button>
												<button onclick={() => (dropping = null)}>Cancel</button>
											{:else}
												<button class="danger" onclick={() => (dropping = collection.name)}>
													Drop
												</button>
											{/if}
										</div>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{/if}
			</div>
		</section>
	</div>
{/if}

<style>
	.collections-workspace {
		display: grid;
		grid-template-columns: minmax(20rem, 0.8fr) minmax(34rem, 1.4fr);
		gap: 1rem;
		align-items: start;
	}

	.collection-create-panel {
		position: sticky;
		top: 1rem;
	}

	.collection-list-panel {
		min-width: 0;
	}

	.empty-collections h3 {
		margin: 0;
	}

	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}

	@media (max-width: 1100px) {
		.collections-workspace {
			grid-template-columns: 1fr;
		}

		.collection-create-panel {
			position: static;
		}
	}
</style>
