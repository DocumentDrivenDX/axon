<script lang="ts">
import '../app.css';

import { base } from '$app/paths';
import { type AuthState, type HealthStatus, fetchAuthMe, fetchHealth } from '$lib/api';
import type { Snippet } from 'svelte';
import { onMount } from 'svelte';

const { children }: { children: Snippet } = $props();

let health: HealthStatus | null = $state(null);
let healthError: string | null = $state(null);
let authState: AuthState = $state({ status: 'loading' } as AuthState);
const homeHref = `${base}/`;
const collectionsHref = `${base}/collections`;
const schemasHref = `${base}/schemas`;
const auditHref = `${base}/audit`;
const databasesHref = `${base}/databases`;

const isGuest = $derived(
	authState.status === 'authenticated' && authState.identity.actor === 'guest',
);
const isReadOnly = $derived(
	authState.status === 'authenticated' && authState.identity.role === 'read',
);

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

onMount(() => {
	void refreshHealth();
	void loadAuth();
	const timer = window.setInterval(() => {
		void refreshHealth();
	}, 15_000);

	return () => window.clearInterval(timer);
});
</script>

<div class="shell">
	<aside class="sidebar panel">
		<div class="brand">
			<a href={homeHref}>Axon Admin</a>
			<p class="muted">Browser console for live collections, schemas, and audit data.</p>
		</div>

		<section class="identity-bar">
			{#if authState.status === 'loading'}
				<p class="muted">Checking identity...</p>
			{:else if authState.status === 'unauthenticated'}
				<div class="message warning">
					<strong>Not authenticated</strong>
					<p>Connect via Tailscale to access this server.</p>
				</div>
			{:else}
				<div class="identity-info">
					<span class="identity-actor">{authState.identity.actor}</span>
					<span class="pill role-{authState.identity.role}">{authState.identity.role}</span>
					{#if isGuest && isReadOnly}
						<span class="pill guest-badge">Guest (read-only)</span>
					{:else if isGuest}
						<span class="pill guest-badge">Guest</span>
					{/if}
				</div>
			{/if}
		</section>

		<nav class="stack">
			<a class="button-link" href={collectionsHref}>Collections</a>
			<a class="button-link" href={schemasHref}>Schemas</a>
			<a class="button-link" href={auditHref}>Audit Log</a>
			<a class="button-link" href={databasesHref}>Databases</a>
		</nav>

		<section class="health panel">
			<div class="panel-header">
				<h2>Health</h2>
				<span class="pill">{health?.status ?? 'checking'}</span>
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
					<p class="muted">Polling `/health`...</p>
				{/if}
			</div>
		</section>
	</aside>

	<main class="content">
		{@render children()}
	</main>
</div>

<style>
	.shell {
		display: grid;
		grid-template-columns: 18rem minmax(0, 1fr);
		gap: 1rem;
		min-height: 100vh;
		padding: 1rem;
	}

	.sidebar {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		padding: 1.1rem;
	}

	.brand a {
		font-size: 1.35rem;
		font-weight: 700;
		letter-spacing: 0.02em;
		text-decoration: none;
	}

	.brand p {
		margin: 0.5rem 0 0;
	}

	.identity-bar {
		padding: 0.5rem 0;
	}

	.identity-info {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.identity-actor {
		font-weight: 600;
		font-size: 0.9rem;
	}

	.guest-badge {
		font-size: 0.75rem;
		opacity: 0.8;
	}

	.message.warning {
		padding: 0.5rem 0.75rem;
		border-radius: 0.375rem;
		background: var(--color-warning-bg, #fef3c7);
		color: var(--color-warning-text, #92400e);
		font-size: 0.85rem;
	}

	.message.warning p {
		margin: 0.25rem 0 0;
	}

	.health h2 {
		margin: 0;
		font-size: 1rem;
	}

	.content {
		padding: 1rem 0;
	}

	@media (max-width: 1024px) {
		.shell {
			grid-template-columns: 1fr;
		}
	}
</style>
