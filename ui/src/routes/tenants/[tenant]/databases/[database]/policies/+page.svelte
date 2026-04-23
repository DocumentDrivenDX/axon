<script lang="ts">
import {
	type CollectionSummary,
	type EffectiveCollectionPolicy,
	type EntityRecord,
	type PolicyExplanation,
	explainPolicy,
	fetchCollections,
	fetchEffectivePolicy,
	fetchEntities,
} from '$lib/api';
import { onMount } from 'svelte';
import type { PageData } from './$types';

type SubjectOption = {
	id: string;
	label: string;
	detail: string | null;
};

const { data }: { data: PageData } = $props();
const scope = $derived(data.scope);
const scopeLabel = $derived(`${data.tenant.db_name} / ${data.database.name}`);

let collections = $state<CollectionSummary[]>([]);
let subjects = $state<SubjectOption[]>([]);
let selectedCollection = $state('');
let selectedSubject = $state('');
let sampleEntity = $state<EntityRecord | null>(null);
let effectivePolicy = $state<EffectiveCollectionPolicy | null>(null);
let explanation = $state<PolicyExplanation | null>(null);
let loadingShell = $state(true);
let loadingEffectivePolicy = $state(false);
let loadingExplanation = $state(false);
let shellError = $state<string | null>(null);
let policyError = $state<string | null>(null);
let explanationError = $state<string | null>(null);
let refreshToken = 0;

const selectedCollectionSummary = $derived(
	collections.find((collection) => collection.name === selectedCollection) ?? null,
);
const selectedSubjectOption = $derived(
	subjects.find((subject) => subject.id === selectedSubject) ?? null,
);
const schemaVersionLabel = $derived(
	selectedCollectionSummary?.schema_version
		? `v${selectedCollectionSummary.schema_version}`
		: 'No schema',
);
const policyVersionLabel = $derived(
	effectivePolicy ? `v${effectivePolicy.policyVersion}` : 'Not loaded',
);
const sampleEntityLabel = $derived(
	sampleEntity ? `${sampleEntity.collection}/${sampleEntity.id}` : 'No sample entity',
);

function defaultCollectionName(nextCollections: CollectionSummary[]): string {
	for (const preferredName of ['invoices', 'task', 'expense']) {
		const preferred = nextCollections.find((collection) => collection.name === preferredName);
		if (preferred) return preferred.name;
	}
	const preferred = nextCollections.find(
		(collection) => !['user', 'users'].includes(collection.name),
	);
	return preferred?.name ?? nextCollections[0]?.name ?? '';
}

function defaultSubjectId(nextSubjects: SubjectOption[]): string {
	return (
		nextSubjects.find((subject) => subject.id === 'finance-agent')?.id ??
		nextSubjects.find((subject) => subject.id === 'requester')?.id ??
		nextSubjects.find((subject) => subject.id === 'contractor')?.id ??
		nextSubjects[0]?.id ??
		'guest'
	);
}

async function loadSubjectOptions(nextCollections: CollectionSummary[]): Promise<SubjectOption[]> {
	const subjectCollection = nextCollections.find((collection) =>
		['user', 'users'].includes(collection.name),
	)?.name;
	if (!subjectCollection) {
		return [{ id: 'guest', label: 'Current caller', detail: null }];
	}

	try {
		const result = await fetchEntities(subjectCollection, { limit: 100 }, scope);
		const options = result.entities
			.map((entity) => {
				const userId = typeof entity.data.user_id === 'string' ? entity.data.user_id : entity.id;
				const label =
					typeof entity.data.display_name === 'string' ? entity.data.display_name : userId;
				const detailParts = [
					typeof entity.data.approval_role === 'string' ? entity.data.approval_role : null,
					typeof entity.data.procurement_role === 'string' ? entity.data.procurement_role : null,
				].filter((value): value is string => Boolean(value));
				return {
					id: userId,
					label,
					detail: detailParts.length ? detailParts.join(' / ') : null,
				};
			})
			.sort((left, right) => left.label.localeCompare(right.label));

		return options.length ? options : [{ id: 'guest', label: 'Current caller', detail: null }];
	} catch {
		return [{ id: 'guest', label: 'Current caller', detail: null }];
	}
}

function formatFields(fields: string[]): string {
	return fields.length ? fields.join(', ') : 'None';
}

function errorMessage(error: unknown, fallback: string): string {
	return error instanceof Error ? error.message : fallback;
}

