<script lang="ts">
import {
	type User,
	type UserAclEntry,
	type UserRole,
	createUser,
	fetchUsers,
	listUsers,
	removeUserRole,
	setUserRole,
	suspendUser,
} from '$lib/api';

// ── Section A: provisioned users ────────────────────────────────────────────

let users = $state<User[]>([]);
let usersLoading = $state(true);
let usersError = $state<string | null>(null);

let newDisplayName = $state('');
let newEmail = $state('');
let creating = $state(false);
let createError = $state<string | null>(null);

let suspendingId = $state<string | null>(null);

async function loadProvisionedUsers() {
	usersLoading = true;
	try {
		users = await listUsers();
		usersError = null;
	} catch (e: unknown) {
		usersError = e instanceof Error ? e.message : 'Failed to load users';
	} finally {
		usersLoading = false;
	}
}

async function handleCreateUser() {
	if (!newDisplayName.trim()) return;
	creating = true;
	createError = null;
	try {
		await createUser(newDisplayName.trim(), newEmail.trim() || null);
		newDisplayName = '';
		newEmail = '';
		await loadProvisionedUsers();
	} catch (e: unknown) {
		createError = e instanceof Error ? e.message : 'Failed to create user';
	} finally {
		creating = false;
	}
}

async function handleSuspend(id: string) {
	try {
		await suspendUser(id);
		suspendingId = null;
		await loadProvisionedUsers();
	} catch (e: unknown) {
		usersError = e instanceof Error ? e.message : 'Failed to suspend user';
		suspendingId = null;
	}
}

function formatDate(ms: number): string {
	return new Date(ms).toLocaleString();
}

// ── Section B: global user ACL ───────────────────────────────────────────────

const ROLES: UserRole[] = ['admin', 'write', 'read'];

let aclUsers = $state<UserAclEntry[]>([]);
let aclLoading = $state(true);
let aclError = $state<string | null>(null);

let newLogin = $state('');
let newRole = $state<UserRole>('read');
let adding = $state(false);
let addError = $state<string | null>(null);

let removingLogin = $state<string | null>(null);

async function loadAclUsers() {
	aclLoading = true;
	try {
		aclUsers = await fetchUsers();
		aclError = null;
	} catch (e: unknown) {
		aclError = e instanceof Error ? e.message : 'Failed to load ACL users';
	} finally {
		aclLoading = false;
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
		await loadAclUsers();
	} catch (e: unknown) {
		addError = e instanceof Error ? e.message : 'Failed to add user';
	} finally {
		adding = false;
	}
}

async function handleRoleChange(login: string, role: UserRole) {
	try {
		await setUserRole(login, role);
		await loadAclUsers();
	} catch (e: unknown) {
		aclError = e instanceof Error ? e.message : 'Failed to update role';
	}
}

async function handleRemove(login: string) {
	try {
		await removeUserRole(login);
		removingLogin = null;
		await loadAclUsers();
	} catch (e: unknown) {
		aclError = e instanceof Error ? e.message : 'Failed to remove user';
		removingLogin = null;
	}
}

$effect(() => {
	void loadProvisionedUsers();
	void loadAclUsers();
});
</script>

<div class="page-header">
	<div>
		<h1>Users</h1>
		<p class="muted">Manage provisioned users and deployment-wide role assignments.</p>
	</div>
</div>

<!-- Section A: provisioned users -->

<section class="panel">
	<div class="panel-header">
		<h2>Create User</h2>
	</div>
	<div class="panel-body">
		<form
			class="add-form"
			onsubmit={(e) => {
				e.preventDefault();
				void handleCreateUser();
			}}
		>
			<input
				class="name-input"
				type="text"
				placeholder="Display name (required)"
				bind:value={newDisplayName}
				disabled={creating}
			/>
			<input
				class="email-input"
				type="email"
				placeholder="Email (optional)"
				bind:value={newEmail}
				disabled={creating}
			/>
			<button type="submit" class="primary" disabled={creating || !newDisplayName.trim()}>
				{creating ? 'Creating…' : 'Create User'}
			</button>
		</form>
		{#if createError}
			<p class="message error">{createError}</p>
		{/if}
	</div>
</section>

{#if usersError}
	<p class="message error">{usersError}</p>
{/if}

{#if usersLoading}
	<p class="message">Loading users…</p>
{:else if users.length === 0}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">No provisioned users yet. Create one above.</p>
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
						<th>Display Name</th>
						<th>Email</th>
						<th>Created</th>
						<th>Status</th>
						<th>Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each users as user}
						<tr>
							<td>
								<code>{user.display_name}</code>
								<span class="id-hint muted">{user.id.slice(0, 8)}</span>
							</td>
							<td>{user.email ?? '—'}</td>
							<td>{formatDate(user.created_at_ms)}</td>
							<td>
								{#if user.suspended_at_ms !== null}
									<span class="badge suspended">Suspended</span>
								{:else}
									<span class="badge active">Active</span>
								{/if}
							</td>
							<td>
								<div class="actions">
									{#if user.suspended_at_ms === null}
										{#if suspendingId === user.id}
											<span class="muted" style="font-size:0.85rem">Suspend?</span>
											<button class="danger" onclick={() => void handleSuspend(user.id)}>
												Confirm
											</button>
											<button onclick={() => (suspendingId = null)}>Cancel</button>
										{:else}
											<button class="danger" onclick={() => (suspendingId = user.id)}>
												Suspend
											</button>
										{/if}
									{:else}
										<span class="muted" style="font-size:0.85rem">—</span>
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

<!-- Section B: deployment ACLs -->

<div class="section-divider">
	<h2>Deployment ACLs</h2>
	<p class="muted">
		Deployment-wide role assignments. These ACLs apply across every tenant; for per-tenant
		membership, open a tenant and use the Members tab.
	</p>
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

{#if aclError}
	<p class="message error">{aclError}</p>
{/if}

{#if aclLoading}
	<p class="message">Loading ACL users…</p>
{:else if aclUsers.length === 0}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">No explicit role assignments. Add one above.</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>ACL Entries</h2>
			<span class="pill">{aclUsers.length}</span>
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
					{#each aclUsers as user}
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
		flex-wrap: wrap;
	}

	.name-input {
		flex: 1;
		min-width: 12rem;
		max-width: 20rem;
	}

	.email-input {
		flex: 1;
		min-width: 12rem;
		max-width: 20rem;
	}

	.login-input {
		flex: 1;
		max-width: 24rem;
	}

	.section-divider {
		margin-top: 2.5rem;
		margin-bottom: 1rem;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
		padding-top: 1.5rem;
	}

	.section-divider h2 {
		margin: 0 0 0.25rem;
		font-size: 1.15rem;
	}

	button.danger {
		border-color: var(--danger, #fb7185);
		color: var(--danger, #fb7185);
	}

	.badge {
		display: inline-block;
		padding: 0.1em 0.5em;
		border-radius: 0.25rem;
		font-size: 0.8em;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.badge.active {
		background: rgba(74, 222, 128, 0.15);
		color: #4ade80;
	}

	.badge.suspended {
		background: rgba(251, 113, 133, 0.15);
		color: #fb7185;
	}

	.id-hint {
		font-size: 0.78em;
		margin-left: 0.4em;
		font-family: monospace;
	}

	code {
		font-family: monospace;
		font-size: 0.85em;
		background: rgba(255, 255, 255, 0.06);
		padding: 0.1em 0.35em;
		border-radius: 0.25rem;
	}
</style>
