<script lang="ts">
import { goto } from '$app/navigation';
import { base } from '$app/paths';
import {
	type MutationIntent,
	type MutationIntentPageInfo,
	type MutationIntentStatusFilter,
	approveMutationIntent,
	fetchMutationIntents,
	rejectMutationIntent,
} from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template as a component.
import JsonTree from '$lib/components/JsonTree.svelte';
import type { JsonValue } from '$lib/components/json-tree-types';
import {
	agentIdentityLabel,
	credentialLabel,
	delegatedByLabel,
	grantVersionLabel,
	requesterLabel as intentRequesterLabel,
	isMcpOriginated,
	originBadge,
	structuredOutcomeSummary,
	tenantRoleLabel,
	toolArgumentsSummary,
	toolNameLabel,
} from '$lib/intent-metadata';
import { onMount } from 'svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const dbHref = $derived(
	`${base}/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}`,
);

const statusViews: MutationIntentStatusFilter[] = [
	'pending',
	'history',
	'approved',
	'rejected',
	'expired',
	'committed',
];

let activeStatus = $state<MutationIntentStatusFilter>('pending');
let intents = $state<MutationIntent[]>([]);
let pageInfo = $state<MutationIntentPageInfo | null>(null);
let totalCount = $state(0);
let loading = $state(true);
let error = $state<string | null>(null);
let requesterQuery = $state('');
let subjectQuery = $state('');
let roleFilter = $state('all');
let riskQuery = $state('');
let ageFilter = $state<'any' | 'last_hour' | 'last_day' | 'older_than_day'>('any');
let collectionFilter = $state('all');
let originFilter = $state('all');
let selectedIntentId = $state<string | null>(null);
let reviewReason = $state('');
let reviewReasonError = $state<string | null>(null);
let actionMessage = $state<string | null>(null);
let actionError = $state<string | null>(null);
let reviewing = $state(false);
// biome-ignore lint/style/useConst: Svelte bind:this assigns the element ref at runtime.
let reviewReasonField = $state<HTMLTextAreaElement | null>(null);

function formatNs(value: string | undefined): string {
	if (!value) return '-';
	const parsed = Number(value);
	if (!Number.isFinite(parsed)) return value;
	return new Date(Math.floor(parsed / 1_000_000)).toLocaleString();
}

function normalize(value: string | null | undefined): string {
	return (value ?? '').trim().toLowerCase();
}

