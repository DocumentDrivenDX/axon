<script lang="ts">
import { base } from '$app/paths';
import { page } from '$app/state';
import type { Snippet } from 'svelte';
import type { LayoutData } from './$types';

const { data, children }: { data: LayoutData; children: Snippet } = $props();

const tenantHref = $derived(`${base}/tenants/${encodeURIComponent(data.tenant.db_name)}`);
const membersHref = $derived(`${tenantHref}/members`);
const credentialsHref = $derived(`${tenantHref}/credentials`);

function isActive(href: string): boolean {
	return page.url.pathname === href || page.url.pathname.startsWith(`${href}/`);
}
</script>

<div class="tenant-header panel">
	<div class="crumbs">
		<a class="crumb" href={`${base}/tenants`}>Tenants</a>
		<span class="sep">/</span>
		<span class="crumb current">{data.tenant.name}</span>
	</div>
	<div class="tenant-meta">
		<code>{data.tenant.db_name}</code>
		<span class="muted">created {new Date(data.tenant.created_at).toLocaleDateString()}</span>
	</div>
	<nav class="subnav">
		<a class="subnav-link" class:active={isActive(tenantHref) && !isActive(membersHref) && !isActive(credentialsHref)} href={tenantHref}>
			Databases
		</a>
		<a class="subnav-link" class:active={isActive(membersHref)} href={membersHref}>Members</a>
		<a class="subnav-link" class:active={isActive(credentialsHref)} href={credentialsHref}>
			Credentials
		</a>
	</nav>
</div>

{@render children()}

<style>
	.tenant-header {
		padding: 0.75rem 1rem;
		margin-bottom: 1rem;
	}

	.crumbs {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		font-size: 0.85rem;
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

	.tenant-meta {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		margin: 0.5rem 0 0.75rem;
		font-size: 0.85rem;
	}

	.tenant-meta code {
		font-family: monospace;
		font-size: 0.85em;
		background: rgba(255, 255, 255, 0.06);
		padding: 0.1em 0.4em;
		border-radius: 0.25rem;
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
