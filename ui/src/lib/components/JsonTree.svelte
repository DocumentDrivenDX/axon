<script lang="ts">
// biome-ignore lint/correctness/noUnusedImports: Used in template branch for redacted leaves.
import { isRedactedPlaceholder } from '../redaction';
// biome-ignore lint/correctness/noUnusedImports: Used in template as recursive self-reference.
import JsonTree from './JsonTree.svelte';
import type { JsonValue } from './json-tree-types';

interface Props {
	data: JsonValue;
	editing?: boolean;
	depth?: number;
	onupdate?: (value: JsonValue) => void;
}

// biome-ignore lint/style/useConst: Svelte $props() requires let for reactivity.
let { data, editing = false, depth = 0, onupdate }: Props = $props();

// biome-ignore lint/style/useConst: Svelte $state() requires let for reactivity.
let collapsed = $state<Record<string, boolean>>({});

function toggleCollapsed(key: string) {
	collapsed[key] = !collapsed[key];
}

function typeOf(value: JsonValue): string {
	if (value === null) return 'null';
	if (Array.isArray(value)) return 'array';
	return typeof value;
}

function typeBadge(value: JsonValue): string {
	const t = typeOf(value);
	if (t === 'array') return `array[${(value as JsonValue[]).length}]`;
	if (t === 'object') {
		const keys = Object.keys(value as Record<string, JsonValue>);
		return `{${keys.length}}`;
	}
	return t;
}

function isContainer(value: JsonValue): boolean {
	return typeOf(value) === 'object' || typeOf(value) === 'array';
}

function formatLeaf(value: JsonValue): string {
	if (value === null) return 'null';
	if (typeof value === 'string') return value;
	return String(value);
}

function updateChild(key: string, childValue: JsonValue) {
	if (!onupdate) return;
	if (Array.isArray(data)) {
		const next = [...data];
		next[Number(key)] = childValue;
		onupdate(next);
	} else if (typeof data === 'object' && data !== null) {
		onupdate({ ...data, [key]: childValue });
	}
}

function handleLeafInput(event: Event) {
	if (!onupdate) return;
	const target = event.target as HTMLInputElement;
	const raw = target.value;

	if (typeof data === 'boolean') {
		onupdate(raw === 'true');
	} else if (typeof data === 'number') {
		const num = Number(raw);
		if (!Number.isNaN(num)) onupdate(num);
	} else if (data === null) {
		if (raw === 'null' || raw === '') {
			onupdate(null);
		} else {
			onupdate(raw);
		}
	} else {
		onupdate(raw);
	}
}

function handleCheckbox(event: Event) {
	if (!onupdate) return;
	const target = event.target as HTMLInputElement;
	onupdate(target.checked);
}

function entries(value: JsonValue): Array<[string, JsonValue]> {
	if (Array.isArray(value)) {
		return value.map((v, i) => [String(i), v]);
	}
	if (typeof value === 'object' && value !== null) {
		return Object.entries(value);
	}
	return [];
}
</script>

