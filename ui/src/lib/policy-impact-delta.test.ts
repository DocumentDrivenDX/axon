import { describe, expect, test } from 'bun:test';

import type { ImpactCell, ImpactDecisionKind, WorkspaceDiagnostic } from './policy-evaluator';
import { computeCellDelta } from './policy-impact-delta';

const diagnostic = (code: string): WorkspaceDiagnostic => ({
	source: 'schema',
	code,
	summary: `${code} summary`,
	missingField: null,
	remediation: `${code} remediation`,
});

const cell = (overrides: Partial<ImpactCell> = {}): ImpactCell => ({
	subjectId: 'subject-1',
	operation: 'read',
	entityId: 'entity-1',
	decision: 'allowed',
	reason: 'ok',
	redactedFields: [],
	deniedFields: [],
	approvalRole: null,
	diagnostic: null,
	explainHref: null,
	...overrides,
});

describe('computeCellDelta', () => {
	test('marks identical cells as unchanged with no changed flags', () => {
		const delta = computeCellDelta(cell(), cell());

		expect(delta).toEqual({
			decisionChanged: false,
			redactedFieldsChanged: false,
			deniedFieldsChanged: false,
			approvalRoleChanged: false,
			diagnosticCodeChanged: false,
			isUnchanged: true,
			onlyActive: false,
			onlyProposed: false,
		});
	});

	test('marks allow to deny decision changes', () => {
		const delta = computeCellDelta(cell({ decision: 'allowed' }), cell({ decision: 'denied' }));

		expect(delta.decisionChanged).toBe(true);
		expect(delta.isUnchanged).toBe(false);
	});

	test('marks deny to allow decision changes', () => {
		const delta = computeCellDelta(cell({ decision: 'denied' }), cell({ decision: 'allowed' }));

		expect(delta.decisionChanged).toBe(true);
	});

	test('marks deny to needs approval decision changes', () => {
		const delta = computeCellDelta(
			cell({ decision: 'denied' }),
			cell({ decision: 'needs_approval' }),
		);

		expect(delta.decisionChanged).toBe(true);
	});

	test('marks added redacted fields as changed', () => {
		const delta = computeCellDelta(
			cell({ redactedFields: ['amount_cents'] }),
			cell({ redactedFields: ['amount_cents', 'commercial_terms'] }),
		);

		expect(delta.redactedFieldsChanged).toBe(true);
	});

	test('marks removed redacted fields as changed', () => {
		const delta = computeCellDelta(
			cell({ redactedFields: ['amount_cents'] }),
			cell({ redactedFields: [] }),
		);

		expect(delta.redactedFieldsChanged).toBe(true);
	});

	test('treats reordered redacted fields as unchanged', () => {
		const delta = computeCellDelta(
			cell({ redactedFields: ['a', 'b'] }),
			cell({ redactedFields: ['b', 'a'] }),
		);

		expect(delta.redactedFieldsChanged).toBe(false);
		expect(delta.isUnchanged).toBe(true);
	});

	test('marks added and removed denied fields as changed', () => {
		const added = computeCellDelta(
			cell({ deniedFields: ['amount_cents'] }),
			cell({ deniedFields: ['amount_cents', 'commercial_terms'] }),
		);
		const removed = computeCellDelta(
			cell({ deniedFields: ['amount_cents'] }),
			cell({ deniedFields: [] }),
		);

		expect(added.deniedFieldsChanged).toBe(true);
		expect(removed.deniedFieldsChanged).toBe(true);
	});

	test('treats reordered denied fields as unchanged', () => {
		const delta = computeCellDelta(
			cell({ deniedFields: ['a', 'b'] }),
			cell({ deniedFields: ['b', 'a'] }),
		);

		expect(delta.deniedFieldsChanged).toBe(false);
		expect(delta.isUnchanged).toBe(true);
	});

	test('marks null to reviewer approval role changes', () => {
		const delta = computeCellDelta(
			cell({ approvalRole: null }),
			cell({ approvalRole: 'reviewer' }),
		);

		expect(delta.approvalRoleChanged).toBe(true);
	});

	test('marks policy_filter_unindexed diagnostic remediation as changed', () => {
		const delta = computeCellDelta(
			cell({ diagnostic: diagnostic('policy_filter_unindexed') }),
			cell({ diagnostic: null }),
		);

		expect(delta.diagnosticCodeChanged).toBe(true);
	});

	test('marks policy_filter_unindexed diagnostic regression as changed', () => {
		const delta = computeCellDelta(
			cell({ diagnostic: null }),
			cell({ diagnostic: diagnostic('policy_filter_unindexed') }),
		);

		expect(delta.diagnosticCodeChanged).toBe(true);
	});

	test('marks active-only cells when proposed is missing', () => {
		const delta = computeCellDelta(cell(), null);

		expect(delta.onlyActive).toBe(true);
		expect(delta.onlyProposed).toBe(false);
		expect(delta.isUnchanged).toBe(false);
	});

	test('marks proposed-only cells when active is missing', () => {
		const delta = computeCellDelta(null, cell());

		expect(delta.onlyActive).toBe(false);
		expect(delta.onlyProposed).toBe(true);
		expect(delta.isUnchanged).toBe(false);
	});

	test('keeps both-null cells in the degenerate changed state', () => {
		const delta = computeCellDelta(null, null);

		expect(delta.onlyActive).toBe(false);
		expect(delta.onlyProposed).toBe(false);
		expect(delta.isUnchanged).toBe(false);
	});

	test('does not mutate frozen inputs while comparing deduplicated field sets', () => {
		const active = cell({
			decision: 'denied' satisfies ImpactDecisionKind,
			redactedFields: ['amount_cents', 'amount_cents'],
			deniedFields: ['commercial_terms', 'amount_cents'],
			diagnostic: diagnostic('policy_filter_unindexed'),
		});
		const proposed = cell({
			decision: 'denied' satisfies ImpactDecisionKind,
			redactedFields: ['amount_cents'],
			deniedFields: ['amount_cents', 'commercial_terms'],
			diagnostic: diagnostic('policy_filter_unindexed'),
		});
		Object.freeze(active.redactedFields);
		Object.freeze(active.deniedFields);
		Object.freeze(active.diagnostic);
		Object.freeze(active);
		Object.freeze(proposed.redactedFields);
		Object.freeze(proposed.deniedFields);
		Object.freeze(proposed.diagnostic);
		Object.freeze(proposed);

		const delta = computeCellDelta(active, proposed);

		expect(delta.redactedFieldsChanged).toBe(false);
		expect(delta.deniedFieldsChanged).toBe(false);
		expect(delta.isUnchanged).toBe(true);
		expect(active.redactedFields).toEqual(['amount_cents', 'amount_cents']);
		expect(active.deniedFields).toEqual(['commercial_terms', 'amount_cents']);
		expect(proposed.deniedFields).toEqual(['amount_cents', 'commercial_terms']);
	});
});
