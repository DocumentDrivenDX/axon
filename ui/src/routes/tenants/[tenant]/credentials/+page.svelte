<script lang="ts">
// biome-ignore lint/correctness/noUnusedImports: Used in template as Svelte component.
import UserPicker from '$lib/UserPicker.svelte';
import {
	type Credential,
	type GrantedDatabase,
	type IssueCredentialRequest,
	issueCredential,
	listCredentials,
	revokeCredential,
} from '$lib/api';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const tenant = $derived(data.tenant);

const OPS = ['read', 'write', 'admin'] as const;
type Op = (typeof OPS)[number];

let credentials = $state<Credential[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);

// Issue modal
let showIssueModal = $state(false);
let issueTargetUser = $state('');
let issueTtlSeconds = $state(3600);
let issueGrants = $state<GrantedDatabase[]>([{ name: '', ops: [] }]);
let issuing = $state(false);
let issueError = $state<string | null>(null);

// JWT one-time display
let jwtValue = $state<string | null>(null);
let jwtJti = $state<string | null>(null);
let jwtCopied = $state(false);

// Revoke confirmation
let confirmRevokeJti = $state<string | null>(null);
let revoking = $state(false);
let revokeError = $state<string | null>(null);

function formatMs(ms: number): string {
	return new Date(ms).toLocaleString();
}

function truncate(s: string, n = 12): string {
	return s.length > n ? `${s.slice(0, n)}…` : s;
}

async function loadCredentials() {
	loading = true;
	try {
		credentials = await listCredentials(tenant.id);
		error = null;
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to load credentials';
	} finally {
		loading = false;
	}
}

function openIssueModal() {
	issueTargetUser = '';
	issueTtlSeconds = 3600;
	issueGrants = [{ name: '', ops: [] }];
	issueError = null;
	showIssueModal = true;
}

function addGrantRow() {
	issueGrants = [...issueGrants, { name: '', ops: [] }];
}

function removeGrantRow(i: number) {
	issueGrants = issueGrants.filter((_, idx) => idx !== i);
}

function toggleOp(i: number, op: Op) {
	issueGrants = issueGrants.map((g, idx) => {
		if (idx !== i) return g;
		const ops = g.ops.includes(op) ? g.ops.filter((o) => o !== op) : ([...g.ops, op] as Op[]);
		return { ...g, ops };
	});
}

async function handleIssue() {
	issuing = true;
	issueError = null;
	try {
		const body: IssueCredentialRequest = {
			target_user: issueTargetUser.trim(),
			ttl_seconds: issueTtlSeconds,
			grants: { databases: issueGrants.filter((g) => g.name.trim()) },
		};
		const result = await issueCredential(tenant.id, body);
		showIssueModal = false;
		jwtValue = result.jwt;
		jwtJti = result.jti;
		await loadCredentials();
	} catch (e: unknown) {
		issueError = e instanceof Error ? e.message : 'Failed to issue credential';
	} finally {
		issuing = false;
	}
}

async function copyJwt() {
	if (!jwtValue) return;
	try {
		await navigator.clipboard.writeText(jwtValue);
	} catch {
		// Fallback for non-secure contexts
		const ta = document.createElement('textarea');
		ta.value = jwtValue;
		ta.style.position = 'fixed';
		ta.style.opacity = '0';
		document.body.appendChild(ta);
		ta.select();
		// biome-ignore lint/suspicious/noExplicitAny: legacy clipboard fallback
		(document as any).execCommand('copy');
		document.body.removeChild(ta);
	}
	jwtCopied = true;
	setTimeout(() => {
		jwtCopied = false;
	}, 2000);
}

function closeJwtDialog() {
	jwtValue = null;
	jwtJti = null;
	jwtCopied = false;
}

async function handleRevoke(jti: string) {
	revoking = true;
	revokeError = null;
	try {
		await revokeCredential(tenant.id, jti);
		confirmRevokeJti = null;
		await loadCredentials();
	} catch (e: unknown) {
		revokeError = e instanceof Error ? e.message : 'Failed to revoke credential';
	} finally {
		revoking = false;
	}
}

