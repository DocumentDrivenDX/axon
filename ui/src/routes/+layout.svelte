<script lang="ts">
import '../app.css';

import { base } from '$app/paths';
import { page } from '$app/state';
import { type AuthState, fetchAuthMe } from '$lib/api';
// biome-ignore lint/correctness/noUnusedImports: Used in template as TenantPicker component.
import TenantPicker from '$lib/components/TenantPicker.svelte';
import type { Snippet } from 'svelte';
import { onMount } from 'svelte';

const { children }: { children: Snippet } = $props();

let authState: AuthState = $state({ status: 'loading' } as AuthState);

const homeHref = `${base}/`;
const tenantsHref = `${base}/tenants`;
const usersHref = `${base}/users`;

// Derive current tenant and database from the URL path.
// Paths like /tenants/{tenant}/databases/{database}/... → extract both.
const urlSegments = $derived(() => page.url.pathname.split('/').filter(Boolean));
let currentTenantDbName: string | null = $state(null);
let currentDatabaseName: string | null = $state(null);

$effect(() => {
	const segs = urlSegments();
	const tenantIdx = segs.indexOf('tenants');
	if (tenantIdx >= 0 && tenantIdx + 1 < segs.length) {
		currentTenantDbName = segs[tenantIdx + 1] ?? null;
	}
	if (currentTenantDbName) {
		const dbIdx = segs.indexOf('databases');
		if (dbIdx >= 0 && dbIdx + 1 < segs.length) {
			currentDatabaseName = segs[dbIdx + 1] ?? null;
		} else {
			currentDatabaseName = null;
		}
	} else {
		currentDatabaseName = null;
	}
});

const isGuest = $derived(
	authState.status === 'authenticated' && authState.identity.actor === 'guest',
);
const isReadOnly = $derived(
	authState.status === 'authenticated' && authState.identity.role === 'read',
);

const tenantHref = $derived(
	currentTenantDbName ? `${base}/tenants/${encodeURIComponent(currentTenantDbName)}` : null,
);
const membersHref = $derived(tenantHref ? `${tenantHref}/members` : null);
const credentialsHref = $derived(tenantHref ? `${tenantHref}/credentials` : null);
const databaseHref = $derived(
	tenantHref && currentDatabaseName
		? `${tenantHref}/databases/${encodeURIComponent(currentDatabaseName)}`
		: null,
);
const collectionsHref = $derived(databaseHref ? `${databaseHref}/collections` : null);
const policiesHref = $derived(databaseHref ? `${databaseHref}/policies` : null);
const schemasHref = $derived(databaseHref ? `${databaseHref}/schemas` : null);
const auditHref = $derived(databaseHref ? `${databaseHref}/audit` : null);
const graphqlHref = $derived(databaseHref ? `${databaseHref}/graphql` : null);

function isActive(href: string | null): boolean {
	if (!href) return false;
	return page.url.pathname === href || page.url.pathname.startsWith(`${href}/`);
}

async function loadAuth() {
	try {
		const identity = await fetchAuthMe();
		authState = { status: 'authenticated', identity };
	} catch {
		authState = { status: 'unauthenticated' };
	}
}

onMount(() => {
	void loadAuth();
});
</script>

<div class="shell">
	<header class="topnav panel">
		<div class="topnav-left">
			<a class="brand-link" href={homeHref}>Axon</a>
			<nav class="topnav-links">
				<a class="nav-link" href={tenantsHref}>Tenants</a>
				<a class="nav-link" href={usersHref}>Users</a>
			</nav>
			<TenantPicker
				currentTenantDbName={currentTenantDbName}
				currentDatabaseName={currentDatabaseName}
			/>
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
			<aside class="sidebar panel" aria-label="Workspace navigation">
				<div class="panel-header">
					<h2>Workspace</h2>
				</div>
				<nav class="sidebar-nav">
					<div class="nav-section">
						<a class="side-link" class:active={isActive(tenantsHref)} href={tenantsHref}>
							Tenants
						</a>
						<a class="side-link" class:active={isActive(usersHref)} href={usersHref}>Users</a>
					</div>

					{#if currentTenantDbName && tenantHref}
						<div class="nav-section">
							<div class="nav-label">Tenant</div>
							<div class="scope-name" title={currentTenantDbName}>{currentTenantDbName}</div>
							<a
								class="side-link"
								class:active={isActive(tenantHref) && !isActive(membersHref) && !isActive(credentialsHref)}
								href={tenantHref}
							>
								Databases
							</a>
							<a class="side-link" class:active={isActive(membersHref)} href={membersHref}>Members</a>
							<a class="side-link" class:active={isActive(credentialsHref)} href={credentialsHref}>
								Credentials
							</a>
						</div>
					{/if}

					{#if currentDatabaseName && databaseHref}
						<div class="nav-section">
							<div class="nav-label">Database</div>
							<div class="scope-name" title={currentDatabaseName}>{currentDatabaseName}</div>
							<a
								class="side-link"
								class:active={page.url.pathname === databaseHref}
								href={databaseHref}
							>
								Overview
							</a>
							<a class="side-link" class:active={isActive(collectionsHref)} href={collectionsHref}>
								Collections
							</a>
							<a class="side-link" class:active={isActive(policiesHref)} href={policiesHref}>
								Policies
							</a>
							<a class="side-link" class:active={isActive(schemasHref)} href={schemasHref}>Schemas</a>
							<a class="side-link" class:active={isActive(auditHref)} href={auditHref}>Audit Log</a>
							<a class="side-link" class:active={isActive(graphqlHref)} href={graphqlHref}>GraphQL</a>
						</div>
					{/if}
				</nav>
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
		transition:
			color 120ms ease,
			background 120ms ease;
	}

	.nav-link:hover {
		color: var(--text);
		background: rgba(255, 255, 255, 0.06);
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

		.sidebar-nav {
			display: flex;
			flex-direction: column;
			gap: 0.9rem;
			padding: 0.75rem;
		}

		.nav-section {
			display: flex;
			flex-direction: column;
			gap: 0.25rem;
		}

		.nav-label {
			margin: 0.25rem 0.25rem 0;
			color: var(--muted);
			font-size: 0.72rem;
			font-weight: 700;
			text-transform: uppercase;
		}

		.scope-name {
			min-width: 0;
			margin: 0 0.25rem 0.35rem;
			overflow: hidden;
			color: var(--text);
			font-family: monospace;
			font-size: 0.82rem;
			text-overflow: ellipsis;
			white-space: nowrap;
		}

		.side-link {
			display: block;
			padding: 0.45rem 0.65rem;
			border-radius: 0.5rem;
			color: var(--muted);
			font-size: 0.9rem;
			text-decoration: none;
			transition:
				color 120ms ease,
				background 120ms ease;
		}

		.side-link:hover {
			background: rgba(255, 255, 255, 0.06);
			color: var(--text);
		}

		.side-link.active {
			background: rgba(125, 211, 252, 0.14);
			color: var(--text);
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
	}
</style>
