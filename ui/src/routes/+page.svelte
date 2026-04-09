<script lang="ts">
/** US-040: Browse Axon data visually */
// Collection list page — fetches collections via GraphQL (US-051)
import { onMount } from 'svelte';

type CollectionSummary = {
	name: string;
	entityCount: number;
};

let collections: CollectionSummary[] = [];
let loading = true;
let error: string | null = null;

onMount(async () => {
	try {
		// Placeholder: in production, this calls the GraphQL endpoint
		collections = [{ name: 'Loading...', entityCount: 0 }];
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load collections';
	} finally {
		loading = false;
	}
});
</script>

<h1>Axon Collections</h1>

{#if loading}
	<p>Loading collections...</p>
{:else if error}
	<p class="error">Error: {error}</p>
{:else}
	<table>
		<thead>
			<tr>
				<th>Collection</th>
				<th>Entities</th>
				<th>Actions</th>
			</tr>
		</thead>
		<tbody>
			{#each collections as col}
				<tr>
					<td><a href="/collections/{col.name}">{col.name}</a></td>
					<td>{col.entityCount}</td>
					<td>
						<a href="/collections/{col.name}">Browse</a>
						<a href="/schemas?collection={col.name}">Schema</a>
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
{/if}

<style>
	table { width: 100%; border-collapse: collapse; }
	th, td { padding: 0.5rem; text-align: left; border-bottom: 1px solid #ddd; }
	.error { color: red; }
</style>
