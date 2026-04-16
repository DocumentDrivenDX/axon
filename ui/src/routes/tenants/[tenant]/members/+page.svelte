<script lang="ts">
import {
	type TenantMember,
	type TenantMemberRole,
	fetchTenantMembers,
	removeTenantMember,
	upsertTenantMember,
} from '$lib/api';
import UserPicker from '$lib/UserPicker.svelte';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();

const ROLES: TenantMemberRole[] = ['admin', 'write', 'read'];

let members = $state<TenantMember[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);

let newUserId = $state('');
let newRole = $state<TenantMemberRole>('read');
let adding = $state(false);
let addError = $state<string | null>(null);

let removingUserId = $state<string | null>(null);

async function loadMembers() {
	loading = true;
	try {
		members = await fetchTenantMembers(data.tenant.id);
		error = null;
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to load members';
	} finally {
		loading = false;
	}
}

async function handleAdd() {
	if (!newUserId.trim()) return;
	adding = true;
	addError = null;
	try {
		await upsertTenantMember(data.tenant.id, newUserId.trim(), newRole);
		newUserId = '';
		newRole = 'read';
		await loadMembers();
	} catch (e: unknown) {
		addError = e instanceof Error ? e.message : 'Failed to add member';
	} finally {
		adding = false;
	}
}

async function handleRoleChange(userId: string, role: TenantMemberRole) {
	try {
		await upsertTenantMember(data.tenant.id, userId, role);
		await loadMembers();
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to update role';
	}
}

async function handleRemove(userId: string) {
	try {
		await removeTenantMember(data.tenant.id, userId);
		removingUserId = null;
		await loadMembers();
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to remove member';
		removingUserId = null;
	}
}

$effect(() => {
	void loadMembers();
});
</script>

<div class="page-header">
	<div>
		<h1>Members</h1>
		<p class="muted">
			Per-tenant role assignments. Members are separate from the global user ACL.
		</p>
	</div>
</div>

<section class="panel">
	<div class="panel-header">
		<h2>Add Member</h2>
	</div>
	<div class="panel-body">
		<form
			class="add-form"
			onsubmit={(e) => {
				e.preventDefault();
				void handleAdd();
			}}
		>
			<UserPicker
				value={newUserId || null}
				onselect={(id) => (newUserId = id)}
				disabled={adding}
				placeholder="Search or pick user…"
			/>
			<select bind:value={newRole} disabled={adding}>
				{#each ROLES as role}
					<option value={role}>{role}</option>
				{/each}
			</select>
			<button type="submit" class="primary" disabled={adding || !newUserId.trim()}>
				{adding ? 'Adding…' : 'Add'}
			</button>
		</form>
		{#if addError}
			<p class="message error">{addError}</p>
		{/if}
	</div>
</section>

{#if error}
	<p class="message error">{error}</p>
{/if}

{#if loading}
	<p class="message">Loading members…</p>
{:else if members.length === 0}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">No members yet. Add one above.</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Members</h2>
			<span class="pill">{members.length}</span>
		</div>
		<div class="panel-body">
			<table>
				<thead>
					<tr>
						<th>User ID</th>
						<th>Role</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each members as member}
						<tr>
							<td><code>{member.user_id}</code></td>
							<td>
								<select
									value={member.role}
									onchange={(e) => {
										const role = (e.target as HTMLSelectElement).value as TenantMemberRole;
										void handleRoleChange(member.user_id, role);
									}}
								>
									{#each ROLES as role}
										<option value={role}>{role}</option>
									{/each}
								</select>
							</td>
							<td>
								<div class="actions">
									{#if removingUserId === member.user_id}
										<span class="muted" style="font-size:0.85rem">Remove?</span>
										<button class="danger" onclick={() => void handleRemove(member.user_id)}>
											Confirm
										</button>
										<button onclick={() => (removingUserId = null)}>Cancel</button>
									{:else}
										<button
											class="danger"
											onclick={() => (removingUserId = member.user_id)}
										>
											Remove
										</button>
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
	.add-form {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}

	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}

	code {
		font-family: monospace;
		font-size: 0.85em;
		background: rgba(255, 255, 255, 0.06);
		padding: 0.1em 0.35em;
		border-radius: 0.25rem;
	}
</style>
