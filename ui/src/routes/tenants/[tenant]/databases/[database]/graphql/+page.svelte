<script lang="ts">
import { executeGraphql, type GraphQLResponse } from '$lib/api';
import type { PageData } from './$types';

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);

const DEFAULT_QUERY = `# Try a query against this database.
# Collection types are generated dynamically from the collection schemas.
#
# Example:
#   query ListTasks {
#     tasks(limit: 10) {
#       id
#       version
#       data
#     }
#   }
#
# Run a minimal introspection query to see what's available:

{
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
  }
}
`;

let query = $state(DEFAULT_QUERY);
let variables = $state('{}');
let result = $state<GraphQLResponse | null>(null);
let running = $state(false);
let error = $state<string | null>(null);

async function run() {
	running = true;
	error = null;
	result = null;
	try {
		let parsedVars: Record<string, unknown> = {};
		if (variables.trim()) {
			try {
				parsedVars = JSON.parse(variables) as Record<string, unknown>;
			} catch (e) {
				error = `Variables must be valid JSON: ${(e as Error).message}`;
				running = false;
				return;
			}
		}
		result = await executeGraphql(query, parsedVars, scope);
	} catch (e: unknown) {
		error = e instanceof Error ? e.message : 'GraphQL request failed';
	} finally {
		running = false;
	}
}

function formatResult(r: GraphQLResponse): string {
	return JSON.stringify(r, null, 2);
}
</script>

<div class="page-header">
	<div>
		<h1>GraphQL</h1>
		<p class="muted">
			Run GraphQL queries, mutations, and introspection against
			<code>/tenants/{scope.tenant}/databases/{scope.database}/graphql</code>.
		</p>
	</div>
	<div class="actions">
		<button class="primary" disabled={running} onclick={run}>
			{running ? 'Running…' : 'Run (⌘↵)'}
		</button>
	</div>
</div>

<div class="gql-shell">
	<section class="panel">
		<div class="panel-header">
			<h2>Query</h2>
		</div>
		<div class="panel-body stack">
			<textarea
				class="code"
				data-testid="graphql-query"
				bind:value={query}
				onkeydown={(e) => {
					if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
						e.preventDefault();
						void run();
					}
				}}
			></textarea>
			<details>
				<summary>Variables (JSON)</summary>
				<textarea class="code small" data-testid="graphql-variables" bind:value={variables}
				></textarea>
			</details>
		</div>
	</section>

	<section class="panel">
		<div class="panel-header">
			<h2>Response</h2>
			{#if result?.errors}
				<span class="pill pill-error">{result.errors.length} error{result.errors.length > 1 ? 's' : ''}</span>
			{:else if result}
				<span class="pill">ok</span>
			{/if}
		</div>
		<div class="panel-body">
			{#if error}
				<p class="message error">{error}</p>
			{:else if !result}
				<p class="muted">Run a query to see results.</p>
			{:else}
				<pre data-testid="graphql-response">{formatResult(result)}</pre>
			{/if}
		</div>
	</section>
</div>

<style>
	.gql-shell {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
		gap: 1rem;
	}

	textarea.code {
		width: 100%;
		min-height: 22rem;
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.88rem;
		tab-size: 2;
	}

	textarea.code.small {
		min-height: 6rem;
	}

	pre {
		margin: 0;
		font-size: 0.85rem;
		white-space: pre-wrap;
		word-break: break-word;
	}

	.pill-error {
		border-color: rgba(251, 113, 133, 0.4);
		color: var(--danger, #fb7185);
	}

	@media (max-width: 1100px) {
		.gql-shell {
			grid-template-columns: 1fr;
		}
	}
</style>