async function refreshPolicyView() {
	if (!selectedCollection) return;

	const token = ++refreshToken;
	loadingEffectivePolicy = true;
	loadingExplanation = true;
	policyError = null;
	explanationError = null;
	effectivePolicy = null;
	explanation = null;
	sampleEntity = null;

	try {
		const entities = await fetchEntities(
			selectedCollection,
			{ limit: 1 },
			scope,
			selectedSubject ? { headers: { 'x-axon-actor': selectedSubject } } : {},
		);
		if (token !== refreshToken) return;
		sampleEntity = entities.entities[0] ?? null;
	} catch {
		if (token !== refreshToken) return;
		sampleEntity = null;
	}

	const actorOptions = selectedSubject ? { actor: selectedSubject } : {};
	const explainInput = {
		operation: 'read',
		collection: selectedCollection,
		...(sampleEntity ? { entityId: sampleEntity.id } : {}),
	};

	const [effectiveResult, explanationResult] = await Promise.allSettled([
		fetchEffectivePolicy(selectedCollection, scope, {
			entityId: sampleEntity?.id ?? null,
			...actorOptions,
		}),
		explainPolicy(explainInput, scope, actorOptions),
	]);

	if (token !== refreshToken) return;

	if (effectiveResult.status === 'fulfilled') {
		effectivePolicy = effectiveResult.value;
	} else {
		policyError = errorMessage(effectiveResult.reason, 'Failed to load effective policy');
	}

	if (explanationResult.status === 'fulfilled') {
		explanation = explanationResult.value;
	} else {
		explanationError = errorMessage(explanationResult.reason, 'Failed to explain policy');
	}

	loadingEffectivePolicy = false;
	loadingExplanation = false;
}

async function loadRouteShell() {
	loadingShell = true;
	shellError = null;

	try {
		const nextCollections = await fetchCollections(scope);
		const nextSubjects = await loadSubjectOptions(nextCollections);
		collections = nextCollections;
		subjects = nextSubjects;
		selectedCollection = defaultCollectionName(nextCollections);
		selectedSubject = defaultSubjectId(nextSubjects);
	} catch (error) {
		shellError = errorMessage(error, 'Failed to load policy route shell');
	} finally {
		loadingShell = false;
	}

	if (selectedCollection) {
		await refreshPolicyView();
	}
}

function handleCollectionChange(event: Event) {
	selectedCollection = (event.currentTarget as HTMLSelectElement).value;
	void refreshPolicyView();
}

function handleSubjectChange(event: Event) {
	selectedSubject = (event.currentTarget as HTMLSelectElement).value;
	void refreshPolicyView();
}

onMount(() => {
	void loadRouteShell();
});
</script>

<div class="page-header">
	<div>
		<h1>Policies</h1>
		<p class="muted" data-testid="policy-scope">
			Inspect live GraphQL policy outcomes for <strong>{scopeLabel}</strong>.
		</p>
	</div>
</div>

