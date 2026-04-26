<script lang="ts">
import { base } from '$app/paths';
import { page } from '$app/state';
import {
	type AuditEntry,
	type CommitMutationIntentOutcome,
	type MutationIntent,
	approveMutationIntent,
	commitMutationIntent,
	fetchIntentAudit,
	fetchMutationIntent,
	rejectMutationIntent,
} from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template for action denials.
import DenialMessage from '$lib/components/DenialMessage.svelte';
// biome-ignore lint/correctness/noUnusedImports: Used in template as a component.
import JsonTree from '$lib/components/JsonTree.svelte';
import type { JsonValue } from '$lib/components/json-tree-types';
import {
	agentIdentityLabel,
	credentialLabel,
	delegatedByLabel,
	grantVersionLabel,
	requesterLabel as intentRequesterLabel,
	originBadge,
	originMetadataSummary,
	structuredOutcomeSummary,
	tenantRoleLabel,
	toolArgumentsSummary,
	toolNameLabel,
} from '$lib/intent-metadata';
import { onMount } from 'svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const intentId = $derived(page.params.intent ?? '');
const basePath = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}`,
);
const inboxHref = $derived(`${basePath}/intents`);
const auditHref = $derived(`${basePath}/audit`);

let intent = $state<MutationIntent | null>(null);
let auditEntries = $state<AuditEntry[]>([]);
let auditLoading = $state(false);
let auditError = $state<string | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let actionMessage = $state<string | null>(null);
let actionError = $state<unknown>(null);
let reviewReason = $state('');
let reviewReasonError = $state<string | null>(null);
let reviewing = $state(false);
let commitToken = $state('');
let committing = $state(false);
let commitOutcome = $state<CommitMutationIntentOutcome | null>(null);
// biome-ignore lint/style/useConst: Svelte bind:this assigns the element ref at runtime.
let reviewReasonField = $state<HTMLTextAreaElement | null>(null);

const canApprove = $derived(intent?.approvalState === 'pending' && !reviewing);
const canReject = $derived(intent?.approvalState === 'pending' && !reviewing);
const staleCommitCode = $derived(
	commitOutcome && !commitOutcome.ok ? (commitOutcome.error.code ?? null) : null,
);
const commitBlockedByStale = $derived(
	staleCommitCode === 'intent_stale' || staleCommitCode === 'intent_mismatch',
);
const canCommit = $derived(
	(intent?.approvalState === 'approved' || intent?.approvalState === 'none') &&
		!commitBlockedByStale &&
		commitToken.trim().length > 0,
);

function formatNs(value: string | number | undefined | null): string {
	if (value === undefined || value === null) return '-';
	const parsed = Number(value);
	if (!Number.isFinite(parsed)) return String(value);
	return new Date(Math.floor(parsed / 1_000_000)).toLocaleString();
}

function asRecord(value: unknown): Record<string, unknown> | null {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	return value as Record<string, unknown>;
}

function stringMember(value: Record<string, unknown> | null, ...keys: string[]): string | null {
	if (!value) return null;
	for (const key of keys) {
		const candidate = value[key];
		if (typeof candidate === 'string' && candidate.trim().length > 0) {
			return candidate;
		}
	}
	return null;
}

function subjectRecord(currentIntent: MutationIntent): Record<string, unknown> | null {
	return asRecord(currentIntent.subject);
}

function subjectField(currentIntent: MutationIntent, ...keys: string[]): string {
	if (keys.includes('user_id') || keys.includes('userId')) {
		return intentRequesterLabel(currentIntent);
	}
	if (keys.includes('agent_id') || keys.includes('agentId')) {
		return agentIdentityLabel(currentIntent);
	}
	if (keys.includes('delegated_by') || keys.includes('delegatedBy')) {
		return delegatedByLabel(currentIntent);
	}
	if (keys.includes('tenant_role') || keys.includes('tenantRole')) {
		return tenantRoleLabel(currentIntent);
	}
	if (keys.includes('credential_id') || keys.includes('credentialId')) {
		return credentialLabel(currentIntent);
	}
	return stringMember(subjectRecord(currentIntent), ...keys) ?? '-';
}

function grantVersion(currentIntent: MutationIntent): string {
	return grantVersionLabel(currentIntent);
}

function toolName(currentIntent: MutationIntent): string {
	return toolNameLabel(currentIntent, auditEntries);
}

function originKind(currentIntent: MutationIntent): string {
	return originBadge(currentIntent);
}

function credentialFor(currentIntent: MutationIntent): string {
	return credentialLabel(currentIntent);
}

function delegatedAuthority(currentIntent: MutationIntent): string {
	const delegatedBy = delegatedByLabel(currentIntent);
	const tenantRole = tenantRoleLabel(currentIntent);
	if (delegatedBy === '-' && tenantRole === '-') return '-';
	const parts = [
		delegatedBy !== '-' ? `delegated by ${delegatedBy}` : null,
		tenantRole !== '-' ? `role ${tenantRole}` : null,
	].filter((value): value is string => Boolean(value));
	return parts.join(' · ');
}

function originMetadata(currentIntent: MutationIntent): JsonValue {
	return originMetadataSummary(currentIntent, auditEntries) as JsonValue;
}

function toolArgumentSummary(currentIntent: MutationIntent): JsonValue {
	return toolArgumentsSummary(currentIntent.operation) as JsonValue;
}

function structuredOutcome(currentIntent: MutationIntent): JsonValue {
	return structuredOutcomeSummary(currentIntent, { auditEntries, commitOutcome }) as JsonValue;
}

function intentTitle(currentIntent: MutationIntent): string {
	return (
		currentIntent.reviewSummary.title ??
		currentIntent.reviewSummary.summary ??
		currentIntent.operation.operationKind
	);
}

function recordHref(collection: string): string {
	return `${basePath}/collections/${encodeURIComponent(collection)}`;
}

function relatedCollections(currentIntent: MutationIntent): string[] {
	return Array.from(new Set(currentIntent.preImages.map((record) => record.collection))).sort();
}

function affectedFields(currentIntent: MutationIntent): string[] {
	return currentIntent.reviewSummary.affected_fields ?? [];
}

function executionSummary(currentIntent: MutationIntent): string {
	if (commitBlockedByStale) {
		return `Commit blocked by ${staleCommitCode}. Re-preview is required before commit.`;
	}
	switch (currentIntent.approvalState) {
		case 'pending':
			return 'Waiting for an approver in the configured review route.';
		case 'approved':
			return 'Approved. Commit still requires the original intent token.';
		case 'none':
			return 'Allowed. Commit requires the preview token captured at preview time.';
		case 'rejected':
			return 'Rejected. This intent can no longer be committed.';
		case 'expired':
			return 'Expired. Preview bindings are no longer executable.';
		case 'committed':
			return 'Committed. This intent has already been consumed.';
		default:
			return 'Execution state unavailable.';
	}
}

function auditReason(entry: AuditEntry): string {
	return entry.intent_lineage?.reason ?? entry.metadata?.reason ?? '-';
}

function auditApprover(entry: AuditEntry): string {
	return (
		entry.intent_lineage?.approver?.actor ??
		entry.intent_lineage?.approver?.user_id ??
		entry.actor ??
		'system'
	);
}

function auditOrigin(entry: AuditEntry): string {
	const origin = entry.intent_lineage?.origin;
	if (!origin) return '-';
	return [origin.surface, origin.tool_name].filter((value) => value && value.length > 0).join(': ');
}

function hasReviewDiff(currentIntent: MutationIntent): boolean {
	return (
		currentIntent.reviewSummary.diff !== undefined && currentIntent.reviewSummary.diff !== null
	);
}

function reviewStatus(currentIntent: MutationIntent): string {
	switch (currentIntent.approvalState) {
		case 'pending':
			return currentIntent.approvalRoute?.reasonRequired
				? 'Approval on this route requires a reason. Rejection also requires a reason.'
				: 'Rejection requires a reason.';
		case 'none':
			return 'This intent does not require approval.';
		case 'approved':
			return 'Review is complete. Approve and reject are disabled.';
		case 'rejected':
			return 'Rejected intents cannot be reviewed again.';
		case 'expired':
			return 'Expired intents cannot be approved or rejected.';
		case 'committed':
			return 'Committed intents are already consumed.';
		default:
			return 'Review status unavailable.';
	}
}

function commitStatus(currentIntent: MutationIntent): string {
	if (commitBlockedByStale) {
		return `Commit is disabled because the latest validation returned ${staleCommitCode}.`;
	}
	switch (currentIntent.approvalState) {
		case 'pending':
			return 'Commit is unavailable until an approver accepts the intent.';
		case 'approved':
		case 'none':
			return 'Enter the preview token to commit this intent.';
		case 'rejected':
			return 'Rejected intents cannot be committed.';
		case 'expired':
			return 'Expired intents cannot be committed.';
		case 'committed':
			return 'Committed intents cannot be committed again.';
		default:
			return 'Commit status unavailable.';
	}
}

function validateReviewReason(action: 'approve' | 'reject'): boolean {
	if (!intent || intent.approvalState !== 'pending') return false;
	const requiresReason = action === 'reject' || Boolean(intent.approvalRoute?.reasonRequired);
	if (!requiresReason || reviewReason.trim().length > 0) {
		reviewReasonError = null;
		return true;
	}
	reviewReasonError =
		action === 'approve'
			? 'Approval reason is required by the current approval route.'
			: 'Rejection reason is required.';
	reviewReasonField?.focus();
	return false;
}

async function loadAuditTrail(targetIntentId: string) {
	auditLoading = true;
	auditError = null;
	try {
		const result = await fetchIntentAudit(targetIntentId, scope);
		auditEntries = result.entries;
	} catch (errorValue: unknown) {
		auditError = errorValue instanceof Error ? errorValue.message : 'Failed to load intent audit';
		auditEntries = [];
	} finally {
		auditLoading = false;
	}
}

async function loadIntent() {
	loading = true;
	error = null;
	actionError = null;
	actionMessage = null;
	reviewReasonError = null;
	auditEntries = [];
	auditError = null;
	try {
		intent = await fetchMutationIntent(scope, intentId);
		if (!intent) {
			error = `Intent ${intentId} was not found.`;
			return;
		}
		await loadAuditTrail(intent.id);
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load mutation intent';
	} finally {
		loading = false;
	}
}

async function approveIntent() {
	if (!intent || !canApprove || !validateReviewReason('approve')) return;
	reviewing = true;
	actionError = null;
	actionMessage = null;
	try {
		intent = await approveMutationIntent(scope, {
			intentId: intent.id,
			...(reviewReason.trim() ? { reason: reviewReason.trim() } : {}),
		});
		reviewReason = '';
		reviewReasonError = null;
		actionMessage = 'Intent approved.';
		await loadAuditTrail(intent.id);
	} catch (errorValue: unknown) {
		actionError =
			errorValue instanceof Error ? errorValue : String(errorValue ?? 'Failed to approve intent');
	} finally {
		reviewing = false;
	}
}

async function rejectIntent() {
	if (!intent || !canReject || !validateReviewReason('reject')) return;
	reviewing = true;
	actionError = null;
	actionMessage = null;
	try {
		intent = await rejectMutationIntent(scope, {
			intentId: intent.id,
			reason: reviewReason.trim(),
		});
		reviewReason = '';
		reviewReasonError = null;
		actionMessage = 'Intent rejected.';
		await loadAuditTrail(intent.id);
	} catch (errorValue: unknown) {
		actionError =
			errorValue instanceof Error ? errorValue : String(errorValue ?? 'Failed to reject intent');
	} finally {
		reviewing = false;
	}
}

async function commitIntent() {
	if (!intent || !canCommit) return;
	committing = true;
	actionError = null;
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
			await loadAuditTrail(intent.id);
		}
	} catch (errorValue: unknown) {
		actionError =
			errorValue instanceof Error ? errorValue : String(errorValue ?? 'Failed to commit intent');
	} finally {
		committing = false;
	}
}

$effect(() => {
	if (reviewReason.trim().length > 0 && reviewReasonError) {
		reviewReasonError = null;
	}
});

$effect(() => {
	if (intent?.approvalState !== 'pending' && reviewReason.length > 0) {
		reviewReason = '';
		reviewReasonError = null;
	}
});

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
					<h2>{intentTitle(intent)}</h2>
					<p class="muted">{intent.reviewSummary.summary ?? intent.id}</p>
				</div>
				<div class="status-stack">
					<span class="pill">{intent.decision}</span>
					<span class="pill state">{intent.approvalState}</span>
					<span class="pill state">{originKind(intent)}</span>
				</div>
			</div>
			<div class="panel-body stack" data-testid="intent-overview">
				{#if actionError}
					<DenialMessage error={actionError} testid="intent-action-error" />
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

				<div class="execution-callout">
					<strong>{executionSummary(intent)}</strong>
					<p class="muted">Operation hash {intent.operationHash}</p>
				</div>

				<div class="meta-grid">
					<span>Intent ID</span>
					<code>{intent.id}</code>
					<span>Operation</span>
					<code>{intent.operation.operationKind}</code>
					<span>Operation hash</span>
					<code>{intent.operationHash}</code>
					<span>Expires</span>
					<strong>{formatNs(intent.expiresAtNs)}</strong>
					<span>Schema version</span>
					<strong>{intent.schemaVersion}</strong>
					<span>Policy version</span>
					<strong>{intent.policyVersion}</strong>
					<span>Grant version</span>
					<strong>{grantVersion(intent)}</strong>
				</div>

				<div class="link-list" data-testid="intent-deep-links">
					{#each relatedCollections(intent) as collection}
						<a class="inline-link" href={recordHref(collection)}>Open {collection}</a>
					{/each}
					<a class="inline-link" href={auditHref}>Open audit log</a>
				</div>
			</div>
		</section>

		<section class="panel" data-testid="intent-bindings">
			<div class="panel-header">
				<h2>Bindings</h2>
			</div>
			<div class="panel-body stack">
				<div class="meta-grid">
					<span>Requester</span>
					<code>{subjectField(intent, 'user_id', 'userId')}</code>
					<span>Agent / tool</span>
					<code>{subjectField(intent, 'agent_id', 'agentId')}</code>
					<span>Delegated by</span>
					<code>{subjectField(intent, 'delegated_by', 'delegatedBy')}</code>
					<span>Tenant role</span>
					<code>{subjectField(intent, 'tenant_role', 'tenantRole')}</code>
					<span>Credential</span>
					<code>{subjectField(intent, 'credential_id', 'credentialId')}</code>
					<span>Grant version</span>
					<strong>{grantVersion(intent)}</strong>
					<span>Approval role</span>
					<code>{intent.approvalRoute?.role ?? '-'}</code>
					<span>Reason required</span>
					<strong>{intent.approvalRoute?.reasonRequired ? 'yes' : 'no'}</strong>
					<span>Separation of duties</span>
					<strong>{intent.approvalRoute?.separationOfDuties ? 'yes' : 'no'}</strong>
					<span>Deadline</span>
					<strong>{intent.approvalRoute?.deadlineSeconds ?? '-'}</strong>
				</div>
				<div data-testid="intent-origin-metadata">
					<h3>Origin metadata</h3>
					<div class="meta-grid">
						<span>Origin</span>
						<strong>{originKind(intent)}</strong>
						<span>Agent identity</span>
						<code>{agentIdentityLabel(intent)}</code>
						<span>Delegated authority</span>
						<code>{delegatedAuthority(intent)}</code>
						<span>Credential</span>
						<code>{credentialFor(intent)}</code>
						<span>Grant version</span>
						<strong>{grantVersion(intent)}</strong>
						<span>Tool name</span>
						<code>{toolName(intent)}</code>
					</div>
					<JsonTree data={originMetadata(intent)} />
				</div>
				<div data-testid="intent-tool-arguments">
					<h3>Tool arguments summary</h3>
					<JsonTree data={toolArgumentSummary(intent)} />
				</div>
				<div data-testid="intent-structured-outcome">
					<h3>Structured outcome</h3>
					<JsonTree data={structuredOutcome(intent)} />
				</div>
				<div>
					<h3>Subject snapshot</h3>
					<JsonTree data={intent.subject as JsonValue} />
				</div>
			</div>
		</section>

		<section class="panel" data-testid="intent-diff">
			<div class="panel-header">
				<h2>Diff</h2>
			</div>
			<div class="panel-body stack">
				{#if affectedFields(intent).length > 0}
					<div class="field-list">
						{#each affectedFields(intent) as field}
							<code>{field}</code>
						{/each}
					</div>
				{/if}
				{#if hasReviewDiff(intent)}
					<JsonTree data={intent.reviewSummary.diff as JsonValue} />
				{:else}
					<p class="muted">No review diff returned.</p>
				{/if}
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
				{:else}
					<p class="muted">No policy explanation returned.</p>
				{/if}

				<div class="table-wrap">
					<table data-testid="intent-pre-images">
						<thead>
							<tr>
								<th>Pre-image</th>
								<th>Version</th>
								<th></th>
							</tr>
						</thead>
						<tbody>
							{#each intent.preImages as record}
								<tr>
									<td><code>{record.collection}/{record.id ?? '-'}</code></td>
									<td>v{record.version ?? '-'}</td>
									<td>
										<a class="inline-link" href={recordHref(record.collection)}>Open collection</a>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Canonical operation</h2>
			</div>
			<div class="panel-body">
				<JsonTree data={intent.operation.operation as JsonValue} />
			</div>
		</section>

		<section class="panel" data-testid="intent-audit-trail">
			<div class="panel-header">
				<h2>Audit trail</h2>
				<span class="pill">{auditEntries.length} entries</span>
			</div>
			<div class="panel-body stack">
				{#if auditError}
					<p class="message error">{auditError}</p>
				{/if}
				{#if auditLoading}
					<p class="muted">Loading audit trail...</p>
				{:else if auditEntries.length === 0}
					<p class="muted">No audit entries were recorded for this intent yet.</p>
				{:else}
					<div class="table-wrap">
						<table>
							<thead>
								<tr>
									<th>When</th>
									<th>Event</th>
									<th>Actor</th>
									<th>Reason</th>
									<th>Policy</th>
									<th>Origin</th>
								</tr>
							</thead>
							<tbody>
								{#each auditEntries as entry}
									<tr>
										<td>{formatNs(entry.timestamp_ns)}</td>
										<td><code>{entry.mutation}</code></td>
										<td>{auditApprover(entry)}</td>
										<td>{auditReason(entry)}</td>
										<td>{entry.intent_lineage?.policy_version ?? '-'}</td>
										<td>{auditOrigin(entry)}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Actions</h2>
			</div>
			<div class="panel-body stack">
				<div class="action-block">
					<h3>Review decision</h3>
					<label>
						<span>Reason</span>
						<textarea
							bind:value={reviewReason}
							bind:this={reviewReasonField}
							aria-invalid={reviewReasonError ? 'true' : undefined}
							disabled={intent.approvalState !== 'pending'}
							data-testid="intent-reason"
						></textarea>
					</label>
					{#if reviewReasonError}
						<p class="message error" data-testid="intent-reason-error">{reviewReasonError}</p>
					{/if}
					<p class="muted" data-testid="intent-review-status">{reviewStatus(intent)}</p>
					<div class="actions">
						<button
							type="button"
							class="primary"
							disabled={!canApprove}
							onclick={() => void approveIntent()}
							data-testid="intent-approve"
						>
							{reviewing ? 'Approving...' : 'Approve'}
						</button>
						<button
							type="button"
							class="danger"
							disabled={!canReject}
							onclick={() => void rejectIntent()}
							data-testid="intent-reject"
						>
							{reviewing ? 'Rejecting...' : 'Reject'}
						</button>
					</div>
				</div>

				<div class="action-block">
					<h3>Commit</h3>
					<label>
						<span>Intent token</span>
						<input
							bind:value={commitToken}
							type="password"
							disabled={
								(intent.approvalState !== 'approved' && intent.approvalState !== 'none') ||
								commitBlockedByStale
							}
							data-testid="intent-commit-token"
						/>
					</label>
					<p class="muted" data-testid="intent-commit-status">{commitStatus(intent)}</p>
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
				</div>
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

	.back-link:hover,
	.inline-link:hover {
		color: var(--accent);
	}

	.detail-grid {
		display: grid;
		grid-template-columns: minmax(0, 1.2fr) minmax(22rem, 0.8fr);
		gap: 1rem;
	}

	.status-stack,
	.link-list,
	.field-list {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.execution-callout {
		display: grid;
		gap: 0.3rem;
		padding: 0.85rem 1rem;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
		background: rgba(125, 211, 252, 0.08);
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

	.stack {
		display: grid;
		gap: 1rem;
	}

	.action-block {
		display: grid;
		gap: 0.75rem;
	}

	.policy-list {
		margin: 0;
		padding-left: 1.2rem;
	}

	.table-wrap {
		overflow-x: auto;
	}

	.inline-link {
		text-decoration: none;
	}

	textarea {
		min-height: 6rem;
	}

	textarea[aria-invalid='true'] {
		border-color: rgba(248, 113, 113, 0.7);
	}

	@media (max-width: 1000px) {
		.detail-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
