<script>
	/** US-042: Inspect audit log visually */
	// Audit table with filter by collection/actor, date range, before/after detail
	let entries = [];
	let filters = {
		collection: '',
		actor: '',
		startDate: '',
		endDate: ''
	};
	let selectedEntry = null;
</script>

<h1>Audit Log</h1>

<div class="filters">
	<label>
		Collection:
		<input bind:value={filters.collection} placeholder="All collections" />
	</label>
	<label>
		Actor:
		<input bind:value={filters.actor} placeholder="All actors" />
	</label>
	<label>
		From:
		<input type="date" bind:value={filters.startDate} />
	</label>
	<label>
		To:
		<input type="date" bind:value={filters.endDate} />
	</label>
	<button>Apply Filters</button>
</div>

<table>
	<thead>
		<tr>
			<th>ID</th>
			<th>Timestamp</th>
			<th>Collection</th>
			<th>Entity</th>
			<th>Operation</th>
			<th>Actor</th>
		</tr>
	</thead>
	<tbody>
		{#if entries.length === 0}
			<tr><td colspan="6">No audit entries found. Perform operations to generate entries.</td></tr>
		{/if}
		{#each entries as entry}
			<tr on:click={() => selectedEntry = entry}>
				<td>{entry.id}</td>
				<td>{new Date(entry.timestamp_ns / 1e6).toISOString()}</td>
				<td>{entry.collection}</td>
				<td>{entry.entity_id}</td>
				<td>{entry.mutation}</td>
				<td>{entry.actor}</td>
			</tr>
		{/each}
	</tbody>
</table>

{#if selectedEntry}
	<div class="detail">
		<h2>Audit Entry #{selectedEntry.id}</h2>
		<h3>Before</h3>
		<pre>{JSON.stringify(selectedEntry.data_before, null, 2) || 'null'}</pre>
		<h3>After</h3>
		<pre>{JSON.stringify(selectedEntry.data_after, null, 2) || 'null'}</pre>
	</div>
{/if}

<style>
	.filters { display: flex; gap: 0.5rem; flex-wrap: wrap; margin-bottom: 1rem; }
	.filters label { display: flex; flex-direction: column; font-size: 0.9em; }
	table { width: 100%; border-collapse: collapse; }
	th, td { padding: 0.5rem; text-align: left; border-bottom: 1px solid #ddd; }
	tbody tr { cursor: pointer; }
	tbody tr:hover { background: #f0f0f0; }
	.detail { margin-top: 1rem; }
	pre { background: #f5f5f5; padding: 1rem; overflow-x: auto; }
</style>
