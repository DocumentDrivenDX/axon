<script lang="ts">
import { listUsers, type User } from '$lib/api';

interface Props {
	value: string | null;
	onselect: (id: string) => void;
	disabled?: boolean;
	placeholder?: string;
}

// biome-ignore lint/style/useConst: Svelte $props() requires let for reactivity.
let { value, onselect, disabled = false, placeholder = 'Search users…' }: Props = $props();

let users = $state<User[]>([]);
let filter = $state('');
let showDropdown = $state(false);
let pasteMode = $state(false);
let pasteValue = $state('');
let loading = $state(true);

// Label for the currently selected user
let selectedLabel = $derived(() => {
	if (!value) return '';
	const u = users.find((u) => u.id === value);
	if (u) return `${u.display_name} — ${u.id.slice(0, 8)}`;
	return value.slice(0, 8);
});

let filtered = $derived(() => {
	if (!filter.trim()) return users;
	const q = filter.toLowerCase();
	return users.filter(
		(u) =>
			u.display_name.toLowerCase().includes(q) ||
			(u.email ?? '').toLowerCase().includes(q),
	);
});

async function load() {
	loading = true;
	try {
		users = await listUsers();
	} catch {
		// If list fails, fall back gracefully — paste mode still works
		users = [];
	} finally {
		loading = false;
	}
}

function selectUser(u: User) {
	filter = '';
	showDropdown = false;
	onselect(u.id);
}

function handlePasteSubmit() {
	const trimmed = pasteValue.trim();
	if (trimmed) {
		pasteValue = '';
		pasteMode = false;
		onselect(trimmed);
	}
}

function handleFilterFocus() {
	showDropdown = true;
}

function handleFilterBlur() {
	// Delay to allow click events on the dropdown to fire first
	setTimeout(() => {
		showDropdown = false;
	}, 150);
}

$effect(() => {
	void load();
});
</script>

<div class="user-picker">
	{#if pasteMode}
		<div class="paste-row">
			<input
				class="paste-input"
				type="text"
				placeholder="Paste user UUID…"
				bind:value={pasteValue}
				{disabled}
				onkeydown={(e) => {
					if (e.key === 'Enter') {
						e.preventDefault();
						handlePasteSubmit();
					}
				}}
			/>
			<button type="button" onclick={handlePasteSubmit} {disabled}>Use</button>
			<button type="button" onclick={() => (pasteMode = false)} {disabled}>Cancel</button>
		</div>
	{:else}
		<div class="combo">
			<div class="selected-value">
				{#if value}
					<span class="selected-chip">
						{selectedLabel()}
						<button
							type="button"
							class="clear-btn"
							aria-label="Clear selection"
							{disabled}
							onclick={() => {
								onselect('');
								filter = '';
							}}
						>×</button>
					</span>
				{/if}
			</div>
			<input
				class="filter-input"
				type="text"
				{placeholder}
				bind:value={filter}
				{disabled}
				onfocus={handleFilterFocus}
				onblur={handleFilterBlur}
			/>
			{#if showDropdown}
				<div class="dropdown" role="listbox">
					{#if loading}
						<div class="dropdown-item muted">Loading…</div>
					{:else if filtered().length === 0}
						<div class="dropdown-item muted">No users match</div>
					{:else}
						{#each filtered() as u}
							<button
								type="button"
								class="dropdown-item"
								role="option"
								aria-selected={u.id === value}
								onclick={() => selectUser(u)}
							>
								<span class="user-name">{u.display_name}</span>
								{#if u.email}
									<span class="user-email muted">{u.email}</span>
								{/if}
								<span class="user-id muted">{u.id.slice(0, 8)}</span>
							</button>
						{/each}
					{/if}
				</div>
			{/if}
		</div>
		<button
			type="button"
			class="paste-toggle"
			title="Paste UUID directly"
			{disabled}
			onclick={() => (pasteMode = true)}
		>UUID</button>
	{/if}
</div>

<style>
	.user-picker {
		display: flex;
		gap: 0.5rem;
		align-items: flex-start;
		position: relative;
	}

	.combo {
		position: relative;
		flex: 1;
		max-width: 28rem;
	}

	.filter-input {
		width: 100%;
	}

	.selected-value {
		margin-bottom: 0.25rem;
	}

	.selected-chip {
		display: inline-flex;
		align-items: center;
		gap: 0.25rem;
		background: rgba(255, 255, 255, 0.08);
		border-radius: 0.25rem;
		padding: 0.15em 0.4em;
		font-size: 0.85em;
		font-family: monospace;
	}

	.clear-btn {
		background: none;
		border: none;
		cursor: pointer;
		padding: 0 0.15em;
		font-size: 1em;
		color: inherit;
		opacity: 0.6;
		line-height: 1;
	}

	.clear-btn:hover {
		opacity: 1;
	}

	.dropdown {
		position: absolute;
		top: 100%;
		left: 0;
		right: 0;
		z-index: 100;
		background: var(--surface, #1e1e2e);
		border: 1px solid rgba(255, 255, 255, 0.12);
		border-radius: 0.375rem;
		max-height: 14rem;
		overflow-y: auto;
		box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
	}

	.dropdown-item {
		display: flex;
		align-items: baseline;
		gap: 0.5rem;
		padding: 0.45rem 0.75rem;
		cursor: pointer;
		font-size: 0.9rem;
		width: 100%;
		text-align: left;
		background: none;
		border: none;
		color: inherit;
	}

	.dropdown-item:hover,
	.dropdown-item[aria-selected='true'] {
		background: rgba(255, 255, 255, 0.07);
	}

	.user-name {
		font-weight: 500;
	}

	.user-email {
		font-size: 0.85em;
	}

	.user-id {
		font-family: monospace;
		font-size: 0.8em;
		margin-left: auto;
	}

	.paste-row {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		flex: 1;
	}

	.paste-input {
		flex: 1;
		max-width: 24rem;
	}

	.paste-toggle {
		font-size: 0.75rem;
		padding: 0.25em 0.5em;
		opacity: 0.7;
	}

	.paste-toggle:hover {
		opacity: 1;
	}

	.muted {
		color: var(--muted, rgba(255, 255, 255, 0.45));
	}
</style>
