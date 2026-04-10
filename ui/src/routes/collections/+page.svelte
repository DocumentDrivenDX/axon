<script lang="ts">
import { base } from '$app/paths';
import { type CollectionSummary, fetchCollections } from '$lib/api';

let collections = $state<CollectionSummary[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);

const schemasHref = `${base}/schemas`;

function collectionHref(name: string): string {
	return `${base}/collections/${encodeURIComponent(name)}`;
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
		collections = await fetchCollections();
		error = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load collections';
	} finally {
		loading = false;
	}
}

$effect(() => {
	void loadCollections();
});

const totalEntities = $derived(collections.reduce((sum, c) => sum + c.entity_count, 0));
</script>

<div class="page-header">
	<div>
		<h1>Collections</h1>
		<p class="muted">Browse registered collections with entity counts and schema versions.</p>
	</div>
	<div class="actions">
		<button onclick={loadCollections}>Refresh</button>
		<a class="button-link primary" href={schemasHref}>Create Collection</a>
	</div>
</div>

{#if loading}
	<p class="message">Loading collections...</p>
{:else if error}
	<p class="message error">{error}</p>
{:else if collections.length === 0}
	<section class="panel">
		<div class="panel-body stack">
			<h2>No collections yet</h2>
			<p class="muted">
				Create one from the schema workspace to start browsing entities and audit history.
			</p>
			<div>
				<a class="button-link primary" href={schemasHref}>Open schema workspace</a>
			</div>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Registered Collections</h2>
			<div class="actions">
				<span class="pill">{collections.length} collections</span>
				<span class="pill">{totalEntities} entities</span>
			</div>
		</div>
		<div class="panel-body">
			<table>
				<thead>
					<tr>
						<th>Collection</th>
						<th>Entities</th>
						<th>Schema</th>
						<th>Created</th>
						<th>Updated</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each collections as collection}
						<tr>
							<td>
								<a href={collectionHref(collection.name)}>
									<strong>{collection.name}</strong>
								</a>
							</td>
							<td>{collection.entity_count}</td>
							<td>
								{collection.schema_version ? `v${collection.schema_version}` : 'No schema'}
							</td>
							<td class="muted">{formatTimestamp(collection.created_at_ns)}</td>
							<td class="muted">{formatTimestamp(collection.updated_at_ns)}</td>
							<td>
								<div class="actions">
									<a class="button-link" href={collectionHref(collection.name)}>
										Browse
									</a>
									<a class="button-link" href={schemaHref(collection.name)}>
										Schema
									</a>
								</div>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
{/if}
