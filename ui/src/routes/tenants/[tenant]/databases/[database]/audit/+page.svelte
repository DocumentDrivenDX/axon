<script lang="ts">
import { base } from '$app/paths';
import { page } from '$app/state';
import {
	type AuditEntry,
	type EffectiveCollectionPolicy,
	fetchAudit,
	fetchEffectivePolicy,
	fetchIntentAudit,
	revertAuditEntry,
	rollbackTransaction,
} from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template for revert/tx-rollback denials.
import DenialMessage from '$lib/components/DenialMessage.svelte';
import { redactValue } from '$lib/redaction';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
// Cache is keyed by `${tenant}/${database}/${collection}` so navigating
// between tenants or databases never reuses another scope's redaction
// list. `$state` so reactive renderers see new entries as they arrive.
const policyByScopedCollection = $state<Record<string, EffectiveCollectionPolicy>>({});

function policyCacheKey(collection: string): string {
	return `${scope.tenant}/${scope.database}/${collection}`;
}

async function ensureEffectivePolicy(collection: string) {
	if (!scope) return;
	const key = policyCacheKey(collection);
	if (policyByScopedCollection[key] !== undefined) return;
	try {
		policyByScopedCollection[key] = await fetchEffectivePolicy(collection, scope);
	} catch {
		// Treat missing policy or fetch failure as "no redaction"; do not
		// block audit rendering on policy lookup. The server has already
		// enforced visibility at fetch time, so the worst case is that we
		// lose the explicit `[redacted]` marker.
		policyByScopedCollection[key] = {
			collection,
			canRead: true,
			canCreate: false,
			canUpdate: false,
			canDelete: false,
			redactedFields: [],
			deniedFields: [],
			policyVersion: 0,
		};
	}
}

function redactedFieldsFor(collection: string): readonly string[] {
	return policyByScopedCollection[policyCacheKey(collection)]?.redactedFields ?? [];
}

function safeJson(payload: unknown, collection: string): string {
	const redacted = redactValue(payload, redactedFieldsFor(collection));
	return JSON.stringify(redacted, null, 2) || 'null';
}

type AuditFilters = {
	collection: string;
	actor: string;
	startDate: string;
	endDate: string;
	intentId: string;
};

