<script lang="ts">
import { invalidateAll } from '$app/navigation';
import { base } from '$app/paths';
import { createTenantDatabase, deleteTenantDatabase } from '$lib/api';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();

const tenant = $derived(data.tenant);
const databases = $derived(data.databases);
const membersCount = $derived(data.membersCount);
const credentialsCount = $derived(data.credentialsCount);

const tenantHref = $derived(`${base}/tenants/${encodeURIComponent(tenant.db_name)}`);
const membersHref = $derived(`${tenantHref}/members`);
const credentialsHref = $derived(`${tenantHref}/credentials`);

let newDbName = $state('');
let creating = $state(false);
let createError = $state<string | null>(null);
let deletingName = $state<string | null>(null);
let error = $state<string | null>(null);

function databaseHref(name: string): string {
	return `${base}/tenants/${encodeURIComponent(tenant.db_name)}/databases/${encodeURIComponent(name)}`;
}

async function handleCreate() {
	if (!newDbName.trim()) return;
	creating = true;
	createError = null;
	try {
		await createTenantDatabase(tenant.id, newDbName.trim());
		newDbName = '';
		await invalidateAll();
	} catch (e: unknown) {
		createError = e instanceof Error ? e.message : 'Failed to create database';
	} finally {
		creating = false;
	}
}

async function handleDelete(name: string) {
	try {
		await deleteTenantDatabase(tenant.id, name);
		deletingName = null;
		await invalidateAll();
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to delete database';
		deletingName = null;
	}
}
</script>

<!-- Tenant metadata banner -->
<section class="panel tenant-banner">
	<div class="tenant-banner-inner">
		<div class="tenant-info">
			<h2 class="tenant-name">{tenant.name}</h2>
			<div class="tenant-meta-row">
				<span class="meta-chip">
					<span class="meta-label">ID</span>
					<code>{tenant.id}</code>
				</span>
				<span class="meta-chip">
					<span class="meta-label">Created</span>
					<span>{new Date(tenant.created_at).toLocaleDateString()}</span>
				</span>
			</div>
		</div>
		<div class="tenant-stats">
			<a class="stat-chip" href={membersHref}>
				<span class="stat-icon">👥</span>
				<div>
					<span class="stat-value">{membersCount}</span>
					<span class="stat-label">Members</span>
				</div>
			</a>
			<a class="stat-chip" href={credentialsHref}>
				<span class="stat-icon">🔑</span>
				<div>
					<span class="stat-value">{credentialsCount}</span>
					<span class="stat-label">Credentials</span>
				</div>
			</a>
		</div>
	</div>
</section>

<!-- Quick actions -->
<section class="panel quick-actions-panel">
	<div class="panel-header">
		<h2>Quick Actions</h2>
	</div>
	<div class="panel-body">
		<div class="quick-actions-grid">
			<a class="action-card primary" href={`${tenantHref}/databases?new=1`}>
				<span class="action-icon">🗄️</span>
				<div>
					<strong>Create Database</strong>
					<p class="muted">Add a new isolated data store to this tenant.</p>
				</div>
			</a>
			<a class="action-card" href={membersHref}>
				<span class="action-icon">👥</span>
				<div>
					<strong>Manage Members</strong>
					<p class="muted">Add or remove tenant members and manage roles.</p>
				</div>
			</a>
			<a class="action-card" href={credentialsHref}>
				<span class="action-icon">🔑</span>
				<div>
					<strong>Issue Credential</strong>
					<p class="muted">Create a JWT credential for a tenant user.</p>
				</div>
			</a>
		</div>
	</div>
</section>

