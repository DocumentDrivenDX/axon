<script lang="ts">
interface Props {
	title: string;
	subject?: string | null;
	policyVersion?: number | null;
	schemaVersion?: number | null;
	policyHref?: string | null;
	testid?: string;
}

// biome-ignore lint/style/useConst: Svelte $props() requires let for reactivity.
let {
	title,
	subject = null,
	policyVersion = null,
	schemaVersion = null,
	policyHref = null,
	testid = 'policy-empty-state',
}: Props = $props();
</script>

<div class="policy-empty-state" data-testid={testid}>
	<p class="muted" data-testid={`${testid}-title`}>{title}</p>
	<p class="muted small">
		{#if subject}
			<span data-testid={`${testid}-subject`}>as {subject}</span>
		{/if}
		{#if subject && (schemaVersion != null || policyVersion != null)}
			<span class="muted">·</span>
		{/if}
		{#if schemaVersion != null}
			<span data-testid={`${testid}-schema-version`}>schema v{schemaVersion}</span>
		{/if}
		{#if schemaVersion != null && policyVersion != null}
			<span class="muted">·</span>
		{/if}
		{#if policyVersion != null}
			<span data-testid={`${testid}-policy-version`}>policy v{policyVersion}</span>
		{/if}
	</p>
	{#if policyHref}
		<p class="muted small">
			<a href={policyHref} data-testid={`${testid}-policy-link`}>View policy</a>
		</p>
	{/if}
</div>

<style>
	.policy-empty-state {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	.small {
		font-size: 0.85rem;
	}
	.muted {
		color: var(--muted);
	}
</style>