function hasTextFilter(value: string): boolean {
	return value.trim().length > 0;
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

function requesterLabel(intent: MutationIntent): string {
	return intentRequesterLabel(intent);
}

function originLabel(intent: MutationIntent): string {
	const subject = asRecord(intent.subject);
	return (
		stringMember(subject, 'agent_id', 'agentId') ??
		stringMember(subject, 'credential_id', 'credentialId') ??
		'-'
	);
}

function roleLabel(intent: MutationIntent): string {
	return intent.approvalRoute?.role ?? '-';
}

function toolName(intent: MutationIntent): string {
	return toolNameLabel(intent);
}

function credentialFor(intent: MutationIntent): string {
	return credentialLabel(intent);
}

function grantVersionFor(intent: MutationIntent): string {
	return grantVersionLabel(intent);
}

function originKind(intent: MutationIntent): string {
	return originBadge(intent);
}

function displayOriginLabel(intent: MutationIntent): string {
	if (!isMcpOriginated(intent)) return 'UI GraphQL';
	return `${agentIdentityLabel(intent)} · ${toolName(intent)}`;
}

function delegatedAuthority(intent: MutationIntent): string {
	const delegatedBy = delegatedByLabel(intent);
	const tenantRole = tenantRoleLabel(intent);
	if (delegatedBy === '-' && tenantRole === '-') return '-';
	const parts = [
		delegatedBy !== '-' ? `delegated by ${delegatedBy}` : null,
		tenantRole !== '-' ? `role ${tenantRole}` : null,
	].filter((value): value is string => Boolean(value));
	return parts.join(' · ');
}

function outcomeRecord(intent: MutationIntent): Record<string, unknown> | null {
	return asRecord(structuredOutcomeSummary(intent));
}

function outcomeLabel(intent: MutationIntent): string {
	return stringMember(outcomeRecord(intent), 'outcome') ?? intent.decision;
}

function outcomeDetail(intent: MutationIntent): string {
	const errorCode = stringMember(outcomeRecord(intent), 'error_code');
	return errorCode ? `${intent.approvalState} · ${errorCode}` : intent.approvalState;
}

function toolArgumentSummary(intent: MutationIntent): JsonValue {
	return toolArgumentsSummary(intent.operation) as JsonValue;
}

function structuredOutcome(intent: MutationIntent): JsonValue {
	return structuredOutcomeSummary(intent) as JsonValue;
}

function collectionSummary(intent: MutationIntent): string {
	const collections = Array.from(
		new Set(
			intent.preImages.map((record) => record.collection).filter((value) => value.length > 0),
		),
	);
	return collections.join(', ') || '-';
}

function riskLabel(intent: MutationIntent): string {
	const parts = [
		intent.reviewSummary.risk,
		...(intent.reviewSummary.policy_explanation ?? []),
		intent.reviewSummary.summary,
	].filter((value): value is string => typeof value === 'string' && value.trim().length > 0);
	return parts.join(', ') || '-';
}

function subjectText(intent: MutationIntent): string {
	return [
		intent.id,
		intentTitle(intent),
		intent.reviewSummary.summary,
		recordLabel(intent),
		intent.operation.operationKind,
		collectionSummary(intent),
	]
		.filter((value) => value && value.length > 0)
		.join(' ');
}

function parseIntentCreatedNs(intent: MutationIntent): number | null {
	const match = /^mint_gql_(\d+)_\d+$/.exec(intent.id);
	if (!match) return null;
	const parsed = Number(match[1]);
	return Number.isFinite(parsed) ? parsed : null;
}

function ageMs(intent: MutationIntent): number | null {
	const createdNs = parseIntentCreatedNs(intent);
	if (createdNs === null) return null;
	return Date.now() - Math.floor(createdNs / 1_000_000);
}

function ageLabel(intent: MutationIntent): string {
	const elapsedMs = ageMs(intent);
	if (elapsedMs === null) return '-';
	if (elapsedMs < 60_000) return `${Math.max(1, Math.floor(elapsedMs / 1000))}s`;
	if (elapsedMs < 3_600_000) return `${Math.floor(elapsedMs / 60_000)}m`;
	if (elapsedMs < 86_400_000) return `${Math.floor(elapsedMs / 3_600_000)}h`;
	return `${Math.floor(elapsedMs / 86_400_000)}d`;
}

function matchesAge(intent: MutationIntent): boolean {
	const elapsedMs = ageMs(intent);
	if (ageFilter === 'any') return true;
	if (elapsedMs === null) return false;
	if (ageFilter === 'last_hour') return elapsedMs <= 3_600_000;
	if (ageFilter === 'last_day') return elapsedMs <= 86_400_000;
	return elapsedMs > 86_400_000;
}

function matchesText(candidate: string, query: string): boolean {
	return !hasTextFilter(query) || normalize(candidate).includes(normalize(query));
}

function matchesView(intent: MutationIntent, status: MutationIntentStatusFilter): boolean {
	if (status === 'history') {
		return ['approved', 'rejected', 'expired', 'committed'].includes(intent.approvalState);
	}
	if (status === 'all') {
		return true;
	}
	return intent.approvalState === status;
}

const roleOptions = $derived(
	Array.from(
		new Set(intents.map((intent) => roleLabel(intent)).filter((value) => value !== '-')),
	).sort(),
);
const collectionOptions = $derived(
	Array.from(
		new Set(
			intents
				.flatMap((intent) => intent.preImages.map((record) => record.collection))
				.filter((value) => value.length > 0),
		),
	).sort(),
);
const originOptions = $derived(
	Array.from(
		new Set(intents.map((intent) => originLabel(intent)).filter((value) => value !== '-')),
	).sort(),
);
const hasClientFilters = $derived(
	hasTextFilter(requesterQuery) ||
		hasTextFilter(subjectQuery) ||
		roleFilter !== 'all' ||
		hasTextFilter(riskQuery) ||
		ageFilter !== 'any' ||
		collectionFilter !== 'all' ||
		originFilter !== 'all',
);
const filteredIntents = $derived.by(() =>
	intents.filter((intent) => {
		const matchesRequester = matchesText(requesterLabel(intent), requesterQuery);
		const matchesSubject = matchesText(subjectText(intent), subjectQuery);
		const matchesRole = roleFilter === 'all' || roleLabel(intent) === roleFilter;
		const matchesRisk = matchesText(riskLabel(intent), riskQuery);
		const matchesCollection =
			collectionFilter === 'all' ||
			intent.preImages.some((record) => record.collection === collectionFilter);
		const matchesOrigin = originFilter === 'all' || originLabel(intent) === originFilter;
		return (
			matchesRequester &&
			matchesSubject &&
			matchesRole &&
			matchesRisk &&
			matchesCollection &&
			matchesOrigin &&
			matchesAge(intent)
		);
	}),
);
const selectedIntent = $derived(
	filteredIntents.find((intent) => intent.id === selectedIntentId) ?? null,
);
const canApprove = $derived(selectedIntent?.approvalState === 'pending' && !reviewing);
const canReject = $derived(selectedIntent?.approvalState === 'pending' && !reviewing);

function reviewStatus(intent: MutationIntent): string {
	switch (intent.approvalState) {
		case 'pending':
			return intent.approvalRoute?.reasonRequired
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

function validateSelectedReason(action: 'approve' | 'reject'): boolean {
	if (!selectedIntent || selectedIntent.approvalState !== 'pending') return false;
	const requiresReason =
		action === 'reject' || Boolean(selectedIntent.approvalRoute?.reasonRequired);
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

function clearFilters() {
	requesterQuery = '';
	subjectQuery = '';
	roleFilter = 'all';
	riskQuery = '';
	ageFilter = 'any';
	collectionFilter = 'all';
	originFilter = 'all';
}

async function loadIntents(
	status: MutationIntentStatusFilter = activeStatus,
	after: string | null = null,
) {
	loading = true;
	error = null;
	actionError = null;
	reviewReasonError = null;
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

async function selectStatus(status: MutationIntentStatusFilter) {
	activeStatus = status;
	actionMessage = null;
	actionError = null;
	reviewReasonError = null;
	await loadIntents(status, null);
}

function selectIntent(intentId: string) {
	selectedIntentId = intentId;
	actionMessage = null;
	actionError = null;
	reviewReasonError = null;
}

function moveSelection(offset: number) {
	if (filteredIntents.length === 0) return;
	const currentIndex = filteredIntents.findIndex((intent) => intent.id === selectedIntentId);
	const baseIndex = currentIndex === -1 ? 0 : currentIndex;
	const nextIndex = Math.min(filteredIntents.length - 1, Math.max(0, baseIndex + offset));
	updateSelection(filteredIntents[nextIndex]?.id ?? filteredIntents[0]?.id ?? null);
}

function updateSelection(intentId: string | null) {
	selectedIntentId = intentId;
}

async function openSelectedIntent() {
	if (!selectedIntent) return;
	await goto(intentHref(selectedIntent));
}

async function reviewSelected(action: 'approve' | 'reject') {
	if (!selectedIntent || !validateSelectedReason(action)) return;
	reviewing = true;
	actionMessage = null;
	actionError = null;
	try {
		const updated =
			action === 'approve'
				? await approveMutationIntent(scope, {
						intentId: selectedIntent.id,
						...(reviewReason.trim() ? { reason: reviewReason.trim() } : {}),
					})
				: await rejectMutationIntent(scope, {
						intentId: selectedIntent.id,
						reason: reviewReason.trim(),
					});
		const nextIntents = intents
			.map((intent) => (intent.id === updated.id ? updated : intent))
			.filter((intent) => matchesView(intent, activeStatus));
		if (nextIntents.length < intents.length) {
			totalCount = Math.max(0, totalCount - 1);
		}
		intents = nextIntents;
		reviewReason = '';
		reviewReasonError = null;
		actionMessage = action === 'approve' ? 'Intent approved.' : 'Intent rejected.';
	} catch (errorValue: unknown) {
		actionError = errorValue instanceof Error ? errorValue.message : `Failed to ${action} intent`;
	} finally {
		reviewing = false;
	}
}

function handleTableKeydown(event: KeyboardEvent) {
	if (filteredIntents.length === 0) return;
	if (event.key === 'ArrowDown') {
		event.preventDefault();
		moveSelection(1);
		return;
	}
	if (event.key === 'ArrowUp') {
		event.preventDefault();
		moveSelection(-1);
		return;
	}
	if (event.key === 'Home') {
		event.preventDefault();
		updateSelection(filteredIntents[0]?.id ?? null);
		return;
	}
	if (event.key === 'End') {
		event.preventDefault();
		updateSelection(filteredIntents.at(-1)?.id ?? null);
		return;
	}
	if (event.key === 'Enter') {
		event.preventDefault();
		void openSelectedIntent();
	}
}

$effect(() => {
	if (roleFilter !== 'all' && !roleOptions.includes(roleFilter)) {
		roleFilter = 'all';
	}
	if (collectionFilter !== 'all' && !collectionOptions.includes(collectionFilter)) {
		collectionFilter = 'all';
	}
	if (originFilter !== 'all' && !originOptions.includes(originFilter)) {
		originFilter = 'all';
	}
});

$effect(() => {
	if (filteredIntents.length === 0) {
		if (selectedIntentId !== null) {
			selectedIntentId = null;
		}
		return;
	}
	if (!selectedIntentId || !filteredIntents.some((intent) => intent.id === selectedIntentId)) {
		selectedIntentId = filteredIntents[0]?.id ?? null;
	}
});

$effect(() => {
	if (selectedIntent?.approvalState !== 'pending') {
		reviewReason = '';
		reviewReasonError = null;
	}
});

$effect(() => {
	if (reviewReason.trim().length > 0 && reviewReasonError) {
		reviewReasonError = null;
	}
});

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
			{#each statusViews as status}
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
		<span class="muted">{filteredIntents.length} shown of {totalCount}</span>
	</div>
	<div class="panel-body">
		<div class="filter-grid">
			<label>
				<span>Requester</span>
				<input bind:value={requesterQuery} data-testid="intent-filter-requester" />
			</label>
			<label>
				<span>Subject</span>
				<input bind:value={subjectQuery} data-testid="intent-filter-subject" />
			</label>
			<label>
				<span>Approver role</span>
				<select bind:value={roleFilter} data-testid="intent-filter-role">
					<option value="all">All roles</option>
					{#each roleOptions as role}
						<option value={role}>{role}</option>
					{/each}
				</select>
			</label>
			<label>
				<span>Risk / rule</span>
				<input bind:value={riskQuery} data-testid="intent-filter-risk" />
			</label>
			<label>
				<span>Age</span>
				<select bind:value={ageFilter} data-testid="intent-filter-age">
					<option value="any">Any age</option>
					<option value="last_hour">Last hour</option>
					<option value="last_day">Last day</option>
					<option value="older_than_day">Older than 1 day</option>
				</select>
			</label>
			<label>
				<span>Collection</span>
				<select bind:value={collectionFilter} data-testid="intent-filter-collection">
					<option value="all">All collections</option>
					{#each collectionOptions as collection}
						<option value={collection}>{collection}</option>
					{/each}
				</select>
			</label>
			<label>
				<span>MCP / tool</span>
				<select bind:value={originFilter} data-testid="intent-filter-origin">
					<option value="all">All origins</option>
					{#each originOptions as origin}
						<option value={origin}>{origin}</option>
					{/each}
				</select>
			</label>
			<div class="filter-actions">
				<button type="button" onclick={clearFilters} disabled={!hasClientFilters}>
					Clear filters
				</button>
			</div>
		</div>

		{#if error}
			<p class="message error" data-testid="intent-error">{error}</p>
		{:else if loading}
			<p class="muted" data-testid="intent-loading">Loading intents...</p>
		{:else if filteredIntents.length === 0}
			<p class="muted" data-testid="intent-empty">
				{#if intents.length === 0}
					No {activeStatus} intents.
				{:else}
					No intents match the current filters.
				{/if}
			</p>
		{:else}
			<div class="inbox-grid">
				<div
					class="table-wrap"
					role="grid"
					aria-label="Mutation intents"
					tabindex="0"
					onkeydown={handleTableKeydown}
					data-testid="intent-inbox-grid"
				>
				<table data-testid="intent-inbox-table">
					<thead>
						<tr>
							<th>Intent</th>
							<th>Requester</th>
							<th>Role</th>
							<th>Origin</th>
							<th>Collection</th>
							<th>Outcome</th>
							<th>Fields</th>
							<th>Age</th>
							<th>Expires</th>
							<th></th>
						</tr>
					</thead>
					<tbody>
						{#each filteredIntents as intent}
							<tr
								data-testid={`intent-row-${intent.id}`}
								class:selected={selectedIntentId === intent.id}
								aria-selected={selectedIntentId === intent.id}
								onclick={() => selectIntent(intent.id)}
							>
								<td>
									<div class="intent-link">
										<code>{intent.id}</code>
										<span>{intentTitle(intent)}</span>
										<span class="state">{intent.decision} / {intent.approvalState}</span>
									</div>
								</td>
								<td><code>{requesterLabel(intent)}</code></td>
								<td><code>{roleLabel(intent)}</code></td>
								<td data-testid={`intent-origin-${intent.id}`}>
									<div class="origin-stack">
										<span class:ui-origin={!isMcpOriginated(intent)} class="origin-pill">
											{originKind(intent)}
										</span>
										<code>{displayOriginLabel(intent)}</code>
										{#if isMcpOriginated(intent)}
											<span class="muted small-copy">{delegatedAuthority(intent)}</span>
										{/if}
									</div>
								</td>
								<td><code>{collectionSummary(intent)}</code></td>
								<td data-testid={`intent-outcome-${intent.id}`}>
									<div class="origin-stack">
										<strong>{outcomeLabel(intent)}</strong>
										<span class="muted small-copy">{outcomeDetail(intent)}</span>
									</div>
								</td>
								<td>{fieldSummary(intent)}</td>
								<td>{ageLabel(intent)}</td>
								<td>{formatNs(intent.expiresAtNs)}</td>
								<td>
									<button
										type="button"
										class="table-action"
										onclick={(event) => {
											event.stopPropagation();
											selectIntent(intent.id);
											void goto(intentHref(intent));
										}}
									>
										Detail
									</button>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
				</div>
				{#if selectedIntent}
					<aside class="selection-panel" data-testid="intent-selection-panel">
						<div class="selection-header">
							<div>
								<h2>{intentTitle(selectedIntent)}</h2>
								<p class="muted"><code>{selectedIntent.id}</code></p>
							</div>
							<button
								type="button"
								class="primary"
								onclick={() => void openSelectedIntent()}
								data-testid="intent-open-detail"
							>
								Open detail
							</button>
						</div>
						{#if actionError}
							<p class="message error" data-testid="intent-inline-error">{actionError}</p>
						{/if}
						{#if actionMessage}
							<p class="message success" data-testid="intent-inline-message">{actionMessage}</p>
						{/if}
						<div class="meta-grid compact-grid">
							<span>Record</span>
							<code>{recordLabel(selectedIntent)}</code>
							<span>Operation</span>
							<code>{selectedIntent.operation.operationKind}</code>
							<span>Requester</span>
							<code>{requesterLabel(selectedIntent)}</code>
							<span>Role</span>
							<code>{roleLabel(selectedIntent)}</code>
							<span>Origin</span>
							<code>{originLabel(selectedIntent)}</code>
							<span>Tool</span>
							<code>{toolName(selectedIntent)}</code>
							<span>Grant</span>
							<code>{grantVersionFor(selectedIntent)}</code>
							<span>Risk</span>
							<strong>{selectedIntent.reviewSummary.risk ?? '-'}</strong>
						</div>
						<div class="stack">
							<div data-testid="intent-inline-mcp">
								<h3>MCP metadata</h3>
								<div class="meta-grid compact-grid">
									<span>Origin</span>
									<strong>{originKind(selectedIntent)}</strong>
									<span>Agent identity</span>
									<code>{agentIdentityLabel(selectedIntent)}</code>
									<span>Delegated authority</span>
									<code>{delegatedAuthority(selectedIntent)}</code>
									<span>Credential</span>
									<code>{credentialFor(selectedIntent)}</code>
									<span>Grant version</span>
									<strong>{grantVersionFor(selectedIntent)}</strong>
									<span>Tool name</span>
									<code>{toolName(selectedIntent)}</code>
								</div>
							</div>
							<div data-testid="intent-inline-tool-arguments">
								<h3>Tool arguments summary</h3>
								<JsonTree data={toolArgumentSummary(selectedIntent)} />
							</div>
							<div data-testid="intent-inline-structured-outcome">
								<h3>Structured outcome</h3>
								<JsonTree data={structuredOutcome(selectedIntent)} />
							</div>
							<div>
								<h3>Policy explanation</h3>
								{#if (selectedIntent.reviewSummary.policy_explanation ?? []).length === 0}
									<p class="muted">No policy explanation returned.</p>
								{:else}
									<ul class="policy-list">
										{#each selectedIntent.reviewSummary.policy_explanation ?? [] as line}
											<li>{line}</li>
										{/each}
									</ul>
								{/if}
							</div>
							<div>
								<h3>Summary</h3>
								<p class="muted">{selectedIntent.reviewSummary.summary || '-'}</p>
							</div>
						</div>
						<label>
							<span>Review reason</span>
							<textarea
								bind:value={reviewReason}
								bind:this={reviewReasonField}
								aria-invalid={reviewReasonError ? 'true' : undefined}
								disabled={selectedIntent.approvalState !== 'pending'}
								data-testid="intent-inline-reason"
							></textarea>
						</label>
						{#if reviewReasonError}
							<p class="message error" data-testid="intent-inline-reason-error">
								{reviewReasonError}
							</p>
						{/if}
						<p class="muted" data-testid="intent-inline-status">{reviewStatus(selectedIntent)}</p>
						<div class="actions">
							<button
								type="button"
								class="primary"
								disabled={!canApprove}
								onclick={() => void reviewSelected('approve')}
								data-testid="intent-inline-approve"
							>
								{reviewing ? 'Approving...' : 'Approve'}
							</button>
							<button
								type="button"
								class="danger"
								disabled={!canReject}
								onclick={() => void reviewSelected('reject')}
								data-testid="intent-inline-reject"
							>
								{reviewing ? 'Rejecting...' : 'Reject'}
							</button>
						</div>
					</aside>
				{/if}
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
	.filter-grid,
	.inbox-grid,
	.selection-header,
	.compact-grid,
	.filter-actions,
	.origin-stack {
		display: grid;
		gap: 0.75rem;
	}

	.filter-grid {
		grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
		margin-bottom: 1rem;
		align-items: end;
	}

	.filter-grid label {
		display: grid;
		gap: 0.35rem;
	}

	.filter-grid span,
	.selection-panel h3 {
		font-size: 0.85rem;
		color: var(--muted);
	}

	.small-copy {
		font-size: 0.8rem;
	}

	.filter-actions {
		align-items: end;
	}

	.status-tabs {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}

	.origin-pill {
		display: inline-flex;
		width: fit-content;
		padding: 0.15rem 0.5rem;
		border-radius: 999px;
		border: 1px solid rgba(125, 211, 252, 0.35);
		font-size: 0.72rem;
		font-weight: 600;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		color: var(--accent);
	}

	.origin-pill.ui-origin {
		border-color: rgba(148, 163, 184, 0.35);
		color: var(--muted);
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
		outline: none;
		border: 1px solid transparent;
		border-radius: 0.5rem;
	}

	.table-wrap:focus-visible {
		border-color: rgba(125, 211, 252, 0.55);
	}

	.intent-link {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	.inbox-grid {
		grid-template-columns: minmax(0, 2fr) minmax(18rem, 1fr);
		align-items: start;
	}

	tbody tr {
		cursor: pointer;
	}

	tbody tr.selected {
		background: rgba(125, 211, 252, 0.08);
	}

	.table-action {
		padding: 0.35rem 0.6rem;
	}

	.state {
		color: var(--muted);
		font-size: 0.8rem;
	}

	.selection-panel {
		display: grid;
		gap: 1rem;
		padding: 1rem;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
		background: rgba(15, 23, 42, 0.35);
	}

	.selection-header {
		grid-template-columns: minmax(0, 1fr) auto;
		align-items: start;
	}

	.selection-header h2,
	.selection-header p,
	.selection-panel h3 {
		margin: 0;
	}

	.compact-grid {
		grid-template-columns: auto minmax(0, 1fr);
	}

	.stack {
		display: grid;
		gap: 0.75rem;
	}

	.policy-list {
		margin: 0;
		padding-left: 1rem;
	}

	textarea {
		min-height: 6rem;
	}

	textarea[aria-invalid='true'] {
		border-color: rgba(248, 113, 113, 0.7);
	}

	.pager {
		justify-content: flex-end;
		margin-top: 1rem;
	}

	@media (max-width: 1100px) {
		.inbox-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
