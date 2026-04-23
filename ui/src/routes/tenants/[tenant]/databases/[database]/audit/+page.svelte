<script lang="ts">
import { type AuditEntry, fetchAudit, revertAuditEntry, rollbackTransaction } from '$lib/api';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);

type AuditFilters = {
	collection: string;
	actor: string;
	startDate: string;
	endDate: string;
};

let entries = $state<AuditEntry[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
const filters = $state<AuditFilters>({
	collection: '',
	actor: '',
	startDate: '',
	endDate: '',
});
let selectedEntry = $state<AuditEntry | null>(null);

// Revert state
let revertConfirming = $state(false);
let revertMessage = $state<string | null>(null);
let revertError = $state<string | null>(null);

// Transaction rollback state
let txRollbackConfirming = $state(false);
let txRollbackMessage = $state<string | null>(null);
let txRollbackError = $state<string | null>(null);

function txSiblingCount(txId: string | number | null | undefined): number {
	if (!txId) return 0;
	return entries.filter((e) => e.transaction_id === txId).length;
}

function selectEntry(entry: AuditEntry) {
	selectedEntry = entry;
	revertConfirming = false;
	revertMessage = null;
	revertError = null;
	txRollbackConfirming = false;
	txRollbackMessage = null;
	txRollbackError = null;
}

async function doTxRollback() {
	if (!selectedEntry?.transaction_id || !scope) return;
	try {
		const result = await rollbackTransaction(String(selectedEntry.transaction_id), false, scope);
		txRollbackMessage = `Transaction rolled back: ${result.entities_rolled_back} entity/entities restored.`;
		txRollbackConfirming = false;
		txRollbackError = null;
		await loadEntries();
	} catch (err: unknown) {
		txRollbackError = err instanceof Error ? err.message : 'Rollback failed';
		txRollbackConfirming = false;
	}
}

async function doRevert() {
	if (!selectedEntry || !scope) return;
	try {
		await revertAuditEntry(selectedEntry.id, scope);
		revertMessage = `Entry #${selectedEntry.id} reverted successfully.`;
		revertConfirming = false;
		revertError = null;
		await loadEntries();
	} catch (err: unknown) {
		revertError = err instanceof Error ? err.message : 'Revert failed';
		revertConfirming = false;
	}
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

		const response = await fetchAudit(auditFilters, scope);
		entries = response.entries;
		selectedEntry = entries[0] ?? null;
		error = null;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load audit entries';
	} finally {
		loading = false;
	}
}

$effect(() => {
	// Re-run when scope changes.
	void scope;
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
			<button class="primary" onclick={() => loadEntries()}>Apply Filters</button>
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
							<tr onclick={() => selectEntry(entry)}>
								<td>{entry.id}</td>
								<td>{formatTimestamp(entry.timestamp_ns)}</td>
								<td>{entry.collection}</td>
								<td>{entry.entity_id}</td>
								<td>
									{entry.mutation}
									{#if entry.transaction_id}
										<span class="pill">tx #{String(entry.transaction_id).substring(0, 8)}</span>
									{/if}
								</td>
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
						{selectedEntry.collection}/{selectedEntry.entity_id} · {formatTimestamp(
							selectedEntry.timestamp_ns,
						)}
					</p>
				</div>
				{#if revertMessage}
					<p class="message success">{revertMessage}</p>
				{/if}
				{#if revertError}
					<p class="message error">{revertError}</p>
				{/if}
				{#if selectedEntry.data_before !== null}
					{#if revertConfirming}
						<div class="actions">
							<span>Revert entry #{selectedEntry.id}?</span>
							<button class="danger" onclick={() => doRevert()}>Yes</button>
							<button onclick={() => (revertConfirming = false)}>No</button>
						</div>
					{:else}
						<div class="actions">
							<button onclick={() => { revertConfirming = true; revertMessage = null; revertError = null; }}>
								Revert this change
							</button>
						</div>
					{/if}
				{/if}
				{#if selectedEntry.transaction_id}
					<div>
						<h3>Transaction</h3>
						<p class="muted">
							Transaction ID: {selectedEntry.transaction_id}<br />
							{txSiblingCount(selectedEntry.transaction_id)} audit entries share this transaction.
						</p>
						{#if txRollbackMessage}
							<p class="message success">{txRollbackMessage}</p>
						{/if}
						{#if txRollbackError}
							<p class="message error">{txRollbackError}</p>
						{/if}
						{#if txRollbackConfirming}
							<div class="actions">
								<span>
									Undo all {txSiblingCount(selectedEntry.transaction_id)} mutations in transaction {selectedEntry.transaction_id}?
								</span>
								<button class="danger" onclick={() => doTxRollback()}>Yes</button>
								<button onclick={() => (txRollbackConfirming = false)}>No</button>
							</div>
						{:else}
							<div class="actions">
								<button onclick={() => { txRollbackConfirming = true; txRollbackMessage = null; txRollbackError = null; }}>
									Rollback this transaction
								</button>
							</div>
						{/if}
					</div>
				{/if}
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
