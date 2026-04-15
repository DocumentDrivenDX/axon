<script lang="ts">
import { base } from '$app/paths';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();

const scope = $derived(
	`${encodeURIComponent(data.tenant.db_name)}/databases/${encodeURIComponent(data.database.name)}`,
);
const collectionsHref = $derived(`${base}/tenants/${scope}/collections`);
const schemasHref = $derived(`${base}/tenants/${scope}/schemas`);
const auditHref = $derived(`${base}/tenants/${scope}/audit`);
</script>

<div class="page-header">
	<div>
		<h1>{data.database.name}</h1>
		<p class="muted">
			Database in tenant <strong>{data.tenant.name}</strong>. Jump to a section to browse
			collections, edit schemas, or inspect the audit log.
		</p>
	</div>
</div>

<section class="panel">
	<div class="panel-header">
		<h2>Sections</h2>
	</div>
	<div class="panel-body section-grid">
		<a class="section-card" href={collectionsHref}>
			<h3>Collections</h3>
			<p class="muted">Browse entities, drop collections, create new ones.</p>
		</a>
		<a class="section-card" href={schemasHref}>
			<h3>Schemas</h3>
			<p class="muted">Edit entity schemas, link types, gates, validation rules.</p>
		</a>
		<a class="section-card" href={auditHref}>
			<h3>Audit Log</h3>
			<p class="muted">Filter the immutable change history by collection, actor, or time.</p>
		</a>
	</div>
</section>

<style>
	.section-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
		gap: 0.75rem;
	}

	.section-card {
		display: block;
		padding: 1rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.6rem;
		text-decoration: none;
		color: var(--text);
		transition: border-color 120ms ease, background 120ms ease;
	}

	.section-card:hover {
		border-color: rgba(125, 211, 252, 0.4);
		background: rgba(125, 211, 252, 0.04);
	}

	.section-card h3 {
		margin: 0 0 0.35rem;
		font-size: 1rem;
	}

	.section-card p {
		margin: 0;
		font-size: 0.85rem;
	}
</style>
