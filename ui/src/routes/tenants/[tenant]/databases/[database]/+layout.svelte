<script lang="ts">
import { base } from '$app/paths';
import { page } from '$app/state';
import type { Snippet } from 'svelte';
import type { LayoutData } from './$types';

const { data, children }: { data: LayoutData; children: Snippet } = $props();

const dbHref = $derived(
	`${base}/tenants/${encodeURIComponent(data.tenant.db_name)}/databases/${encodeURIComponent(data.database.name)}`,
);
const collectionsHref = $derived(`${dbHref}/collections`);
const policiesHref = $derived(`${dbHref}/policies`);
const schemasHref = $derived(`${dbHref}/schemas`);
const intentsHref = $derived(`${dbHref}/intents`);
const auditHref = $derived(`${dbHref}/audit`);
const graphqlHref = $derived(`${dbHref}/graphql`);

function isActive(href: string): boolean {
	return page.url.pathname === href || page.url.pathname.startsWith(`${href}/`);
}
</script>

<div class="db-header panel">
	<div class="crumbs">
		<a class="crumb" href={`${base}/tenants`}>Tenants</a>
		<span class="sep">/</span>
		<a class="crumb" href={`${base}/tenants/${encodeURIComponent(data.tenant.db_name)}`}>
			{data.tenant.name}
		</a>
		<span class="sep">/</span>
		<span class="crumb current">{data.database.name}</span>
	</div>
	<nav class="subnav">
		<a class="subnav-link" class:active={isActive(collectionsHref)} href={collectionsHref}>
			Collections
		</a>
		<a class="subnav-link" class:active={isActive(policiesHref)} href={policiesHref}>
			Policies
		</a>
		<a class="subnav-link" class:active={isActive(schemasHref)} href={schemasHref}>Schemas</a>
		<a class="subnav-link" class:active={isActive(intentsHref)} href={intentsHref}>Intents</a>
		<a class="subnav-link" class:active={isActive(auditHref)} href={auditHref}>Audit Log</a>
		<a class="subnav-link" class:active={isActive(graphqlHref)} href={graphqlHref}>GraphQL</a>
	</nav>
</div>

{@render children()}

<style>
	.db-header {
		padding: 0.75rem 1rem;
		margin-bottom: 1rem;
	}

	.crumbs {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		font-size: 0.85rem;
		margin-bottom: 0.6rem;
	}

	.crumb {
		color: var(--muted);
		text-decoration: none;
	}

	.crumb:hover {
		color: var(--text);
	}

	.crumb.current {
		color: var(--text);
		font-weight: 600;
	}

	.sep {
		color: var(--muted);
		opacity: 0.5;
	}

	.subnav {
		display: flex;
		gap: 0.25rem;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
		padding-top: 0.5rem;
	}

	.subnav-link {
		padding: 0.4rem 0.8rem;
		border-radius: 0.5rem;
		text-decoration: none;
		font-size: 0.88rem;
		color: var(--muted);
		transition: color 120ms ease, background 120ms ease;
	}

	.subnav-link:hover {
		color: var(--text);
		background: rgba(255, 255, 255, 0.06);
	}

	.subnav-link.active {
		color: var(--text);
		background: rgba(125, 211, 252, 0.14);
	}
</style>
