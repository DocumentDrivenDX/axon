<script lang="ts">
/**
 * TransactionFixtureEditor — structured editor for a transaction-operation
 * fixture array. Operators can add / remove / reorder operations and edit
 * each one (op type, collection, id, expectedVersion, data/patch fields)
 * without touching raw JSON.
 *
 * The component keeps its own $state array and serialises back to the parent
 * via the `onchange` prop, matching the shape that buildExplainInput expects:
 *
 *   [
 *     { readEntity:   { collection, id } },
 *     { createEntity: { collection, data } },
 *     { updateEntity: { collection, id, expectedVersion, data } },
 *     { patchEntity:  { collection, id, expectedVersion, patch } },
 *     { deleteEntity: { collection, id, expectedVersion } },
 *   ]
 */

export type TransactionOpKind =
	| 'readEntity'
	| 'createEntity'
	| 'updateEntity'
	| 'patchEntity'
	| 'deleteEntity';

export type TransactionOp = {
	/** Internal UI key – not serialised. */
	key: number;
	kind: TransactionOpKind;
	collection: string;
	id: string;
	expectedVersionText: string;
	dataText: string;
	patchText: string;
};

type Props = {
	/** Current serialised fixture JSON (initialisation only). */
	value: string;
	/** Increment when the parent intentionally reseeds the fixture from outside the editor. */
	resetKey: number;
	/** Scope defaults used when adding or recovering an operation. */
	defaultCollection?: string;
	defaultEntityId?: string;
	/** Called with updated serialised JSON whenever the ops change. */
	onchange: (json: string) => void;
};

const props: Props = $props();

let keyCounter = $state(0);

function makeOp(
	kind: TransactionOpKind = 'readEntity',
	seed: Partial<Pick<TransactionOp, 'collection' | 'id'>> = {},
): TransactionOp {
	return {
		key: keyCounter++,
		kind,
		collection: seed.collection ?? props.defaultCollection ?? '',
		id: kind === 'createEntity' ? '' : seed.id ?? props.defaultEntityId ?? '',
		expectedVersionText: '',
		dataText: '{}',
		patchText: '{}',
	};
}

function opFromRaw(raw: Record<string, unknown>): TransactionOp {
	const kind = (Object.keys(raw)[0] ?? 'readEntity') as TransactionOpKind;
	const payload = (raw[kind] ?? {}) as Record<string, unknown>;
	return {
		key: keyCounter++,
		kind,
		collection:
			typeof payload.collection === 'string' ? payload.collection : props.defaultCollection ?? '',
		id:
			typeof payload.id === 'string'
				? payload.id
				: kind === 'createEntity'
					? ''
					: props.defaultEntityId ?? '',
		expectedVersionText:
			typeof payload.expectedVersion === 'number' ? String(payload.expectedVersion) : '',
		dataText: payload.data !== undefined ? JSON.stringify(payload.data, null, 2) : '{}',
		patchText: payload.patch !== undefined ? JSON.stringify(payload.patch, null, 2) : '{}',
	};
}

function parseInitialValue(json: string): TransactionOp[] {
	try {
		const parsed = JSON.parse(json);
		if (!Array.isArray(parsed)) return [makeOp()];
		return parsed.length
			? parsed.map((item) => opFromRaw(item as Record<string, unknown>))
			: [makeOp()];
	} catch {
		return [makeOp()];
	}
}

let lastResetKey = $state<number | null>(null);
let ops = $state<TransactionOp[]>([]);

$effect.pre(() => {
	if (lastResetKey === props.resetKey) return;
	lastResetKey = props.resetKey;
	ops = parseInitialValue(props.value);
});

function serialise(currentOps: TransactionOp[]): string {
	const reference = currentOps.find((op) => op.collection || op.id);
	const arr = currentOps.map((op) => {
		const collection = op.collection || reference?.collection || props.defaultCollection;
		const payload: Record<string, unknown> = {
			collection: collection || undefined,
		};

		if (op.kind !== 'createEntity') {
			const id = op.id || reference?.id || props.defaultEntityId;
			if (id) payload.id = id;
		}

		const expVer = Number(op.expectedVersionText.trim());
		if (
			(op.kind === 'updateEntity' || op.kind === 'patchEntity' || op.kind === 'deleteEntity') &&
			op.expectedVersionText.trim() &&
			Number.isInteger(expVer)
		) {
			payload.expectedVersion = expVer;
		}

		if (op.kind === 'createEntity' || op.kind === 'updateEntity') {
			try {
				payload.data = JSON.parse(op.dataText);
			} catch {
				payload.data = {};
			}
		}
		if (op.kind === 'patchEntity') {
			try {
				payload.patch = JSON.parse(op.patchText);
			} catch {
				payload.patch = {};
			}
		}

		return { [op.kind]: payload };
	});
	return JSON.stringify(arr, null, 2);
}

