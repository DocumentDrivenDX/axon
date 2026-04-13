<script lang="ts">
import '../app.css';

import { base } from '$app/paths';
import { type AuthState, type HealthStatus, type Tenant, fetchAuthMe, fetchHealth, fetchTenants } from '$lib/api';
import { getSelectedTenant, setSelectedTenant } from '$lib/stores.svelte';
import type { Snippet } from 'svelte';
import { onMount } from 'svelte';

const { children }: { children: Snippet } = $props();

let health: HealthStatus | null = $state(null);
let healthError: string | null = $state(null);
let authState: AuthState = $state({ status: 'loading' } as AuthState);
let tenants: Tenant[] = $state([]);

const homeHref = `${base}/`;
const collectionsHref = `${base}/collections`;
const schemasHref = `${base}/schemas`;
const auditHref = `${base}/audit`;
const tenantsHref = `${base}/tenants`;

const isGuest = $derived(
	authState.status === 'authenticated' && authState.identity.actor === 'guest',
);
const isReadOnly = $derived(
	authState.status === 'authenticated' && authState.identity.role === 'read',
);

const selectedTenant = $derived(getSelectedTenant());

async function refreshHealth() {
	try {
		health = await fetchHealth();
		healthError = null;
	} catch (errorValue: unknown) {
		healthError = errorValue instanceof Error ? errorValue.message : 'Failed to reach /health';
	}
}

async function loadAuth() {
	try {
		const identity = await fetchAuthMe();
		authState = { status: 'authenticated', identity };
	} catch {
		authState = { status: 'unauthenticated' };
	}
}

async function loadTenants() {
	try {
		tenants = await fetchTenants();
		// Auto-select first tenant if none selected and tenants exist.
		if (!getSelectedTenant() && tenants.length > 0) {
			setSelectedTenant(tenants[0] ?? null);
		}
	} catch {
		// Tenant list is a best-effort; silently ignore (e.g. no auth).
	}
}

function handleTenantChange(event: Event) {
	const id = (event.target as HTMLSelectElement).value;
	const found = tenants.find((t) => t.id === id) ?? null;
	setSelectedTenant(found);
}

onMount(() => {
	void refreshHealth();
	void loadAuth();
	void loadTenants();
	const timer = window.setInterval(() => {
		void refreshHealth();
	}, 15_000);

	return () => window.clearInterval(timer);
});
</script>

<div class="shell">
	<header class="topnav panel">
		<div class="topnav-left">
			<a class="brand-link" href={homeHref}>Axon</a>
			<nav class="topnav-links">
				<a class="nav-link" href={collectionsHref}>Collections</a>
				<a class="nav-link" href={schemasHref}>Schemas</a>
				<a class="nav-link" href={auditHref}>Audit Log</a>
				<a class="nav-link" href={tenantsHref}>Tenants</a>
			</nav>
		</div>

		<div class="topnav-center">
			{#if tenants.length > 0}
				<div class="tenant-selector">
					<label for="tenant-select" class="tenant-label">Tenant</label>
					<select
						id="tenant-select"
						class="tenant-select"
						value={selectedTenant?.id ?? ''}
						onchange={handleTenantChange}
					>
						{#each tenants as tenant}
							<option value={tenant.id}>{tenant.name}</option>
						{/each}
					</select>
				</div>
			{/if}
		</div>

		<div class="topnav-right">
			{#if authState.status === 'loading'}
				<span class="muted user-widget">···</span>
			{:else if authState.status === 'unauthenticated'}
				<span class="pill warning-pill">Not authenticated — connect via Tailscale</span>
			{:else}
				<div class="user-widget">
					<span class="user-actor">{authState.identity.actor}</span>
					<span class="pill role-{authState.identity.role}">{authState.identity.role}</span>
					{#if isGuest && isReadOnly}
						<span class="pill guest-badge">guest · read-only</span>
					{:else if isGuest}
						<span class="pill guest-badge">guest</span>
					{/if}
				</div>
			{/if}
		</div>
	</header>

	<div class="body">
		<aside class="sidebar panel">
			<div class="panel-header">
				<h2>Health</h2>
				<span class="pill {healthError ? 'pill-error' : ''}">{healthError ? 'error' : (health?.status ?? 'checking')}</span>
			</div>
			<div class="panel-body stack">
				{#if healthError}
					<p class="message error">{healthError}</p>
				{:else if health}
					<div>
						<strong>Version</strong>
						<p class="muted">{health.version}</p>
					</div>
					<div>
						<strong>Uptime</strong>
						<p class="muted">{health.uptime_seconds}s</p>
					</div>
					<div>
						<strong>Backend</strong>
						<p class="muted">{health.backing_store.backend}</p>
					</div>
					<div>
						<strong>Default Namespace</strong>
						<p class="muted">{health.default_namespace}</p>
					</div>
				{:else}
					<p class="muted">Polling…</p>
				{/if}
			</div>
		</aside>

		<main class="content">
			{@render children()}
		</main>
	</div>
</div>

<style>
	.shell {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		min-height: 100vh;
		padding: 1rem;
	}

	/* ── Top nav ─────────────────────────────────────────────────────────── */

	.topnav {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
		padding: 0.6rem 1.1rem;
	}

	.topnav-left {
		display: flex;
		align-items: center;
		gap: 1.5rem;
	}

	.brand-link {
		font-size: 1.2rem;
		font-weight: 700;
		letter-spacing: 0.04em;
		text-decoration: none;
		color: var(--accent);
		white-space: nowrap;
	}

	.topnav-links {
		display: flex;
		align-items: center;
		gap: 0.25rem;
	}

	.nav-link {
		padding: 0.4rem 0.75rem;
		border-radius: 0.6rem;
		text-decoration: none;
		font-size: 0.9rem;
		color: var(--muted);
		transition: color 120ms ease, background 120ms ease;
	}

	.nav-link:hover {
		color: var(--text);
		background: rgba(255, 255, 255, 0.06);
	}

	.topnav-center {
		flex: 1;
		display: flex;
		justify-content: center;
	}

	.tenant-selector {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.tenant-label {
		font-size: 0.8rem;
		color: var(--muted);
		white-space: nowrap;
	}

	.tenant-select {
		background: var(--surface, #1e1e2e);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 0.5rem;
		color: var(--text);
		font-size: 0.875rem;
		padding: 0.3rem 0.6rem;
		cursor: pointer;
	}

	.tenant-select:focus {
		outline: none;
		border-color: var(--accent);
	}

	.topnav-right {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		flex-shrink: 0;
	}

	.user-widget {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.user-actor {
		font-size: 0.875rem;
		font-weight: 500;
		color: var(--text);
	}

	.guest-badge {
		font-size: 0.75rem;
		opacity: 0.8;
	}

	.warning-pill {
		border-color: rgba(251, 191, 36, 0.4);
		color: #fbbf24;
		font-size: 0.82rem;
	}

	.pill-error {
		border-color: rgba(251, 113, 133, 0.4);
		color: var(--danger);
	}

	/* ── Body grid ───────────────────────────────────────────────────────── */

	.body {
		display: grid;
		grid-template-columns: 16rem minmax(0, 1fr);
		gap: 1rem;
		flex: 1;
	}

	.sidebar {
		display: flex;
		flex-direction: column;
		align-self: start;
	}

	.sidebar h2 {
		margin: 0;
		font-size: 1rem;
	}

	.content {
		padding: 0;
		min-width: 0;
	}

	@media (max-width: 900px) {
		.body {
			grid-template-columns: 1fr;
		}

		.topnav-links {
			display: none;
		}

		.topnav-center {
			display: none;
		}
	}
</style>
