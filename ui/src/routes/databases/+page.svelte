<script lang="ts">
import {
	fetchTenants,
	createTenant,
	fetchTenantDatabases,
	assignDatabase,
	removeDatabase,
	type Tenant,
	type TenantDatabase,
} from '$lib/api';
import { onMount } from 'svelte';

let tenants: Tenant[] = $state([]);
let tenantDatabases: Record<string, TenantDatabase[]> = $state({});
let loading = $state(true);
let error: string | null = $state(null);
let statusMessage: string | null = $state(null);
let newTenantName = $state('');
let newDbNames: Record<string, string> = $state({});
let confirmDeleteDb: { tenantId: string; dbName: string } | null = $state(null);

async function loadAll() {
	loading = true;
	error = null;
	try {
		const loaded = await fetchTenants();
		tenants = loaded;
		const dbMap: Record<string, TenantDatabase[]> = {};
		await Promise.all(
			loaded.map(async (tenant) => {
				try {
					dbMap[tenant.id] = await fetchTenantDatabases(tenant.id);
				} catch {
					dbMap[tenant.id] = [];
				}
			}),
		);
		tenantDatabases = dbMap;
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to load tenants';
	} finally {
		loading = false;
	}
}

async function submitCreateTenant() {
	const name = newTenantName.trim();
	if (!name) return;
	error = null;
	statusMessage = null;
	try {
		await createTenant(name);
		newTenantName = '';
		statusMessage = `Tenant "${name}" created.`;
		await loadAll();
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to create tenant';
	}
}

async function submitAssignDb(tenantId: string) {
	const dbName = newDbNames[tenantId]?.trim();
	if (!dbName) return;
	error = null;
	statusMessage = null;
	try {
		await assignDatabase(tenantId, dbName);
		newDbNames = { ...newDbNames, [tenantId]: '' };
		statusMessage = `Database "${dbName}" assigned.`;
		await loadAll();
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to assign database';
	}
}

async function deleteDb(tenantId: string, dbName: string) {
	error = null;
	statusMessage = null;
	try {
		await removeDatabase(tenantId, dbName);
		confirmDeleteDb = null;
		statusMessage = `Database "${dbName}" removed.`;
		await loadAll();
	} catch (errorValue: unknown) {
		error = errorValue instanceof Error ? errorValue.message : 'Failed to remove database';
		confirmDeleteDb = null;
	}
}

onMount(() => void loadAll());
</script>

<div class="page-header">
	<div>
		<h1>Databases</h1>
		<p class="muted">Manage tenants and their database assignments.</p>
	</div>
</div>

{#if error}
	<p class="message error">{error}</p>
{/if}
{#if statusMessage}
	<p class="message success">{statusMessage}</p>
{/if}

<!-- Create Tenant form -->
<section class="panel">
	<div class="panel-header"><h2>Create Tenant</h2></div>
	<div class="panel-body stack">
		<label>
			<span>Tenant Name</span>
			<input bind:value={newTenantName} placeholder="my-org" />
		</label>
		<button class="primary" disabled={!newTenantName.trim()} onclick={submitCreateTenant}>
			Create Tenant
		</button>
	</div>
</section>

<!-- Tenant list -->
{#if loading}
	<p class="message">Loading tenants...</p>
{:else if tenants.length === 0}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">No tenants yet.</p>
		</div>
	</section>
{:else}
	{#each tenants as tenant}
		<section class="panel">
			<div class="panel-header">
				<h2>{tenant.name}</h2>
				<span class="pill muted">{tenant.id.slice(0, 8)}</span>
			</div>
			<div class="panel-body stack">
				<!-- databases list -->
				{#if (tenantDatabases[tenant.id]?.length ?? 0) > 0}
					<table>
						<thead>
							<tr>
								<th>Database</th>
								<th>Created</th>
								<th>Actions</th>
							</tr>
						</thead>
						<tbody>
							{#each tenantDatabases[tenant.id] as db}
								<tr>
									<td><code>{db.db_name}</code></td>
									<td class="muted">{new Date(db.created_at).toLocaleDateString()}</td>
									<td>
										{#if confirmDeleteDb?.tenantId === tenant.id && confirmDeleteDb?.dbName === db.db_name}
											<span class="muted" style="font-size:0.85rem">Remove?</span>
											<button class="danger" onclick={() => deleteDb(tenant.id, db.db_name)}>Confirm</button>
											<button onclick={() => (confirmDeleteDb = null)}>Cancel</button>
										{:else}
											<button class="danger" onclick={() => (confirmDeleteDb = { tenantId: tenant.id, dbName: db.db_name })}>Remove</button>
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{:else}
					<p class="muted">No databases assigned.</p>
				{/if}

				<!-- Assign database form -->
				<div style="display:flex; gap:0.5rem; align-items:center;">
					<input
						bind:value={newDbNames[tenant.id]}
						placeholder="database-name"
						style="flex:1"
					/>
					<button
						disabled={!newDbNames[tenant.id]?.trim()}
						onclick={() => submitAssignDb(tenant.id)}
					>
						Assign Database
					</button>
				</div>
			</div>
		</section>
	{/each}
{/if}

<style>
	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}
</style>
