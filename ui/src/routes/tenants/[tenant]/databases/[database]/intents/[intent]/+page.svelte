<script lang="ts">
import { base } from '$app/paths';
import { page } from '$app/state';
import {
	type CommitMutationIntentOutcome,
	type MutationIntent,
	approveMutationIntent,
	commitMutationIntent,
	fetchMutationIntent,
	rejectMutationIntent,
} from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template as a component.
import JsonTree from '$lib/components/JsonTree.svelte';
// biome-ignore lint/correctness/noUnusedImports: Used in template for JsonTree data casts.
import type { JsonValue } from '$lib/components/json-tree-types';
import { onMount } from 'svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const intentId = $derived(page.params.intent ?? '');
const inboxHref = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}/intents`,
);

let intent = $state<MutationIntent | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let actionMessage = $state<string | null>(null);
let reviewReason = $state('');
let reviewing = $state(false);
let commitToken = $state('');
let committing = $state(false);
let commitOutcome = $state<CommitMutationIntentOutcome | null>(null);

const canApprove = $derived(
	intent?.approvalState === 'pending' &&
		(!intent.approvalRoute?.reasonRequired || reviewReason.trim().length > 0),
);
const canReject = $derived(intent?.approvalState === 'pending' && reviewReason.trim().length > 0);
const canCommit = $derived(
	(intent?.approvalState === 'approved' || intent?.approvalState === 'none') &&
		commitToken.trim().length > 0,
);

function formatNs(value: string | undefined): string {
	if (!value) return '-';
	const parsed = Number(value);
	if (!Number.isFinite(parsed)) return value;
	return new Date(Math.floor(parsed / 1_000_000)).toLocaleString();
}

async function loadIntent() {
	loading = true;
	error = null;
	actionMessage = null;
	try {
		intent = await fetchMutationIntent(scope, intentId);
		if (!intent) {
			error = `Intent ${intentId} was not found.`;
		}
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load mutation intent';
	} finally {
		loading = false;
	}
}

async function approveIntent() {
	if (!intent || !canApprove) return;
	reviewing = true;
	error = null;
	actionMessage = null;
	try {
		intent = await approveMutationIntent(scope, {
			intentId: intent.id,
			...(reviewReason.trim() ? { reason: reviewReason.trim() } : {}),
		});
		reviewReason = '';
		actionMessage = 'Intent approved.';
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to approve intent';
	} finally {
		reviewing = false;
	}
}

async function rejectIntent() {
	if (!intent || !canReject) return;
	reviewing = true;
	error = null;
	actionMessage = null;
	try {
		intent = await rejectMutationIntent(scope, {
			intentId: intent.id,
			reason: reviewReason.trim(),
		});
		reviewReason = '';
		actionMessage = 'Intent rejected.';
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to reject intent';
	} finally {
		reviewing = false;
	}
}

async function commitIntent() {
	if (!intent || !canCommit) return;
	committing = true;
	error = null;
	actionMessage = null;
	commitOutcome = null;
	try {
		const outcome = await commitMutationIntent(scope, {
			intentToken: commitToken.trim(),
			intentId: intent.id,
		});
		commitOutcome = outcome;
		if (outcome.ok) {
			intent = outcome.result.intent ?? intent;
			actionMessage = outcome.result.committed ? 'Intent committed.' : 'Commit did not apply.';
			commitToken = '';
		}
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to commit intent';
	} finally {
		committing = false;
	}
}

onMount(() => {
	void loadIntent();
});
</script>

<div class="page-header">
	<div>
		<a class="back-link" href={inboxHref}>Back to intents</a>
		<h1>Intent Detail</h1>
		<p class="muted">{intentId}</p>
	</div>
</div>

{#if loading}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">Loading intent...</p>
		</div>
	</section>
{:else if error && !intent}
	<section class="panel">
		<div class="panel-body">
			<p class="message error">{error}</p>
		</div>
	</section>
{:else if intent}
	<div class="detail-grid" data-testid="intent-detail">
		<section class="panel">
			<div class="panel-header">
				<div>
					<h2>{intent.reviewSummary.title ?? intent.operation.operationKind}</h2>
					<p class="muted">{intent.reviewSummary.summary ?? intent.id}</p>
				</div>
				<div class="status-stack">
					<span class="pill">{intent.decision}</span>
					<span class="pill state">{intent.approvalState}</span>
				</div>
			</div>
			<div class="panel-body stack">
				{#if error}
					<p class="message error">{error}</p>
				{/if}
				{#if actionMessage}
					<p class="message success">{actionMessage}</p>
				{/if}
				{#if commitOutcome && !commitOutcome.ok}
					<div class="message error" data-testid="intent-commit-error">
						<strong>{commitOutcome.error.code ?? 'intent_error'}</strong>
						<p>{commitOutcome.error.message}</p>
					</div>
				{/if}

				<div class="meta-grid">
					<span>Intent ID</span>
					<code>{intent.id}</code>
					<span>Operation</span>
					<code>{intent.operation.operationKind}</code>
					<span>Operation hash</span>
					<code>{intent.operationHash}</code>
					<span>Schema version</span>
					<strong>{intent.schemaVersion}</strong>
					<span>Policy version</span>
					<strong>{intent.policyVersion}</strong>
					<span>Expires</span>
					<strong>{formatNs(intent.expiresAtNs)}</strong>
					<span>Approval role</span>
					<strong>{intent.approvalRoute?.role ?? '-'}</strong>
				</div>
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Review</h2>
			</div>
			<div class="panel-body stack">
				{#if (intent.reviewSummary.policy_explanation ?? []).length > 0}
					<ul class="policy-list" data-testid="intent-policy-lines">
						{#each intent.reviewSummary.policy_explanation ?? [] as line}
							<li>{line}</li>
						{/each}
					</ul>
				{/if}

				<div class="table-wrap">
					<table>
						<thead>
							<tr>
								<th>Pre-image</th>
								<th>Version</th>
							</tr>
						</thead>
						<tbody>
							{#each intent.preImages as record}
								<tr>
									<td><code>{record.collection}/{record.id ?? '-'}</code></td>
									<td>v{record.version ?? '-'}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>

				<div>
					<h3>Summary</h3>
					<JsonTree data={intent.reviewSummary as JsonValue} />
				</div>
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Operation</h2>
			</div>
			<div class="panel-body">
				<JsonTree data={intent.operation.operation as JsonValue} />
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Actions</h2>
			</div>
			<div class="panel-body stack">
				{#if intent.approvalState === 'pending'}
					<label>
						<span>Reason</span>
						<textarea bind:value={reviewReason} data-testid="intent-reason"></textarea>
					</label>
					<div class="actions">
						<button
							type="button"
							class="primary"
							disabled={!canApprove || reviewing}
							onclick={() => void approveIntent()}
							data-testid="intent-approve"
						>
							Approve
						</button>
						<button
							type="button"
							class="danger"
							disabled={!canReject || reviewing}
							onclick={() => void rejectIntent()}
							data-testid="intent-reject"
						>
							Reject
						</button>
					</div>
				{/if}

				{#if intent.approvalState === 'approved' || intent.approvalState === 'none'}
					<label>
						<span>Intent token</span>
						<input bind:value={commitToken} type="password" data-testid="intent-commit-token" />
					</label>
					<div class="actions">
						<button
							type="button"
							class="primary"
							disabled={!canCommit || committing}
							onclick={() => void commitIntent()}
							data-testid="intent-commit-action"
						>
							{committing ? 'Committing...' : 'Commit'}
						</button>
					</div>
				{/if}

				{#if intent.approvalState !== 'pending' && intent.approvalState !== 'approved' && intent.approvalState !== 'none'}
					<p class="muted">No review action is available for {intent.approvalState} intents.</p>
				{/if}
			</div>
		</section>
	</div>
{/if}

<style>
	.back-link {
		display: inline-flex;
		margin-bottom: 0.5rem;
		color: var(--muted);
		text-decoration: none;
	}

	.back-link:hover {
		color: var(--accent);
	}

	.detail-grid {
		display: grid;
		grid-template-columns: minmax(0, 1.2fr) minmax(20rem, 0.8fr);
		gap: 1rem;
	}

	.status-stack {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.pill.state {
		border-color: rgba(148, 163, 184, 0.35);
		color: var(--muted);
	}

	.meta-grid {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		gap: 0.6rem 1rem;
	}

	.meta-grid > span,
	label span {
		color: var(--muted);
	}

	h2,
	h3,
	p {
		margin: 0;
	}

	h3 {
		margin-bottom: 0.75rem;
		color: var(--muted);
		font-size: 0.9rem;
		text-transform: uppercase;
	}

	.policy-list {
		margin: 0;
		padding-left: 1.2rem;
	}

	.table-wrap {
		overflow-x: auto;
	}

	@media (max-width: 1000px) {
		.detail-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