{#if loadingShell}
	<section class="panel">
		<div class="panel-body">
			<p class="muted">Loading policy collections and subject options…</p>
		</div>
	</section>
{:else if shellError}
	<section class="panel">
		<div class="panel-body">
			<p class="message error">{shellError}</p>
		</div>
	</section>
{:else}
	<section class="panel">
		<div class="panel-header">
			<h2>Scope</h2>
		</div>
		<div class="panel-body controls-grid">
			<label class="control">
				<span>Collection</span>
				<select
					id="policy-collection"
					data-testid="policy-collection-picker"
					bind:value={selectedCollection}
					onchange={handleCollectionChange}
				>
					{#each collections as collection}
						<option value={collection.name}>{collection.name}</option>
					{/each}
				</select>
			</label>

			<label class="control">
				<span>Subject</span>
				<select
					id="policy-subject"
					data-testid="policy-subject-picker"
					bind:value={selectedSubject}
					onchange={handleSubjectChange}
				>
					{#each subjects as subject}
						<option value={subject.id}>
							{subject.label}{subject.detail ? ` · ${subject.detail}` : ''}
						</option>
					{/each}
				</select>
			</label>

			<div class="control static">
				<span>Sample entity</span>
				<div class="value" data-testid="policy-sample-entity">{sampleEntityLabel}</div>
			</div>

			<div class="control static">
				<span>Active schema version</span>
				<div class="value" data-testid="policy-schema-version">{schemaVersionLabel}</div>
			</div>

			<div class="control static">
				<span>Active policy version</span>
				<div class="value" data-testid="policy-version">{policyVersionLabel}</div>
			</div>

			<div class="control static">
				<span>Subject detail</span>
				<div class="value">{selectedSubjectOption?.detail ?? 'Header-scoped actor'}</div>
			</div>
		</div>
	</section>

	<div class="policy-grid">
		<section class="panel">
			<div class="panel-header">
				<h2>Effective Policy</h2>
			</div>
			<div class="panel-body stack">
				{#if loadingEffectivePolicy}
					<p class="muted">Loading effective policy…</p>
				{:else if policyError}
					<p class="message error">{policyError}</p>
				{:else if effectivePolicy}
					<div class="capability-grid" data-testid="policy-capabilities">
						<div class="capability">
							<span class="label">Read</span>
							<strong>{effectivePolicy.canRead ? 'Allowed' : 'Denied'}</strong>
						</div>
						<div class="capability">
							<span class="label">Create</span>
							<strong>{effectivePolicy.canCreate ? 'Allowed' : 'Denied'}</strong>
						</div>
						<div class="capability">
							<span class="label">Update</span>
							<strong>{effectivePolicy.canUpdate ? 'Allowed' : 'Denied'}</strong>
						</div>
						<div class="capability">
							<span class="label">Delete</span>
							<strong>{effectivePolicy.canDelete ? 'Allowed' : 'Denied'}</strong>
						</div>
					</div>

					<div class="field-list">
						<div>
							<span class="label">Redacted fields</span>
							<p data-testid="policy-redacted-fields">
								{formatFields(effectivePolicy.redactedFields)}
							</p>
						</div>
						<div>
							<span class="label">Denied fields</span>
							<p data-testid="policy-denied-fields">
								{formatFields(effectivePolicy.deniedFields)}
							</p>
						</div>
					</div>
				{:else}
					<p class="muted">Choose a collection to inspect policy capabilities.</p>
				{/if}
			</div>
		</section>

		<section class="panel">
			<div class="panel-header">
				<h2>Policy Explanation</h2>
			</div>
			<div class="panel-body stack">
				{#if loadingExplanation}
					<p class="muted">Explaining policy for the selected subject…</p>
				{:else if explanationError}
					<p class="message error">{explanationError}</p>
				{:else if explanation}
					<div data-testid="policy-explanation">
						<div class="explanation-row">
							<span class="label">Decision</span>
							<strong>{explanation.decision}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Reason</span>
							<strong>{explanation.reason}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Rules</span>
							<strong>{explanation.rules.map((rule) => rule.name).join(', ') || 'None'}</strong>
						</div>
						<div class="explanation-row">
							<span class="label">Denied fields</span>
							<strong>{formatFields(explanation.deniedFields)}</strong>
						</div>
						{#if explanation.approval}
							<div class="explanation-row">
								<span class="label">Approval route</span>
								<strong>
									{explanation.approval.name} · {explanation.approval.role ?? 'no role'}
								</strong>
							</div>
						{/if}
					</div>
				{:else}
					<p class="muted">Choose a subject to inspect explainPolicy output.</p>
				{/if}
			</div>
		</section>
	</div>
{/if}

<style>
	.controls-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
		gap: 0.9rem;
	}

	.control {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.control span,
	.label {
		font-size: 0.78rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted);
	}

	.control select,
	.control .value {
		min-height: 2.5rem;
		padding: 0.6rem 0.75rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.5rem;
		background: rgba(6, 10, 18, 0.8);
		color: var(--text);
		font-size: 0.92rem;
	}

	.control.static .value {
		display: flex;
		align-items: center;
	}

	.policy-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(20rem, 1fr));
		gap: 1rem;
	}

	.stack {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	.capability-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 0.75rem;
	}

	.capability {
		padding: 0.85rem;
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 0.5rem;
		background: rgba(255, 255, 255, 0.02);
	}

	.capability strong,
	.explanation-row strong {
		font-size: 0.95rem;
	}

	.field-list {
		display: grid;
		gap: 0.85rem;
	}

	.field-list p {
		margin: 0.35rem 0 0;
		font-size: 0.92rem;
	}

	.explanation-row {
		display: grid;
		gap: 0.25rem;
		padding: 0.75rem 0;
		border-top: 1px solid rgba(255, 255, 255, 0.08);
	}

	.explanation-row:first-child {
		padding-top: 0;
		border-top: 0;
	}

	@media (max-width: 720px) {
		.capability-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
