<script lang="ts">
import { base } from '$app/paths';
import { type CollectionSummary, fetchCollections } from '$lib/api';
import { onMount } from 'svelte';

let collections: CollectionSummary[] = [];
let loading = true;
let error: string | null = null;
const schemasHref = `${base}/schemas`;

function collectionHref(name: string): string {
	return `${base}/collections/${encodeURIComponent(name)}`;
}

function schemaHref(name: string): string {
	return `${schemasHref}?collection=${encodeURIComponent(name)}`;
}

async function loadCollections() {
	try {
		collections = await fetchCollections();
		error = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load collections';
	} finally {
		loading = false;
	}
}

onMount(() => {
	void loadCollections();
});
</script>

<div class="page-header">
	<div>
		<h1>Collections</h1>
		<p class="muted">Live collection metadata from the HTTP gateway.</p>
	</div>
	<div class="actions">
		<button on:click={loadCollections}>Refresh</button>
		<a class="button-link primary" href={schemasHref}>Create Collection</a>
	</div>
</div>

{#if loading}
	<p class="message">Loading collections…</p>
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
			<span class="pill">{collections.length} loaded</span>
		</div>
		<div class="panel-body">
			<table>
				<thead>
					<tr>
						<th>Collection</th>
						<th>Entities</th>
						<th>Schema</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each collections as collection}
						<tr>
							<td>
								<strong>{collection.name}</strong>
							</td>
							<td>{collection.entity_count}</td>
							<td>{collection.schema_version ? `v${collection.schema_version}` : 'No schema'}</td>
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
