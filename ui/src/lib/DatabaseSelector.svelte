<script lang="ts">
import { type TenantDatabase, fetchTenantDatabases } from '$lib/api';
import { getSelectedDatabase, getSelectedTenant, setSelectedDatabase } from '$lib/stores.svelte';

let databases = $state<TenantDatabase[]>([]);
let loading = $state(false);
let lastTenantId: string | null = null;

$effect(() => {
	const tenant = getSelectedTenant();
	if (!tenant) {
		databases = [];
		setSelectedDatabase(null);
		lastTenantId = null;
		return;
	}
	// Reset database selection when the tenant changes.
	if (tenant.id !== lastTenantId) {
		setSelectedDatabase(null);
		lastTenantId = tenant.id;
	}
	loading = true;
	fetchTenantDatabases(tenant.id).then((dbs) => {
		databases = dbs;
		loading = false;
		// Auto-select the first database if none selected.
		if (!getSelectedDatabase() && dbs.length > 0) {
			setSelectedDatabase(dbs[0] ?? null);
		}
	});
});
</script>

{#if loading}
	<span class="db-label muted">···</span>
{:else if databases.length > 0}
	<div class="db-selector">
		<label for="database-select" class="db-label">Database</label>
		<select
			id="database-select"
			class="db-select"
			value={getSelectedDatabase()?.name ?? ''}
			onchange={(e) => {
				const name = (e.target as HTMLSelectElement).value;
				const db = databases.find((d) => d.name === name) ?? null;
				setSelectedDatabase(db);
			}}
		>
			{#each databases as db}
				<option value={db.name}>{db.name}</option>
			{/each}
		</select>
	</div>
{/if}

<style>
	.db-selector {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.db-label {
		font-size: 0.8rem;
		color: var(--muted);
		white-space: nowrap;
	}

	.db-select {
		background: var(--surface, #1e1e2e);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 0.5rem;
		color: var(--text);
		font-size: 0.875rem;
		padding: 0.3rem 0.6rem;
		cursor: pointer;
	}

	.db-select:focus {
		outline: none;
		border-color: var(--accent);
	}
</style>
