<script lang="ts">
import { base } from '$app/paths';
import {
	type MutationIntent,
	type MutationIntentApprovalState,
	type MutationIntentPageInfo,
	fetchMutationIntents,
} from '$lib/api';
import { onMount } from 'svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const dbHref = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}`,
);

const statuses: MutationIntentApprovalState[] = [
	'pending',
	'approved',
	'rejected',
	'expired',
	'committed',
];

let activeStatus = $state<MutationIntentApprovalState>('pending');
let intents = $state<MutationIntent[]>([]);
let pageInfo = $state<MutationIntentPageInfo | null>(null);
let totalCount = $state(0);
let loading = $state(true);
let error = $state<string | null>(null);

function formatNs(value: string | undefined): string {
	if (!value) return '-';
	const parsed = Number(value);
	if (!Number.isFinite(parsed)) return value;
	return new Date(Math.floor(parsed / 1_000_000)).toLocaleString();
}

function intentHref(intent: MutationIntent): string {
	return `${dbHref}/intents/${encodeURIComponent(intent.id)}`;
}

function intentTitle(intent: MutationIntent): string {
	return (
		intent.reviewSummary.title ?? intent.reviewSummary.summary ?? intent.operation.operationKind
	);
}

function recordLabel(intent: MutationIntent): string {
	const record = intent.preImages[0];
	if (!record) return '-';
	return `${record.collection}/${record.id ?? '-'}`;
}

function fieldSummary(intent: MutationIntent): string {
	return (intent.reviewSummary.affected_fields ?? []).join(', ') || '-';
}

async function loadIntents(
	status: MutationIntentApprovalState = activeStatus,
	after: string | null = null,
) {
	loading = true;
	error = null;
	try {
		const result = await fetchMutationIntents(scope, {
			filter: { status },
			limit: 25,
			after,
		});
		intents = result.edges.map((edge) => edge.node);
		pageInfo = result.pageInfo;
		totalCount = result.totalCount;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load mutation intents';
	} finally {
		loading = false;
	}
}

async function selectStatus(status: MutationIntentApprovalState) {
	activeStatus = status;
	await loadIntents(status, null);
}

onMount(() => {
	void loadIntents();
});
</script>

<div class="page-header">
	<div>
		<h1>Mutation Intents</h1>
		<p class="muted">Review policy-routed mutation intents in {data.database.name}.</p>
	</div>
</div>

<section class="panel">
	<div class="panel-header">
		<div class="status-tabs" role="tablist" aria-label="Intent status">
			{#each statuses as status}
				<button
					type="button"
					class:active={activeStatus === status}
					role="tab"
					aria-selected={activeStatus === status}
					onclick={() => void selectStatus(status)}
				>
					{status}
				</button>
			{/each}
		</div>
		<span class="muted">{totalCount} total</span>
	</div>
	<div class="panel-body">
		{#if error}
			<p class="message error">{error}</p>
		{:else if loading}
			<p class="muted">Loading intents...</p>
		{:else if intents.length === 0}
			<p class="muted">No {activeStatus} intents.</p>
		{:else}
			<div class="table-wrap">
				<table data-testid="intent-inbox-table">
					<thead>
						<tr>
							<th>Intent</th>
							<th>Decision</th>
							<th>Operation</th>
							<th>Record</th>
							<th>Fields</th>
							<th>Expires</th>
						</tr>
					</thead>
					<tbody>
						{#each intents as intent}
							<tr data-testid={`intent-row-${intent.id}`}>
								<td>
									<a class="intent-link" href={intentHref(intent)}>
										<code>{intent.id}</code>
										<span>{intentTitle(intent)}</span>
									</a>
								</td>
								<td>
									<span class="pill">{intent.decision}</span>
									<span class="state">{intent.approvalState}</span>
								</td>
								<td><code>{intent.operation.operationKind}</code></td>
								<td><code>{recordLabel(intent)}</code></td>
								<td>{fieldSummary(intent)}</td>
								<td>{formatNs(intent.expiresAtNs)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
			<div class="actions pager">
				<button
					type="button"
					disabled={!pageInfo?.hasNextPage}
					onclick={() => void loadIntents(activeStatus, pageInfo?.endCursor ?? null)}
				>
					Next
				</button>
			</div>
		{/if}
	</div>
</section>

<style>
	.status-tabs {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.status-tabs button {
		border-radius: 0.55rem;
		padding: 0.45rem 0.75rem;
	}

	.status-tabs button.active {
		border-color: rgba(125, 211, 252, 0.6);
		background: rgba(125, 211, 252, 0.14);
		color: var(--text);
	}

	.table-wrap {
		overflow-x: auto;
	}

	.intent-link {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		text-decoration: none;
	}

	.intent-link:hover span {
		color: var(--accent);
	}

	.state {
		display: block;
		margin-top: 0.4rem;
		color: var(--muted);
		font-size: 0.85rem;
	}

	.pager {
		justify-content: flex-end;
		margin-top: 1rem;
	}
</style>
