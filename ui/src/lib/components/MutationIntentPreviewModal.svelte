<script lang="ts">
import type {
	CommitMutationIntentOutcome,
	MutationIntentError,
	MutationPreviewResult,
} from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template as a component.
import JsonTree from '$lib/components/JsonTree.svelte';
// biome-ignore lint/correctness/noUnusedImports: Used in template for JsonTree data casts.
import type { JsonValue } from '$lib/components/json-tree-types';

type Props = {
	open: boolean;
	preview: MutationPreviewResult | null;
	commitOutcome?: CommitMutationIntentOutcome | null;
	commitError?: MutationIntentError | null;
	committing?: boolean;
	rePreviewing?: boolean;
	dirty?: boolean;
	intentDetailHref?: string | null;
	previousIntentId?: string | null;
	previousIntentHref?: string | null;
	onClose?: () => void;
	onCommit?: () => void;
	onRePreview?: () => void;
};

const {
	open,
	preview,
	commitOutcome = null,
	commitError = null,
	committing = false,
	rePreviewing = false,
	dirty = false,
	intentDetailHref = null,
	previousIntentId = null,
	previousIntentHref = null,
	onClose,
	onCommit,
	onRePreview,
}: Props = $props();

const effectiveError = $derived(
	commitError ?? (commitOutcome && !commitOutcome.ok ? commitOutcome.error : null),
);
const committed = $derived(commitOutcome?.ok ? commitOutcome.result : null);
const intent = $derived(preview?.intent ?? committed?.intent ?? null);
const status = $derived(
	effectiveError?.code ??
		committed?.errorCode ??
		(committed ? 'committed' : preview?.decision) ??
		'unknown',
);
// `intent_stale` and `intent_mismatch` invalidate the previewed token: the
// committed operation does not match what the policy and pre-images said it
// would, so the user must re-preview to obtain a fresh intent (and a fresh
// token bound to the new pre-image versions). Surface a re-preview affordance
// rather than letting the user re-click commit on a known-bad token.
const isStaleOrMismatch = $derived(
	effectiveError?.code === 'intent_stale' || effectiveError?.code === 'intent_mismatch',
);
const canCommit = $derived(
	preview?.decision === 'allow' &&
		typeof preview.intentToken === 'string' &&
		preview.intentToken.length > 0 &&
		!committed &&
		!effectiveError &&
		!dirty,
);
const canRePreview = $derived(!!onRePreview && (isStaleOrMismatch || dirty) && !committed);

// biome-ignore lint/style/useConst: Svelte bind:this mutates this state.
let rePreviewButton: HTMLButtonElement | undefined = $state();
$effect(() => {
	// When the preview becomes dirty while the modal is open, move focus
	// to the primary re-preview action so the user does not commit a
	// stale token by reflex. Acceptance: focus returns to the changed
	// field or primary re-preview action.
	if (open && dirty && canRePreview && rePreviewButton) {
		rePreviewButton.focus();
	}
});

function formatNs(value: string | undefined): string {
	if (!value) return 'not set';
	const parsed = Number(value);
	if (!Number.isFinite(parsed)) return value;
	return new Date(Math.floor(parsed / 1_000_000)).toLocaleString();
}
</script>

