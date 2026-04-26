<script lang="ts">
import { type AxonGraphqlError, isAxonGraphqlError } from '../api';

interface Props {
	error: unknown;
	testid?: string;
}

// biome-ignore lint/style/useConst: Svelte $props() requires let for reactivity.
let { error, testid = 'denial-message' }: Props = $props();

const denial = $derived<AxonGraphqlError | null>(isAxonGraphqlError(error) ? error : null);
const fallbackMessage = $derived(
	error instanceof Error ? error.message : typeof error === 'string' ? error : '',
);
const code = $derived(denial?.code ?? null);
const fieldPath = $derived(denial?.fieldPath ?? null);
const policy = $derived(
	denial?.detail && typeof denial.detail.policy === 'string' ? denial.detail.policy : null,
);
const reason = $derived(
	denial?.detail && typeof denial.detail.reason === 'string' ? denial.detail.reason : null,
);
const ruleIds = $derived(denial?.ruleIds ?? []);
const explanation = $derived(
	denial?.detail && typeof denial.detail.explanation === 'string'
		? denial.detail.explanation
		: null,
);
const missingIndex = $derived(
	denial?.detail && typeof denial.detail.missing_index === 'string'
		? denial.detail.missing_index
		: null,
);
</script>

<p class="message error" data-testid={testid}>
	{#if denial}
		{#if code}
			<span class="pill code-pill" data-testid={`${testid}-code`}>{code}</span>
		{/if}
		{#if reason}
			<span data-testid={`${testid}-reason`}>{reason}</span>
		{:else}
			<span>{denial.message}</span>
		{/if}
		{#if fieldPath}
			<span class="muted">·</span>
			<span data-testid={`${testid}-field-path`}>field <code>{fieldPath}</code></span>
		{/if}
		{#if policy}
			<span class="muted">·</span>
			<span data-testid={`${testid}-policy`}>policy <code>{policy}</code></span>
		{/if}
		{#if missingIndex}
			<span class="muted">·</span>
			<span data-testid={`${testid}-missing-index`}>
				missing index <code>{missingIndex}</code>
			</span>
		{/if}
		{#if explanation}
			<br />
			<span class="muted small" data-testid={`${testid}-explanation`}>{explanation}</span>
		{/if}
		{#if ruleIds.length > 0}
			<br />
			<span class="muted small" data-testid={`${testid}-rule-ids`}>
				rules: {ruleIds.join(', ')}
			</span>
		{/if}
	{:else}
		<span>{fallbackMessage || 'Unknown error'}</span>
	{/if}
</p>

<style>
	.code-pill {
		border-color: var(--danger);
		color: var(--danger);
		margin-right: 0.4rem;
	}
	.muted {
		color: var(--muted);
	}
	.small {
		font-size: 0.8rem;
	}
</style>
