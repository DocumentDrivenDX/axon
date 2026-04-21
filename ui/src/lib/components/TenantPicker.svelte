<script lang="ts">
import { goto } from '$app/navigation';
import { base } from '$app/paths';
import { type Tenant, fetchTenants } from '$lib/api';

interface Props {
	currentTenantDbName: string | null;
	currentDatabaseName: string | null;
}

const { currentTenantDbName, currentDatabaseName }: Props = $props();

let tenants = $state<Tenant[]>([]);
let loading = $state(false);
let showDropdown = $state(false);
let error = $state<string | null>(null);

// Current tenant name for display
const currentTenantName = $derived(
	currentTenantDbName
		? (tenants.find((t) => t.db_name === currentTenantDbName)?.name ?? currentTenantDbName)
		: null,
);

async function loadTenants() {
	loading = true;
	error = null;
	try {
		tenants = await fetchTenants();
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to load tenants';
	} finally {
		loading = false;
	}
}

function tenantHref(tenant: Tenant): string {
	return `${base}/tenants/${encodeURIComponent(tenant.db_name)}`;
}

function isCurrentTenant(t: Tenant): boolean {
	return currentTenantDbName !== null && t.db_name === currentTenantDbName;
}

function closeDropdown() {
	showDropdown = false;
}

async function selectTenant(tenant: Tenant) {
	closeDropdown();
	await goto(tenantHref(tenant));
}

function openDropdown() {
	if (showDropdown) {
		closeDropdown();
		return;
	}
	showDropdown = true;
	void loadTenants();
}

// Close on outside click
function handleDocumentClick(e: MouseEvent) {
	const target = e.target as HTMLElement;
	if (!target.closest('.tenant-picker')) {
		closeDropdown();
	}
}

$effect(() => {
	if (showDropdown) {
		document.addEventListener('click', handleDocumentClick);
	}
	return () => {
		document.removeEventListener('click', handleDocumentClick);
	};
});
</script>

<div class="tenant-picker">
	<button class="picker-trigger" onclick={openDropdown}>
		<span class="picker-icon">
			{#if currentTenantName}
				<span class="current-tenant">
					{currentTenantName}
					{#if currentDatabaseName}
						<span class="current-db">· {currentDatabaseName}</span>
					{/if}
				</span>
			{:else}
				<span class="muted">Select tenant</span>
			{/if}
		</span>
		<span class="chevron">▾</span>
	</button>

	{#if showDropdown}
		<div class="picker-dropdown">
			{#if error}
				<p class="message error">{error}</p>
			{:else if loading}
				<p class="muted">Loading tenants…</p>
			{:else}
				<div class="dropdown-section">
					<div class="dropdown-label">Tenants</div>
					{#each tenants as tenant}
						<button
							class="dropdown-item"
							class:selected={isCurrentTenant(tenant)}
							onclick={() => void selectTenant(tenant)}
						>
							<span class="item-name">{tenant.name}</span>
							<span class="item-slug">{tenant.db_name}</span>
							{#if isCurrentTenant(tenant)}
								<span class="item-check">✓</span>
							{/if}
						</button>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>

<style>
	.tenant-picker {
		position: relative;
	}

	.picker-trigger {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		padding: 0.4rem 0.75rem;
		border: 1px solid var(--border);
		border-radius: 0.6rem;
		background: var(--panel-strong);
		color: var(--text);
		font-size: 0.88rem;
		cursor: pointer;
		transition:
			border-color 120ms ease,
			background 120ms ease;
		white-space: nowrap;
		font-family: inherit;
	}

	.picker-trigger:hover {
		border-color: var(--accent-strong);
		background: #253041;
	}

	.picker-icon {
		display: flex;
		align-items: center;
		gap: 0.35rem;
	}

	.current-tenant {
		font-weight: 600;
	}

	.current-db {
		color: var(--accent);
		font-weight: 400;
	}

	.chevron {
		font-size: 0.75rem;
		opacity: 0.6;
	}

	.picker-dropdown {
		position: absolute;
		top: calc(100% + 0.35rem);
		right: 0;
		z-index: 200;
		min-width: 22rem;
		max-width: 30rem;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: 0.75rem;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
		padding: 0.5rem;
		max-height: 32rem;
		overflow-y: auto;
	}

	.dropdown-section {
		display: flex;
		flex-direction: column;
		gap: 0.15rem;
	}

	.dropdown-label {
		font-size: 0.72rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--muted);
		padding: 0.35rem 0.6rem;
	}

	.dropdown-item {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		width: 100%;
		padding: 0.45rem 0.6rem;
		border: none;
		border-radius: 0.4rem;
		background: none;
		color: var(--text);
		font-size: 0.88rem;
		cursor: pointer;
		text-align: left;
		font-family: inherit;
		transition: background 80ms ease;
	}

	.dropdown-item:hover {
		background: rgba(255, 255, 255, 0.06);
	}

	.dropdown-item.selected {
		background: rgba(125, 211, 252, 0.12);
	}

	.item-name {
		flex: 1;
		font-weight: 500;
	}

	.item-slug {
		font-family: monospace;
		font-size: 0.78rem;
		color: var(--muted);
	}

	.item-check {
		color: var(--accent);
		font-size: 0.85rem;
	}

	.muted {
		color: var(--muted);
		font-size: 0.85rem;
	}

	.message {
		padding: 0.6rem 0.75rem;
		border-radius: 0.5rem;
		font-size: 0.85rem;
	}

	.message.error {
		background: rgba(251, 113, 133, 0.1);
		border: 1px solid rgba(251, 113, 133, 0.3);
		color: #fecdd3;
	}
</style>
