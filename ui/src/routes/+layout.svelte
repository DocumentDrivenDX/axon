<script lang="ts">
import '../app.css';

import { base } from '$app/paths';
import { type HealthStatus, fetchHealth } from '$lib/api';
import { onMount } from 'svelte';

let health: HealthStatus | null = null;
let healthError: string | null = null;
const homeHref = `${base}/`;
const schemasHref = `${base}/schemas`;
const auditHref = `${base}/audit`;

async function refreshHealth() {
	try {
		health = await fetchHealth();
		healthError = null;
	} catch (errorValue: unknown) {
		healthError = errorValue instanceof Error ? errorValue.message : 'Failed to reach /health';
	}
}

onMount(() => {
	void refreshHealth();
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

		<nav class="stack">
			<a class="button-link" href={homeHref}>Collections</a>
			<a class="button-link" href={schemasHref}>Schemas</a>
			<a class="button-link" href={auditHref}>Audit Log</a>
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
					<p class="muted">Polling `/health`…</p>
				{/if}
			</div>
		</section>
	</aside>

	<main class="content">
		<slot />
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
