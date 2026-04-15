<script lang="ts">
import { base } from '$app/paths';
import { invalidate } from '$app/navigation';
import { createTenantDatabase, deleteTenantDatabase } from '$lib/api';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();

let newDbName = $state('');
let creating = $state(false);
let createError = $state<string | null>(null);
let deletingName = $state<string | null>(null);
let error = $state<string | null>(null);

function databaseHref(name: string): string {
	return `${base}/tenants/${encodeURIComponent(data.tenant.db_name)}/databases/${encodeURIComponent(name)}`;
}

async function handleCreate() {
	if (!newDbName.trim()) return;
	creating = true;
	createError = null;
	try {
		await createTenantDatabase(data.tenant.id, newDbName.trim());
		newDbName = '';
		await invalidate(() => true);
	} catch (e: unknown) {
		createError = e instanceof Error ? e.message : 'Failed to create database';
	} finally {
		creating = false;
	}
}

async function handleDelete(name: string) {
	try {
		await deleteTenantDatabase(data.tenant.id, name);
		deletingName = null;
		await invalidate(() => true);
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to delete database';
		deletingName = null;
	}
}
</script>

<div class="page-header">
	<div>
		<h1>Databases</h1>
		<p class="muted">
			Each database in this tenant is an isolated data store with its own collections,
			schemas, and audit log.
		</p>
	</div>
</div>

<section class="panel">
	<div class="panel-header">
		<h2>Create Database</h2>
	</div>
	<div class="panel-body">
		<form
			class="create-form"
			onsubmit={(e) => {
				e.preventDefault();
				void handleCreate();
			}}
		>
			<input
				class="name-input"
				type="text"
				placeholder="Database name (letters, digits, _ or -)"
				bind:value={newDbName}
				disabled={creating}
			/>
			<button type="submit" class="primary" disabled={creating || !newDbName.trim()}>
				{creating ? 'Creating…' : 'Create'}
			</button>
		</form>
		{#if createError}
			<p class="message error">{createError}</p>
		{/if}
	</div>
</section>

{#if error}
	<p class="message error">{error}</p>
{/if}

{#if data.databases.length === 0}
	<section class="panel">
		<div class="panel-body stack">
			<h2>No databases yet</h2>
			<p class="muted">Create one above to start managing collections and schemas.</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Databases</h2>
			<span class="pill">{data.databases.length}</span>
		</div>
		<div class="panel-body">
			<table>
				<thead>
					<tr>
						<th>Name</th>
						<th>Created</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each data.databases as db}
						<tr>
							<td>
								<a href={databaseHref(db.name)}>
									<strong>{db.name}</strong>
								</a>
							</td>
							<td class="muted">{new Date(db.created_at_ms).toLocaleDateString()}</td>
							<td>
								<div class="actions">
									<a class="button-link" href={databaseHref(db.name)}>Open</a>
									{#if deletingName === db.name}
										<span class="muted" style="font-size:0.85rem">Delete {db.name}?</span>
										<button class="danger" onclick={() => void handleDelete(db.name)}>
											Confirm
										</button>
										<button onclick={() => (deletingName = null)}>Cancel</button>
									{:else}
										<button class="danger" onclick={() => (deletingName = db.name)}>Delete</button>
									{/if}
								</div>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
{/if}

<style>
	.create-form {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}

	.name-input {
		flex: 1;
		max-width: 24rem;
	}

	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}
</style>
