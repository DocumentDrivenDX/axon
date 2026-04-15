<script lang="ts">
import { base } from '$app/paths';
import { goto } from '$app/navigation';
import { type Tenant, createTenant, deleteTenant, fetchTenants } from '$lib/api';

let tenants = $state<Tenant[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
let newName = $state('');
let creating = $state(false);
let createError = $state<string | null>(null);
let deletingId = $state<string | null>(null);

function tenantHref(tenant: Tenant): string {
	return `${base}/tenants/${encodeURIComponent(tenant.db_name)}`;
}

async function loadTenants() {
	loading = true;
	try {
		tenants = await fetchTenants();
		error = null;
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to load tenants';
	} finally {
		loading = false;
	}
}

async function handleCreate() {
	if (!newName.trim()) return;
	creating = true;
	createError = null;
	try {
		const tenant = await createTenant(newName.trim());
		newName = '';
		await loadTenants();
		// Navigate into the new tenant.
		await goto(tenantHref(tenant));
	} catch (e: unknown) {
		createError = e instanceof Error ? e.message : 'Failed to create tenant';
	} finally {
		creating = false;
	}
}

async function handleDelete(id: string) {
	deletingId = id;
	try {
		await deleteTenant(id);
		await loadTenants();
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to delete tenant';
	} finally {
		deletingId = null;
	}
}

$effect(() => {
	void loadTenants();
});
</script>

<div class="page-header">
	<div>
		<h1>Tenants</h1>
		<p class="muted">
			Each tenant owns one or more isolated databases. Click a tenant to manage its
			databases, members, and credentials.
		</p>
	</div>
</div>

<section class="panel">
	<div class="panel-header">
		<h2>Create Tenant</h2>
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
				placeholder="Tenant name"
				bind:value={newName}
				disabled={creating}
			/>
			<button type="submit" class="primary" disabled={creating || !newName.trim()}>
				{creating ? 'Creating…' : 'Create'}
			</button>
		</form>
		{#if createError}
			<p class="message error">{createError}</p>
		{/if}
	</div>
</section>

{#if loading}
	<p class="message">Loading tenants…</p>
{:else if error}
	<p class="message error">{error}</p>
{:else if tenants.length === 0}
	<section class="panel">
		<div class="panel-body stack">
			<h2>No tenants yet</h2>
			<p class="muted">Create a tenant above to start managing isolated databases.</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Tenants</h2>
			<span class="pill">{tenants.length}</span>
		</div>
		<div class="panel-body">
			<table>
				<thead>
					<tr>
						<th>Name</th>
						<th>Database slug</th>
						<th>Created</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each tenants as tenant}
						<tr>
							<td>
								<a href={tenantHref(tenant)}>
									<strong>{tenant.name}</strong>
								</a>
							</td>
							<td><code>{tenant.db_name}</code></td>
							<td class="muted">{new Date(tenant.created_at).toLocaleDateString()}</td>
							<td>
								<div class="actions">
									<a class="button-link" href={tenantHref(tenant)}>Open</a>
									{#if deletingId === tenant.id}
										<span class="muted" style="font-size:0.85rem">Delete {tenant.name}?</span>
										<button class="danger" onclick={() => void handleDelete(tenant.id)}>
											Confirm
										</button>
										<button onclick={() => (deletingId = null)}>Cancel</button>
									{:else}
										<button class="danger" onclick={() => (deletingId = tenant.id)}>
											Delete
										</button>
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
		max-width: 20rem;
	}

	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}

	code {
		font-family: monospace;
		font-size: 0.85em;
		background: rgba(255, 255, 255, 0.06);
		padding: 0.1em 0.35em;
		border-radius: 0.25rem;
	}
</style>