function notify() {
	props.onchange(serialise(ops));
}

function newOperationSeed(): Pick<TransactionOp, 'collection' | 'id'> {
	const reference = ops.find((op) => op.collection || op.id);
	return {
		collection: reference?.collection || props.defaultCollection || '',
		id: reference?.id || props.defaultEntityId || '',
	};
}

function addOp() {
	ops = [...ops, makeOp('readEntity', newOperationSeed())];
	notify();
}

function removeOp(index: number) {
	ops = ops.filter((_, i) => i !== index);
	notify();
}

function moveUp(index: number) {
	if (index === 0) return;
	const next = [...ops];
	const above = next[index - 1];
	const current = next[index];
	if (!above || !current) return;
	next[index - 1] = current;
	next[index] = above;
	ops = next;
	notify();
}

function moveDown(index: number) {
	if (index === ops.length - 1) return;
	const next = [...ops];
	const current = next[index];
	const below = next[index + 1];
	if (!current || !below) return;
	next[index] = below;
	next[index + 1] = current;
	ops = next;
	notify();
}

function setKind(index: number, kind: TransactionOpKind) {
	ops = ops.map((op, i) => (i === index ? { ...op, kind } : op));
	notify();
}

function setField(index: number, field: keyof TransactionOp, val: string) {
	ops = ops.map((op, i) => (i === index ? { ...op, [field]: val } : op));
	notify();
}

const opKindOptions: Array<{ value: TransactionOpKind; label: string }> = [
	{ value: 'readEntity', label: 'readEntity' },
	{ value: 'createEntity', label: 'createEntity' },
	{ value: 'updateEntity', label: 'updateEntity' },
	{ value: 'patchEntity', label: 'patchEntity' },
	{ value: 'deleteEntity', label: 'deleteEntity' },
];

function needsId(kind: TransactionOpKind): boolean {
	return kind !== 'createEntity';
}

function needsExpectedVersion(kind: TransactionOpKind): boolean {
	return kind === 'updateEntity' || kind === 'patchEntity' || kind === 'deleteEntity';
}

function needsData(kind: TransactionOpKind): boolean {
	return kind === 'createEntity' || kind === 'updateEntity';
}

function needsPatch(kind: TransactionOpKind): boolean {
	return kind === 'patchEntity';
}
</script>