{#if open && preview}
	<div class="modal-backdrop" role="presentation" data-testid="mutation-intent-modal">
		<div class="intent-modal" role="dialog" aria-modal="true" aria-labelledby="intent-title">
			<header class="modal-header">
				<div>
					<h2 id="intent-title">Mutation intent</h2>
					<p class="muted">
						{intent?.id ?? 'preview'} · expires {formatNs(intent?.expiresAtNs)}
					</p>
					{#if previousIntentId}
						<p class="muted" data-testid="intent-lineage-supersedes">
							Supersedes
							{#if previousIntentHref}
								<a href={previousIntentHref} data-testid="intent-lineage-link">{previousIntentId}</a>
							{:else}
								<code>{previousIntentId}</code>
							{/if}
						</p>
					{/if}
				</div>
				<span class:danger={preview.decision === 'deny' || !!effectiveError} class="intent-status">
					{status}
				</span>
			</header>

			<div class="modal-grid">
				<section class="modal-section">
					<h3>Operation</h3>
					<div class="meta-grid">
						<span>Kind</span>
						<code>{preview.canonicalOperation.operationKind}</code>
						<span>Hash</span>
						<code>{preview.canonicalOperation.operationHash}</code>
						<span>Decision</span>
						<strong>{preview.decision}</strong>
					</div>
					<JsonTree data={preview.canonicalOperation.operation as JsonValue} />
				</section>

				<section class="modal-section">
					<h3>Diff</h3>
					<JsonTree data={preview.diff as JsonValue} />
				</section>
			</div>

			<div class="modal-grid compact">
				<section class="modal-section">
					<h3>Affected rows</h3>
					{#if preview.affectedRecords.length === 0}
						<p class="muted">None</p>
					{:else}
						<ul class="rows-list">
							{#each preview.affectedRecords as record}
								<li>
									<code>{record.collection}/{record.id ?? '-'}</code>
									<span>v{record.version ?? '-'}</span>
								</li>
							{/each}
						</ul>
					{/if}
				</section>

				<section class="modal-section">
					<h3>Fields</h3>
					{#if preview.affectedFields.length === 0}
						<p class="muted">None</p>
					{:else}
						<div class="field-list">
							{#each preview.affectedFields as field}
								<code>{field}</code>
							{/each}
						</div>
					{/if}
				</section>
			</div>

			{#if preview.approvalRoute}
				<section class="modal-section">
					<h3>Approval</h3>
					<div class="meta-grid">
						<span>Role</span>
						<code>{preview.approvalRoute.role}</code>
						<span>Reason required</span>
						<strong>{preview.approvalRoute.reasonRequired ? 'yes' : 'no'}</strong>
						<span>Separation of duties</span>
						<strong>{preview.approvalRoute.separationOfDuties ? 'yes' : 'no'}</strong>
					</div>
					{#if intent?.id && intentDetailHref}
						<a
							class="intent-detail-link"
							href={intentDetailHref}
							data-testid="intent-open-pending-detail"
						>
							Open pending intent detail
						</a>
					{/if}
				</section>
			{/if}

			<section class="modal-section">
				<h3>Policy explanation</h3>
				{#if preview.policyExplanation.length === 0}
					<p class="muted">No policy explanation returned.</p>
				{:else}
					<ul class="policy-list" data-testid="intent-policy-explanation">
						{#each preview.policyExplanation as line}
							<li>{line}</li>
						{/each}
					</ul>
				{/if}
			</section>

			{#if dirty}
				<section class="modal-section dirty-box" data-testid="intent-dirty-notice">
					<h3>Preview out of date</h3>
					<p>
						You changed the form after this preview was taken. Re-preview to
						bind a fresh intent before committing.
					</p>
				</section>
			{/if}

			{#if effectiveError}
				<section class="modal-section error-box" data-testid="intent-error">
					<h3>{effectiveError.code ?? 'intent_error'}</h3>
					<p>{effectiveError.message}</p>
					{#if effectiveError.stale.length > 0}
						<table>
							<thead>
								<tr>
									<th>Dimension</th>
									<th>Expected</th>
									<th>Actual</th>
									<th>Path</th>
								</tr>
							</thead>
							<tbody>
								{#each effectiveError.stale as stale}
									<tr>
										<td>{stale.dimension}</td>
										<td>{stale.expected ?? '-'}</td>
										<td>{stale.actual ?? '-'}</td>
										<td>{stale.path ?? '-'}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				</section>
			{/if}

			<footer class="modal-actions">
				<button type="button" data-testid="intent-close" onclick={() => onClose?.()}>Close</button>
				{#if canRePreview}
					<button
						type="button"
						bind:this={rePreviewButton}
						disabled={rePreviewing}
						onclick={() => onRePreview?.()}
						data-testid="intent-re-preview"
					>
						{rePreviewing ? 'Re-previewing...' : 'Re-preview'}
					</button>
				{/if}
				<button
					type="button"
					class="primary"
					disabled={!canCommit || committing || rePreviewing}
					onclick={() => onCommit?.()}
					data-testid="intent-commit"
				>
					{committing ? 'Committing...' : committed ? 'Committed' : 'Commit'}
				</button>
			</footer>
		</div>
	</div>
{/if}

<style>
	.modal-backdrop {
		position: fixed;
		inset: 0;
		z-index: 40;
		display: grid;
		place-items: center;
		padding: 1rem;
		background: rgba(0, 0, 0, 0.6);
	}

	.intent-modal {
		width: min(72rem, 100%);
		max-height: min(92vh, 60rem);
		overflow: auto;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
		background: var(--panel);
		box-shadow: var(--shadow);
		padding: 1rem;
	}

	.modal-header,
	.modal-actions,
	.modal-grid,
	.meta-grid,
	.rows-list li,
	.field-list {
		display: flex;
		gap: 0.75rem;
	}

	.modal-header,
	.modal-actions {
		align-items: center;
		justify-content: space-between;
	}

	.modal-header {
		margin-bottom: 1rem;
	}

	h2,
	h3,
	p {
		margin: 0;
	}

	h3 {
		margin-bottom: 0.75rem;
		font-size: 0.9rem;
		color: var(--muted);
		text-transform: uppercase;
	}

	.intent-status {
		border: 1px solid rgba(125, 211, 252, 0.3);
		border-radius: 0.5rem;
		padding: 0.35rem 0.65rem;
		color: var(--accent);
		font-weight: 700;
	}

	.intent-status.danger {
		border-color: rgba(251, 113, 133, 0.35);
		color: var(--danger);
	}

	.modal-grid {
		align-items: stretch;
		margin-bottom: 1rem;
	}

	.modal-grid > * {
		flex: 1 1 0;
		min-width: 0;
	}

	.modal-grid.compact > * {
		flex-basis: 20rem;
	}

	.modal-section {
		margin-bottom: 1rem;
		border: 1px solid rgba(47, 55, 66, 0.8);
		border-radius: 0.5rem;
		padding: 1rem;
		background: rgba(15, 23, 32, 0.55);
	}

	.meta-grid {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		margin-bottom: 1rem;
		font-size: 0.88rem;
	}

	.meta-grid > span {
		color: var(--muted);
	}

	.rows-list,
	.policy-list {
		margin: 0;
		padding-left: 1.2rem;
	}

	.rows-list li {
		align-items: center;
		justify-content: space-between;
		margin-bottom: 0.4rem;
	}

	.field-list {
		flex-wrap: wrap;
	}

	.error-box {
		border-color: rgba(251, 113, 133, 0.35);
		background: rgba(251, 113, 133, 0.08);
	}

	.dirty-box {
		border-color: rgba(250, 204, 21, 0.45);
		background: rgba(250, 204, 21, 0.1);
	}

	.intent-detail-link {
		display: inline-block;
		margin-top: 0.5rem;
		color: var(--accent);
		text-decoration: underline;
		font-size: 0.88rem;
	}

	.modal-actions {
		position: sticky;
		bottom: -1rem;
		margin: 0 -1rem -1rem;
		padding: 1rem;
		border-top: 1px solid var(--border);
		background: var(--panel);
	}

	@media (max-width: 760px) {
		.modal-grid,
		.modal-header,
		.modal-actions {
			flex-direction: column;
			align-items: stretch;
		}
	}
</style>