{#if isContainer(data)}
	{#each entries(data) as [key, child]}
		<div class="tree-row" style="padding-left: {depth * 1.2}rem">
			{#if isContainer(child)}
				<button class="toggle" aria-label="Toggle {key}" onclick={() => toggleCollapsed(key)}>
					<span class="arrow" class:open={!collapsed[key]}></span>
				</button>
				<span class="key">{key}</span>
				<span class="type-badge">{typeBadge(child)}</span>
				{#if !collapsed[key]}
					<div class="children">
						<JsonTree
							data={child}
							{editing}
							depth={depth + 1}
							onupdate={(v: JsonValue) => updateChild(key, v)}
						/>
					</div>
				{/if}
			{:else}
				<span class="leaf-spacer"></span>
				<span class="key">{key}</span>
				<span class="type-badge leaf-badge">{typeBadge(child)}</span>
				{#if editing && typeof child === 'boolean'}
					<input
						type="checkbox"
						class="leaf-checkbox"
						checked={child}
						onchange={handleCheckbox}
					/>
				{:else if editing && child !== null}
					<input
						class="leaf-input"
						type={typeof child === 'number' ? 'number' : 'text'}
						value={formatLeaf(child)}
						oninput={(e) => {
							const target = e.target as HTMLInputElement;
							const raw = target.value;
							if (typeof child === 'number') {
								const num = Number(raw);
								if (!Number.isNaN(num)) updateChild(key, num);
							} else {
								updateChild(key, raw);
							}
						}}
					/>
				{:else if isRedactedPlaceholder(child)}
					<span
						class="leaf-value is-redacted"
						data-testid="redacted-field"
					>{formatLeaf(child)}</span>
				{:else}
					<span class="leaf-value" class:is-string={typeof child === 'string'}
						class:is-number={typeof child === 'number'}
						class:is-boolean={typeof child === 'boolean'}
						class:is-null={child === null}
					>
						{#if typeof child === 'string'}"{child}"{:else}{formatLeaf(child)}{/if}
					</span>
				{/if}
			{/if}
		</div>
	{/each}
{:else}
	<div class="tree-row" style="padding-left: {depth * 1.2}rem">
		{#if editing && typeof data === 'boolean'}
			<input type="checkbox" class="leaf-checkbox" checked={data} onchange={handleCheckbox} />
		{:else if editing && data !== null}
			<input
				class="leaf-input"
				type={typeof data === 'number' ? 'number' : 'text'}
				value={formatLeaf(data)}
				oninput={handleLeafInput}
			/>
		{:else if isRedactedPlaceholder(data)}
			<span class="leaf-value is-redacted" data-testid="redacted-field">{formatLeaf(data)}</span>
		{:else}
			<span class="leaf-value"
				class:is-string={typeof data === 'string'}
				class:is-number={typeof data === 'number'}
				class:is-boolean={typeof data === 'boolean'}
				class:is-null={data === null}
			>
				{#if typeof data === 'string'}"{data}"{:else}{formatLeaf(data)}{/if}
			</span>
		{/if}
	</div>
{/if}

<style>
	.tree-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.4rem;
		padding-top: 0.25rem;
		padding-bottom: 0.25rem;
		min-height: 1.7rem;
	}

	.toggle {
		all: unset;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 1.1rem;
		height: 1.1rem;
		flex-shrink: 0;
	}

	.arrow {
		display: inline-block;
		width: 0;
		height: 0;
		border-left: 0.32rem solid transparent;
		border-right: 0.32rem solid transparent;
		border-top: 0.4rem solid var(--muted);
		transition: transform 120ms ease;
		transform: rotate(-90deg);
	}

	.arrow.open {
		transform: rotate(0deg);
	}

	.leaf-spacer {
		display: inline-block;
		width: 1.1rem;
		flex-shrink: 0;
	}

	.key {
		color: var(--accent);
		font-weight: 600;
		font-size: 0.88rem;
	}

	.type-badge {
		display: inline-flex;
		align-items: center;
		border: 1px solid rgba(125, 211, 252, 0.2);
		border-radius: 999px;
		padding: 0.05rem 0.45rem;
		color: var(--muted);
		font-size: 0.72rem;
		font-weight: 500;
		letter-spacing: 0.02em;
	}

	.leaf-badge {
		border-color: rgba(148, 163, 184, 0.2);
	}

	.leaf-value {
		font-family: 'Iosevka Term', 'IBM Plex Mono', monospace;
		font-size: 0.88rem;
	}

	.is-string {
		color: #86efac;
	}

	.is-number {
		color: #fbbf24;
	}

	.is-boolean {
		color: #c084fc;
	}

	.is-null {
		color: var(--muted);
		font-style: italic;
	}

	.is-redacted {
		color: var(--accent, #fbbf24);
		font-weight: 600;
		letter-spacing: 0.02em;
	}

	.leaf-input {
		width: auto;
		min-width: 6rem;
		max-width: 20rem;
		padding: 0.2rem 0.5rem;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
		background: rgba(15, 23, 32, 0.75);
		color: var(--text);
		font-family: 'Iosevka Term', 'IBM Plex Mono', monospace;
		font-size: 0.85rem;
	}

	.leaf-input:focus {
		outline: none;
		border-color: var(--accent-strong);
	}

	.leaf-checkbox {
		accent-color: var(--accent-strong);
		width: 1rem;
		height: 1rem;
	}

	.children {
		width: 100%;
	}
</style>