<div class="txn-editor" data-testid="transaction-fixture-editor">
	{#if ops.length === 0}
		<p class="muted">No operations. Add one below.</p>
	{/if}

	{#each ops as op, index (op.key)}
		<div class="txn-op" data-testid="transaction-fixture-op" data-op-index={index} data-op-kind={op.kind}>
			<div class="txn-op-header">
				<span class="txn-op-num">#{index + 1}</span>

				<select
					aria-label="Operation kind"
					data-testid="transaction-fixture-op-kind"
					value={op.kind}
					onchange={(e) => setKind(index, (e.currentTarget as HTMLSelectElement).value as TransactionOpKind)}
				>
					{#each opKindOptions as opt}
						<option value={opt.value}>{opt.label}</option>
					{/each}
				</select>

				<div class="txn-op-actions">
					<button
						type="button"
						class="txn-btn"
						aria-label="Move operation up"
						data-testid="transaction-fixture-op-move-up"
						disabled={index === 0}
						onclick={() => moveUp(index)}
					>
						&#x2191;
					</button>
					<button
						type="button"
						class="txn-btn"
						aria-label="Move operation down"
						data-testid="transaction-fixture-op-move-down"
						disabled={index === ops.length - 1}
						onclick={() => moveDown(index)}
					>
						&#x2193;
					</button>
					<button
						type="button"
						class="txn-btn txn-btn-remove"
						aria-label="Remove operation"
						data-testid="transaction-fixture-op-remove"
						onclick={() => removeOp(index)}
					>
						&#x2715;
					</button>
				</div>
			</div>

			<div class="txn-op-fields">
				<label class="txn-field">
					<span>Collection</span>
					<input
						type="text"
						placeholder="e.g. invoices"
						aria-label="Collection"
						data-testid="transaction-fixture-op-collection"
						value={op.collection}
						oninput={(e) => setField(index, 'collection', (e.currentTarget as HTMLInputElement).value)}
					/>
				</label>

				{#if needsId(op.kind)}
					<label class="txn-field">
						<span>Entity ID</span>
						<input
							type="text"
							placeholder="UUID or string ID"
							aria-label="Entity ID"
							data-testid="transaction-fixture-op-id"
							value={op.id}
							oninput={(e) => setField(index, 'id', (e.currentTarget as HTMLInputElement).value)}
						/>
					</label>
				{/if}

				{#if needsExpectedVersion(op.kind)}
					<label class="txn-field">
						<span>Expected version</span>
						<input
							type="text"
							inputmode="numeric"
							placeholder="e.g. 1"
							aria-label="Expected version"
							data-testid="transaction-fixture-op-expected-version"
							value={op.expectedVersionText}
							oninput={(e) => setField(index, 'expectedVersionText', (e.currentTarget as HTMLInputElement).value)}
						/>
					</label>
				{/if}

				{#if needsData(op.kind)}
					<label class="txn-field txn-field-wide">
						<span>Data (JSON)</span>
						<textarea
							rows="5"
							spellcheck="false"
							aria-label="Data JSON"
							data-testid="transaction-fixture-op-data"
							value={op.dataText}
							oninput={(e) => setField(index, 'dataText', (e.currentTarget as HTMLTextAreaElement).value)}
						></textarea>
					</label>
				{/if}

				{#if needsPatch(op.kind)}
					<label class="txn-field txn-field-wide">
						<span>Patch (JSON)</span>
						<textarea
							rows="5"
							spellcheck="false"
							aria-label="Patch JSON"
							data-testid="transaction-fixture-op-patch"
							value={op.patchText}
							oninput={(e) => setField(index, 'patchText', (e.currentTarget as HTMLTextAreaElement).value)}
						></textarea>
					</label>
				{/if}
			</div>
		</div>
	{/each}

	<div class="txn-footer">
		<button
			type="button"
			data-testid="transaction-fixture-add-op"
			onclick={addOp}
		>
			+ Add operation
		</button>
	</div>
</div>

<style>
	.txn-editor {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.txn-op {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
		padding: 0.75rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.5rem;
		background: rgba(255, 255, 255, 0.02);
	}

	.txn-op-header {
		display: flex;
		align-items: center;
		gap: 0.6rem;
	}

	.txn-op-num {
		font-size: 0.78rem;
		font-weight: 600;
		color: var(--muted);
		min-width: 1.5rem;
	}

	.txn-op-header select {
		flex: 1;
		min-height: 2rem;
		padding: 0.35rem 0.6rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.4rem;
		background: rgba(6, 10, 18, 0.8);
		color: var(--text);
		font-size: 0.88rem;
	}

	.txn-op-actions {
		display: flex;
		gap: 0.3rem;
		margin-left: auto;
	}

	.txn-btn {
		padding: 0.25rem 0.5rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.35rem;
		background: rgba(255, 255, 255, 0.04);
		color: var(--text);
		font-size: 0.82rem;
		cursor: pointer;
	}

	.txn-btn:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}

	.txn-btn-remove {
		color: var(--error, #e05);
	}

	.txn-op-fields {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr));
		gap: 0.6rem;
	}

	.txn-field {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
	}

	.txn-field span {
		font-size: 0.75rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted);
	}

	.txn-field input,
	.txn-field textarea {
		padding: 0.45rem 0.65rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.4rem;
		background: rgba(6, 10, 18, 0.8);
		color: var(--text);
		font-size: 0.88rem;
		font-family: monospace;
		resize: vertical;
	}

	.txn-field-wide {
		grid-column: 1 / -1;
	}

	.txn-footer {
		display: flex;
		justify-content: flex-start;
	}

	.txn-footer button {
		padding: 0.45rem 0.85rem;
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 0.4rem;
		background: rgba(255, 255, 255, 0.04);
		color: var(--text);
		font-size: 0.88rem;
		cursor: pointer;
	}
</style>