let entries = $state<AuditEntry[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
const filters = $state<AuditFilters>({
	collection: '',
	actor: '',
	startDate: '',
	endDate: '',
	intentId: page.url.searchParams.get('intent') ?? '',
});
let selectedEntry = $state<AuditEntry | null>(null);

// Revert state
let revertConfirming = $state(false);
let revertMessage = $state<string | null>(null);
let revertError = $state<unknown>(null);

// Transaction rollback state
let txRollbackConfirming = $state(false);
let txRollbackMessage = $state<string | null>(null);
let txRollbackError = $state<unknown>(null);

const basePath = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}`,
);

function intentDetailHref(intentId: string): string {
	return `${basePath}/intents/${encodeURIComponent(intentId)}`;
}

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
	if (entry.collection) {
		void ensureEffectivePolicy(entry.collection);
	}
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
		// Preserve structured AxonGraphqlError so DenialMessage can render
		// code/fieldPath/policy.
		txRollbackError = err instanceof Error ? err : String(err ?? 'Rollback failed');
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
		revertError = err instanceof Error ? err : String(err ?? 'Revert failed');
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

function auditEventLabel(entry: AuditEntry): string {
	if (
		entry.intent_lineage?.intent_id &&
		entry.collection !== '__mutation_intents' &&
		entry.mutation.startsWith('entity.')
	) {
		return 'intent.commit';
	}
	return entry.mutation;
}

async function loadEntries() {
	loading = true;
	try {
		if (filters.intentId && scope) {
			const response = await fetchIntentAudit(filters.intentId, scope);
			entries = response.entries;
		} else {
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
		}
		selectedEntry = entries[0] ?? null;
		// Mirror selectEntry()'s policy lookup so the auto-selected first
		// row's before/after payloads render with the correct redaction
		// list — otherwise the panel shows raw values until the user
		// manually clicks the row.
		if (selectedEntry?.collection) {
			void ensureEffectivePolicy(selectedEntry.collection);
		}
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

{#if filters.intentId}
	<div class="message" data-testid="audit-intent-banner">
		Filtered by intent <code>{filters.intentId}</code>
		<a class="inline-link" href={intentDetailHref(filters.intentId)} data-testid="audit-intent-detail-link">
			View intent detail
		</a>
		<button
			onclick={() => {
				filters.intentId = '';
				void loadEntries();
			}}
		>
			Clear filter
		</button>
	</div>
{/if}

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
			<label>
				<span>Intent ID</span>
				<input
					bind:value={filters.intentId}
					placeholder="Filter by intent ID"
					data-testid="audit-intent-filter"
				/>
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
		<div class="panel-body audit-table-wrap">
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
							<tr
								onclick={() => selectEntry(entry)}
								data-testid="audit-entry-row"
								data-intent-id={entry.intent_lineage?.intent_id ?? null}
							>
								<td>{entry.id}</td>
								<td>{formatTimestamp(entry.timestamp_ns)}</td>
								<td>{entry.collection}</td>
								<td>{entry.entity_id}</td>
								<td>
									{auditEventLabel(entry)}
									{#if entry.transaction_id}
										<span class="pill">tx #{String(entry.transaction_id).substring(0, 8)}</span>
									{/if}
									{#if entry.intent_lineage?.intent_id}
										<span class="pill" data-testid="audit-entry-intent-pill">intent</span>
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
					<strong>{auditEventLabel(selectedEntry)}</strong>
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
					<DenialMessage error={revertError} testid="audit-revert-error" />
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
							<DenialMessage error={txRollbackError} testid="audit-tx-rollback-error" />
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
				{#if selectedEntry.intent_lineage}
					{@const rl = redactValue(selectedEntry.intent_lineage, redactedFieldsFor(selectedEntry.collection))}
					<div data-testid="audit-intent-lineage">
						<h3>Intent Lineage</h3>
						<div class="meta-grid">
							<span>Intent ID</span>
							<a
								class="inline-link"
								href={intentDetailHref(selectedEntry.intent_lineage.intent_id)}
								data-testid="audit-intent-link"
							>
								{selectedEntry.intent_lineage.intent_id}
							</a>
							<span>Decision</span>
							<strong data-testid="audit-lineage-decision">{rl.decision}</strong>
							<span>Policy version</span>
							<strong data-testid="audit-lineage-policy-version">{rl.policy_version}</strong>
							<span>Schema version</span>
							<strong data-testid="audit-lineage-schema-version">{rl.schema_version}</strong>
							{#if rl.approver?.actor ?? rl.approver?.user_id}
								<span>Approver</span>
								<code data-testid="audit-lineage-approver">
									{rl.approver?.actor ?? rl.approver?.user_id}
								</code>
							{/if}
							{#if rl.reason}
								<span>Reason</span>
								<span data-testid="audit-lineage-reason">{rl.reason}</span>
							{/if}
							{#if rl.origin}
								<span>Origin</span>
								<code data-testid="audit-lineage-origin">
									{[rl.origin.surface, rl.origin.tool_name]
										.filter((v) => v && v.length > 0)
										.join(': ')}
								</code>
							{/if}
						</div>
					</div>
				{/if}
				<div>
					<h3>Before</h3>
					<pre>{safeJson(selectedEntry.data_before, selectedEntry.collection)}</pre>
				</div>
				<div>
					<h3>After</h3>
					<pre>{safeJson(selectedEntry.data_after, selectedEntry.collection)}</pre>
				</div>
			{:else}
				<p class="muted">Select an entry to inspect its before/after payloads.</p>
			{/if}
		</div>
	</section>
</div>

<style>
	.audit-table-wrap {
		overflow-x: auto;
	}
</style>