$effect(() => {
	void loadCredentials();
});
</script>

<div class="page-header">
	<div>
		<h1>Credentials</h1>
		<p class="muted">JWT credentials for tenant <strong>{tenant.name}</strong>.</p>
	</div>
	<div class="header-actions">
		<button class="primary" onclick={openIssueModal}>Issue Credential</button>
	</div>
</div>

{#if loading}
	<p class="message">Loading credentials…</p>
{:else if error}
	<p class="message error">{error}</p>
{:else if credentials.length === 0}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">No credentials yet. Click "Issue Credential" to create one.</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Credentials</h2>
			<span class="pill">{credentials.length}</span>
		</div>
		<div class="panel-body">
			{#if revokeError}
				<p class="message error">{revokeError}</p>
			{/if}
			<table>
				<thead>
					<tr>
						<th>JTI</th>
						<th>Target User</th>
						<th>Issued</th>
						<th>Expires</th>
						<th>Grants</th>
						<th>Status</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each credentials as cred}
						<tr class={cred.revoked ? 'revoked-row' : ''}>
							<td><code title={cred.jti}>{truncate(cred.jti)}</code></td>
							<td><code title={cred.user_id}>{truncate(cred.user_id)}</code></td>
							<td class="muted">{formatMs(cred.issued_at_ms)}</td>
							<td class="muted">{formatMs(cred.expires_at_ms)}</td>
							<td>
								{#if cred.grants.databases.length === 0}
									<span class="muted">none</span>
								{:else}
									{cred.grants.databases
										.map((db) => `${db.name}: ${db.ops.join(', ')}`)
										.join(' | ')}
								{/if}
							</td>
							<td>
								{#if cred.revoked}
									<span class="pill pill-error">revoked</span>
								{:else}
									<span class="pill">active</span>
								{/if}
							</td>
							<td>
								{#if !cred.revoked}
									{#if confirmRevokeJti === cred.jti}
										<div class="row-actions">
											<span class="muted confirm-label">Revoke?</span>
											<button
												class="danger"
												disabled={revoking}
												onclick={() => void handleRevoke(cred.jti)}
											>
												{revoking ? '…' : 'Confirm'}
											</button>
											<button onclick={() => (confirmRevokeJti = null)}>Cancel</button>
										</div>
									{:else}
										<button class="danger" onclick={() => (confirmRevokeJti = cred.jti)}>
											Revoke
										</button>
									{/if}
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
{/if}

<!-- Issue Credential Modal -->
{#if showIssueModal}
	<div
		class="modal-overlay"
		role="dialog"
		aria-modal="true"
		aria-label="Issue Credential"
	>
		<div class="modal">
			<div class="modal-header">
				<h2>Issue Credential</h2>
				<button class="close-btn" onclick={() => (showIssueModal = false)}>✕</button>
			</div>
			<form
				class="modal-body stack"
				onsubmit={(e) => {
					e.preventDefault();
					void handleIssue();
				}}
			>
				<div class="field-group">
					<span class="field-label">Target User</span>
					<UserPicker
						value={issueTargetUser || null}
						onselect={(id) => (issueTargetUser = id)}
						disabled={issuing}
						placeholder="Search or pick user…"
					/>
				</div>
				<label>
					<span>TTL (seconds)</span>
					<input
						type="number"
						min="1"
						bind:value={issueTtlSeconds}
						disabled={issuing}
						required
					/>
				</label>
				<div class="grants-section">
					<div class="grants-header">
						<strong>Database Grants</strong>
						<button type="button" onclick={addGrantRow} disabled={issuing}>+ Add Database</button>
					</div>
					{#each issueGrants as row, i}
						<div class="grant-row">
							<input
								type="text"
								placeholder="Database name"
								bind:value={row.name}
								disabled={issuing}
							/>
							<div class="ops-group">
								{#each OPS as op}
									<label class="op-label">
										<input
											type="checkbox"
											checked={row.ops.includes(op)}
											disabled={issuing}
											onchange={() => toggleOp(i, op)}
										/>
										{op}
									</label>
								{/each}
							</div>
							{#if issueGrants.length > 1}
								<button
									type="button"
									class="danger"
									onclick={() => removeGrantRow(i)}
									disabled={issuing}
								>
									Remove
								</button>
							{/if}
						</div>
					{/each}
				</div>
				{#if issueError}
					<p class="message error">{issueError}</p>
				{/if}
				<div class="modal-actions">
					<button type="button" onclick={() => (showIssueModal = false)} disabled={issuing}>
						Cancel
					</button>
					<button type="submit" class="primary" disabled={issuing || !issueTargetUser.trim()}>
						{issuing ? 'Issuing…' : 'Issue'}
					</button>
				</div>
			</form>
		</div>
	</div>
{/if}

<!-- JWT One-Time Display Dialog -->
{#if jwtValue}
	<div
		class="modal-overlay"
		role="dialog"
		aria-modal="true"
		aria-label="Credential Issued"
	>
		<div class="modal">
			<div class="modal-header">
				<h2>Credential Issued</h2>
			</div>
			<div class="modal-body stack">
				<p class="muted">
					This JWT is shown <strong>once only</strong> and cannot be retrieved again. Copy it now.
				</p>
				{#if jwtJti}
					<p class="muted">JTI: <code>{jwtJti}</code></p>
				{/if}
				<textarea class="jwt-display" rows={5} readonly>{jwtValue}</textarea>
				<div class="modal-actions">
					<button class="primary" onclick={copyJwt}>
						{jwtCopied ? 'Copied!' : 'Copy to Clipboard'}
					</button>
					<button onclick={closeJwtDialog}>Close</button>
				</div>
			</div>
		</div>
	</div>
{/if}

<style>
	.page-header {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 1rem;
		margin-bottom: 1rem;
	}

	.header-actions {
		flex-shrink: 0;
	}

	code {
		font-family: monospace;
		font-size: 0.85em;
		background: rgba(255, 255, 255, 0.06);
		padding: 0.1em 0.35em;
		border-radius: 0.25rem;
	}

	.revoked-row {
		opacity: 0.6;
	}

	.row-actions {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}

	.confirm-label {
		font-size: 0.85rem;
	}

	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}

	/* ── Modal ───────────────────────────────────────────────────────────── */

	.modal-overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.6);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 100;
	}

	.modal {
		background: var(--surface, #1e1e2e);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 0.75rem;
		width: min(42rem, 90vw);
		max-height: 85vh;
		overflow-y: auto;
	}

	.modal-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 1rem 1.25rem 0.75rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
	}

	.modal-header h2 {
		margin: 0;
		font-size: 1.1rem;
	}

	.close-btn {
		background: none;
		border: none;
		font-size: 1rem;
		cursor: pointer;
		color: var(--muted);
		padding: 0.25rem 0.5rem;
	}

	.modal-body {
		padding: 1.25rem;
	}

	.modal-actions {
		display: flex;
		gap: 0.5rem;
		justify-content: flex-end;
		margin-top: 0.5rem;
	}

	/* ── Grants builder ──────────────────────────────────────────────────── */

	.grants-section {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.grants-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.grant-row {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		flex-wrap: wrap;
	}

	.grant-row input[type='text'] {
		flex: 1;
		min-width: 8rem;
	}

	.ops-group {
		display: flex;
		gap: 0.75rem;
		align-items: center;
	}

	.op-label {
		display: flex;
		align-items: center;
		gap: 0.25rem;
		font-size: 0.875rem;
		cursor: pointer;
	}

	/* ── JWT display ─────────────────────────────────────────────────────── */

	.jwt-display {
		width: 100%;
		font-family: monospace;
		font-size: 0.8rem;
		background: rgba(0, 0, 0, 0.3);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 0.4rem;
		padding: 0.5rem;
		color: var(--text);
		resize: none;
		word-break: break-all;
		box-sizing: border-box;
	}

	.pill-error {
		border-color: rgba(251, 113, 133, 0.4);
		color: var(--danger, #fb7185);
	}

	.field-group {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
	}

	.field-label {
		font-size: 0.85rem;
		color: var(--muted, rgba(255, 255, 255, 0.55));
	}
</style>
