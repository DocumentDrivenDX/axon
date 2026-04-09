<script lang="ts">
import { type AuditEntry, fetchAudit } from '$lib/api';
import { onMount } from 'svelte';

type AuditFilters = {
	collection: string;
	actor: string;
	startDate: string;
	endDate: string;
};

let entries: AuditEntry[] = [];
let loading = true;
let error: string | null = null;
const filters = {
	collection: '',
	actor: '',
	startDate: '',
	endDate: '',
} satisfies AuditFilters;
let selectedEntry: AuditEntry | null = null;

function selectEntry(entry: AuditEntry) {
	selectedEntry = entry;
}

function dateToNs(date: string, isEndOfDay = false): string | undefined {
	if (!date) {
		return undefined;
	}

	const suffix = isEndOfDay ? 'T23:59:59.999Z' : 'T00:00:00.000Z';
	const milliseconds = Date.parse(`${date}${suffix}`);
	return Number.isFinite(milliseconds) ? `${BigInt(milliseconds) * 1_000_000n}` : undefined;
}

function formatTimestamp(timestampNs: number): string {
	return new Date(timestampNs / 1_000_000).toLocaleString();
}

async function loadEntries() {
	loading = true;
	try {
		const auditFilters: {
			collection?: string;
			actor?: string;
			sinceNs?: string;
			untilNs?: string;
		} = {};

		if (filters.collection) {
			auditFilters.collection = filters.collection;
		}
		if (filters.actor) {
			auditFilters.actor = filters.actor;
		}
		const sinceNs = dateToNs(filters.startDate);
		if (sinceNs) {
			auditFilters.sinceNs = sinceNs;
		}
		const untilNs = dateToNs(filters.endDate, true);
		if (untilNs) {
			auditFilters.untilNs = untilNs;
		}

		const response = await fetchAudit(auditFilters);
		entries = response.entries;
		selectedEntry = entries[0] ?? null;
		error = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load audit entries';
	} finally {
		loading = false;
	}
}

onMount(() => {
	void loadEntries();
});
</script>

<div class="page-header">
	<div>
		<h1>Audit Log</h1>
		<p class="muted">Filter recent changes by collection, actor, or time range.</p>
	</div>
</div>

<section class="panel">
	<div class="panel-body stack">
		<div class="two-column">
			<label>
				<span>Collection</span>
				<input bind:value={filters.collection} placeholder="All collections" />
			</label>
			<label>
				<span>Actor</span>
				<input bind:value={filters.actor} placeholder="All actors" />
			</label>
			<label>
				<span>Since</span>
				<input type="date" bind:value={filters.startDate} />
			</label>
			<label>
				<span>Until</span>
				<input type="date" bind:value={filters.endDate} />
			</label>
		</div>
		<div class="actions">
			<button class="primary" on:click={loadEntries}>Apply Filters</button>
		</div>
	</div>
</section>

{#if error}
	<p class="message error">{error}</p>
{/if}

<div class="two-column">
	<section class="panel">
		<div class="panel-header">
			<h2>Recent Entries</h2>
			<span class="pill">{entries.length} shown</span>
		</div>
		<div class="panel-body">
			{#if loading}
				<p class="message">Loading audit entries…</p>
			{:else if entries.length === 0}
				<p class="muted">No audit entries matched the current filters.</p>
			{:else}
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
						{#each entries as entry}
							<tr on:click={() => selectEntry(entry)}>
								<td>{entry.id}</td>
								<td>{formatTimestamp(entry.timestamp_ns)}</td>
								<td>{entry.collection}</td>
								<td>{entry.entity_id}</td>
								<td>{entry.mutation}</td>
								<td>{entry.actor ?? 'system'}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			{/if}
		</div>
	</section>

	<section class="panel">
		<div class="panel-header">
			<h2>{selectedEntry ? `Entry #${selectedEntry.id}` : 'Entry Detail'}</h2>
		</div>
		<div class="panel-body stack">
			{#if selectedEntry}
				<div>
					<strong>{selectedEntry.mutation}</strong>
					<p class="muted">
						{selectedEntry.collection}/{selectedEntry.entity_id} · {formatTimestamp(selectedEntry.timestamp_ns)}
					</p>
				</div>
				<div>
					<h3>Before</h3>
					<pre>{JSON.stringify(selectedEntry.data_before, null, 2) || 'null'}</pre>
				</div>
				<div>
					<h3>After</h3>
					<pre>{JSON.stringify(selectedEntry.data_after, null, 2) || 'null'}</pre>
				</div>
			{:else}
				<p class="muted">Select an entry to inspect its before/after payloads.</p>
			{/if}
		</div>
	</section>
</div>