<!-- Create database form -->
<section class="panel">
	<div class="panel-header">
		<h2>Create Database</h2>
	</div>
	<div class="panel-body">
		<form
			class="create-form"
			onsubmit={(e) => {
				e.preventDefault();
				void handleCreate();
			}}
		>
			<input
				class="name-input"
				type="text"
				placeholder="Database name (letters, digits, _ or -)"
				bind:value={newDbName}
				disabled={creating}
			/>
			<button type="submit" class="primary" disabled={creating || !newDbName.trim()}>
				{creating ? 'Creating…' : 'Create'}
			</button>
		</form>
		{#if createError}
			<p class="message error">{createError}</p>
		{/if}
	</div>
</section>

{#if error}
	<p class="message error">{error}</p>
{/if}

{#if databases.length === 0}
	<section class="panel">
		<div class="panel-body stack">
			<h2>No databases yet</h2>
			<p class="muted">Create one above to start managing collections and schemas.</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Databases</h2>
			<span class="pill">{databases.length}</span>
		</div>
		<div class="panel-body">
			<table>
				<thead>
					<tr>
						<th>Name</th>
						<th>Created</th>
						<th>Entities</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each databases as db}
						<tr>
							<td>
								<a href={databaseHref(db.name)}>
									<strong>{db.name}</strong>
								</a>
							</td>
							<td class="muted">{new Date(db.created_at_ms).toLocaleDateString()}</td>
							<td class="muted">{db.entity_count ?? '—'}</td>
							<td>
								<div class="actions">
									<a class="button-link" href={databaseHref(db.name)}>Open</a>
									{#if deletingName === db.name}
										<span class="muted" style="font-size:0.85rem">Delete {db.name}?</span>
										<button class="danger" onclick={() => void handleDelete(db.name)}>
											Confirm
										</button>
										<button onclick={() => (deletingName = null)}>Cancel</button>
									{:else}
										<button class="danger" onclick={() => (deletingName = db.name)}>Delete</button>
									{/if}
								</div>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
{/if}

<style>
	.create-form {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}

	.name-input {
		flex: 1;
		max-width: 24rem;
	}

	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}

	/* ── Tenant banner ─────────────────────────────────────────────── */

	.tenant-banner {
		margin-bottom: 1rem;
	}

	.tenant-banner-inner {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1.5rem;
		padding: 1.25rem;
		flex-wrap: wrap;
	}

	.tenant-name {
		margin: 0 0 0.5rem;
		font-size: 1.25rem;
	}

	.tenant-meta-row {
		display: flex;
		gap: 0.75rem;
		flex-wrap: wrap;
	}

	.meta-chip {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		padding: 0.2rem 0.6rem;
		border-radius: 0.5rem;
		background: rgba(125, 211, 252, 0.08);
		border: 1px solid rgba(125, 211, 252, 0.18);
		font-size: 0.82rem;
	}

	.meta-label {
		color: var(--muted);
		font-weight: 600;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.meta-chip code {
		font-family: monospace;
		font-size: 0.82em;
	}

	.tenant-stats {
		display: flex;
		gap: 0.75rem;
	}

	.stat-chip {
		display: inline-flex;
		align-items: center;
		gap: 0.6rem;
		padding: 0.5rem 0.85rem;
		border-radius: 0.75rem;
		border: 1px solid var(--border);
		background: var(--panel-strong);
		text-decoration: none;
		color: var(--text);
		transition: border-color 120ms ease, background 120ms ease;
	}

	.stat-chip:hover {
		border-color: var(--accent-strong);
		background: #253041;
	}

	.stat-icon {
		font-size: 1.2rem;
	}

	.stat-value {
		display: block;
		font-size: 1.15rem;
		font-weight: 700;
		line-height: 1.1;
	}

	.stat-label {
		display: block;
		font-size: 0.72rem;
		color: var(--muted);
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	/* ── Quick actions ─────────────────────────────────────────────── */

	.quick-actions-panel {
		margin-bottom: 1rem;
	}

	.quick-actions-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr));
		gap: 0.75rem;
	}

	.action-card {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 1rem;
		border-radius: 0.75rem;
		border: 1px solid var(--border);
		background: var(--panel-strong);
		text-decoration: none;
		color: var(--text);
		transition: border-color 120ms ease, background 120ms ease, transform 120ms ease;
	}

	.action-card:hover {
		border-color: var(--accent-strong);
		background: #253041;
		transform: translateY(-1px);
	}

	.action-card.primary {
		border-color: rgba(14, 165, 233, 0.35);
		background: rgba(14, 165, 233, 0.08);
	}

	.action-card.primary:hover {
		border-color: rgba(14, 165, 233, 0.6);
		background: rgba(14, 165, 233, 0.14);
	}

	.action-icon {
		font-size: 1.5rem;
		flex-shrink: 0;
	}

	.action-card strong {
		display: block;
		font-size: 0.95rem;
		margin-bottom: 0.15rem;
	}

	.action-card p {
		margin: 0;
		font-size: 0.82rem;
	}
</style>
