<script lang="ts">
import {
	type UserAclEntry,
	type UserRole,
	fetchUsers,
	removeUserRole,
	setUserRole,
} from '$lib/api';

const ROLES: UserRole[] = ['admin', 'write', 'read'];

let users = $state<UserAclEntry[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);

let newLogin = $state('');
let newRole = $state<UserRole>('read');
let adding = $state(false);
let addError = $state<string | null>(null);

let removingLogin = $state<string | null>(null);

async function loadUsers() {
	loading = true;
	try {
		users = await fetchUsers();
		error = null;
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to load users';
	} finally {
		loading = false;
	}
}

async function handleAdd() {
	if (!newLogin.trim()) return;
	adding = true;
	addError = null;
	try {
		await setUserRole(newLogin.trim(), newRole);
		newLogin = '';
		newRole = 'read';
		await loadUsers();
	} catch (e: unknown) {
		addError = e instanceof Error ? e.message : 'Failed to add user';
	} finally {
		adding = false;
	}
}

async function handleRoleChange(login: string, role: UserRole) {
	try {
		await setUserRole(login, role);
		await loadUsers();
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to update role';
	}
}

async function handleRemove(login: string) {
	try {
		await removeUserRole(login);
		removingLogin = null;
		await loadUsers();
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'Failed to remove user';
		removingLogin = null;
	}
}

$effect(() => {
	void loadUsers();
});
</script>

<div class="page-header">
	<div>
		<h1>Users</h1>
		<p class="muted">
			Deployment-wide role assignments. These ACLs apply across every tenant; for per-tenant
			membership, open a tenant and use the Members tab.
		</p>
	</div>
</div>

<section class="panel">
	<div class="panel-header">
		<h2>Assign Role</h2>
	</div>
	<div class="panel-body">
		<form
			class="add-form"
			onsubmit={(e) => {
				e.preventDefault();
				void handleAdd();
			}}
		>
			<input
				class="login-input"
				type="text"
				placeholder="Login (principal)"
				bind:value={newLogin}
				disabled={adding}
			/>
			<select bind:value={newRole} disabled={adding}>
				{#each ROLES as role}
					<option value={role}>{role}</option>
				{/each}
			</select>
			<button type="submit" class="primary" disabled={adding || !newLogin.trim()}>
				{adding ? 'Saving…' : 'Assign'}
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
	<p class="message">Loading users…</p>
{:else if users.length === 0}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">No explicit role assignments. Add one above.</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Users</h2>
			<span class="pill">{users.length}</span>
		</div>
		<div class="panel-body">
			<table>
				<thead>
					<tr>
						<th>Login</th>
						<th>Role</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each users as user}
						<tr>
							<td><code>{user.login}</code></td>
							<td>
								<select
									value={user.role}
									onchange={(e) => {
										const role = (e.target as HTMLSelectElement).value as UserRole;
										void handleRoleChange(user.login, role);
									}}
								>
									{#each ROLES as role}
										<option value={role}>{role}</option>
									{/each}
								</select>
							</td>
							<td>
								<div class="actions">
									{#if removingLogin === user.login}
										<span class="muted" style="font-size:0.85rem">Remove?</span>
										<button class="danger" onclick={() => void handleRemove(user.login)}>
											Confirm
										</button>
										<button onclick={() => (removingLogin = null)}>Cancel</button>
									{:else}
										<button class="danger" onclick={() => (removingLogin = user.login)}>
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

	.login-input {
		flex: 1;
		max-width: 24rem;
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
