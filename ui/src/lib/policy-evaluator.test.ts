import { describe, expect, test } from 'bun:test';
import type {
	CollectionDetail,
	CollectionSummary,
	EffectiveCollectionPolicy,
	EntityRecord,
	PolicyExplainDiagnostic,
	PolicyExplainResult,
	PolicyExplanation,
} from './api';
import {
	type BuildExplainInputArgs,
	EFFECTIVE_POLICY_CONSOLE_QUERY,
	EXPLAIN_POLICY_CONSOLE_QUERY,
	IMPACT_MATRIX_OPERATIONS,
	buildEffectiveConsolePreset,
	buildExplainConsolePreset,
	buildExplainInput,
	buildGraphqlConsolePreset,
	buildGraphqlDiagnostics,
	buildImpactMatrixInputs,
	buildMcpEnvelopePreview,
	buildSchemaDiagnostics,
	dedupeDiagnostics,
	defaultCollectionName,
	defaultPatchFixture,
	defaultTransactionFixture,
	errorMessage,
	formatDiagnosticError,
	formatFields,
	formatMcpReproduction,
	mcpOutcomeFromExplanation,
	mcpToolNameForOperation,
	operationRequiresEntity,
	operationRequiresExpectedVersion,
	parseJsonFixture,
	parseOptionalInteger,
	prettyJson,
	resolveImpactCell,
	tryBuildExplainInput,
} from './policy-evaluator';

const summary = (name: string): CollectionSummary => ({
	name,
	entity_count: 0,
	schema_version: 1,
});

const entity = (overrides: Partial<EntityRecord> = {}): EntityRecord => ({
	collection: 'invoices',
	id: 'inv-1',
	version: 3,
	data: { amount_cents: 1000 },
	...overrides,
});

const baseExplainArgs = (
	overrides: Partial<BuildExplainInputArgs> = {},
): BuildExplainInputArgs => ({
	operation: 'read',
	collection: 'invoices',
	entity: entity(),
	expectedVersionText: '',
	rollbackVersionText: '',
	lifecycleName: 'status',
	targetState: 'approved',
	dataFixtureText: '{}',
	patchFixtureText: '{}',
	transactionFixtureText: '[]',
	...overrides,
});

describe('prettyJson', () => {
	test('serialises with two-space indent', () => {
		expect(prettyJson({ a: 1 })).toBe('{\n  "a": 1\n}');
	});
	test('treats null/undefined as empty object', () => {
		expect(prettyJson(null)).toBe('{}');
		expect(prettyJson(undefined)).toBe('{}');
	});
});

describe('formatFields', () => {
	test('joins non-empty list', () => {
		expect(formatFields(['a', 'b'])).toBe('a, b');
	});
	test('returns "None" when empty', () => {
		expect(formatFields([])).toBe('None');
	});
});

describe('errorMessage', () => {
	test('uses Error.message', () => {
		expect(errorMessage(new Error('boom'), 'fallback')).toBe('boom');
	});
	test('falls back for non-Error values', () => {
		expect(errorMessage('nope', 'fallback')).toBe('fallback');
		expect(errorMessage(undefined, 'fallback')).toBe('fallback');
	});
});

describe('defaultCollectionName', () => {
	test('prefers invoices, then task, then expense', () => {
		expect(defaultCollectionName([summary('users'), summary('invoices'), summary('task')])).toBe(
			'invoices',
		);
		expect(defaultCollectionName([summary('users'), summary('task')])).toBe('task');
		expect(defaultCollectionName([summary('users'), summary('expense')])).toBe('expense');
	});
	test('falls back to first non-user collection', () => {
		expect(defaultCollectionName([summary('users'), summary('orders')])).toBe('orders');
	});
	test('falls back to first collection when only users exist', () => {
		expect(defaultCollectionName([summary('users')])).toBe('users');
	});
	test('returns empty string for empty list', () => {
		expect(defaultCollectionName([])).toBe('');
	});
});

describe('parseOptionalInteger', () => {
	test('returns null for blank input', () => {
		expect(parseOptionalInteger('', 'V')).toBeNull();
		expect(parseOptionalInteger('   ', 'V')).toBeNull();
	});
	test('parses non-negative integers', () => {
		expect(parseOptionalInteger('0', 'V')).toBe(0);
		expect(parseOptionalInteger('42', 'V')).toBe(42);
	});
	test('throws for negative or fractional input', () => {
		expect(() => parseOptionalInteger('-1', 'Expected version')).toThrow(
			'Expected version must be a non-negative integer',
		);
		expect(() => parseOptionalInteger('1.5', 'Expected version')).toThrow();
		expect(() => parseOptionalInteger('abc', 'Expected version')).toThrow();
	});
});

describe('parseJsonFixture', () => {
	test('parses valid JSON', () => {
		expect(parseJsonFixture('{"a":1}', 'F')).toEqual({ a: 1 });
		expect(parseJsonFixture('[1,2]', 'F')).toEqual([1, 2]);
	});
	test('throws with the supplied label on invalid JSON', () => {
		expect(() => parseJsonFixture('{not json', 'Patch fixture')).toThrow(
			'Patch fixture must be valid JSON',
		);
	});
});

describe('defaultPatchFixture', () => {
	test('bumps amount_cents when present', () => {
		expect(defaultPatchFixture(entity({ data: { amount_cents: 100 } }))).toEqual({
			amount_cents: 500_100,
		});
	});
	test('bumps budget_cents when amount missing', () => {
		expect(defaultPatchFixture(entity({ data: { budget_cents: 1000 } }))).toEqual({
			budget_cents: 16_000,
		});
	});
	test('toggles status approved/draft', () => {
		expect(defaultPatchFixture(entity({ data: { status: 'approved' } }))).toEqual({
			status: 'draft',
		});
		expect(defaultPatchFixture(entity({ data: { status: 'pending' } }))).toEqual({
			status: 'approved',
		});
	});
	test('appends dry-run marker to title fallback', () => {
		expect(defaultPatchFixture(entity({ data: { title: 'Q1' } }))).toEqual({
			title: 'Q1 (policy dry-run)',
		});
	});
	test('returns empty object when nothing matches', () => {
		expect(defaultPatchFixture(null)).toEqual({});
		expect(defaultPatchFixture(entity({ data: { other: 'x' } }))).toEqual({});
	});
});

describe('defaultTransactionFixture', () => {
	test('returns empty array when entity is null', () => {
		expect(defaultTransactionFixture(null)).toEqual([]);
	});
	test('produces an updateEntity envelope from the entity', () => {
		const e = entity({ id: 'a', version: 7, data: { amount_cents: 1 } });
		expect(defaultTransactionFixture(e)).toEqual([
			{
				updateEntity: {
					collection: 'invoices',
					id: 'a',
					expectedVersion: 7,
					data: { amount_cents: 1 },
				},
			},
		]);
	});
});

describe('operation predicates', () => {
	test('operationRequiresEntity', () => {
		for (const op of ['update', 'patch', 'delete', 'transition', 'rollback'] as const) {
			expect(operationRequiresEntity(op)).toBe(true);
		}
		for (const op of ['read', 'create', 'transaction'] as const) {
			expect(operationRequiresEntity(op)).toBe(false);
		}
	});
	test('operationRequiresExpectedVersion', () => {
		for (const op of ['update', 'patch', 'delete', 'transition'] as const) {
			expect(operationRequiresExpectedVersion(op)).toBe(true);
		}
		for (const op of ['read', 'create', 'rollback', 'transaction'] as const) {
			expect(operationRequiresExpectedVersion(op)).toBe(false);
		}
	});
});

describe('buildSchemaDiagnostics', () => {
	const detail = (schema: unknown): CollectionDetail => ({
		name: 'invoices',
		entity_count: 0,
		schema: schema as CollectionDetail['schema'],
	});

	test('returns no diagnostics for non-read operations', () => {
		expect(
			buildSchemaDiagnostics(
				detail({
					collection: 'invoices',
					version: 1,
					access_control: { rules: [{ where: { field: 'team_id' } }] },
					indexes: [],
				}),
				'create',
			),
		).toEqual([]);
	});

	test('returns no diagnostics when schema is null', () => {
		expect(buildSchemaDiagnostics(null, 'read')).toEqual([]);
	});

	test('flags unindexed where fields and skips indexed ones', () => {
		const result = buildSchemaDiagnostics(
			detail({
				collection: 'invoices',
				version: 1,
				access_control: {
					rules: [{ where: { field: 'team_id' } }, { where: { field: 'status' } }],
				},
				indexes: [{ field: 'team_id' }],
			}),
			'read',
		);
		expect(result).toHaveLength(1);
		const [first] = result;
		expect(first).toMatchObject({
			source: 'schema',
			code: 'policy_filter_unindexed',
			missingField: 'status',
		});
		expect(first?.summary).toContain('"status"');
		expect(first?.remediation).toContain('"status"');
	});

	test('walks nested arrays and objects to find where clauses', () => {
		const result = buildSchemaDiagnostics(
			detail({
				collection: 'invoices',
				version: 1,
				access_control: {
					rules: [
						{
							effect: 'allow',
							conditions: [{ where: { field: 'owner_id' } }],
						},
					],
				},
				indexes: [],
			}),
			'read',
		);
		expect(result).toHaveLength(1);
		expect(result[0]?.missingField).toBe('owner_id');
	});
});

describe('buildGraphqlDiagnostics', () => {
	const baseDiag = (overrides: Partial<PolicyExplainDiagnostic> = {}): PolicyExplainDiagnostic => ({
		code: 'POLICY_FILTER_UNINDEXED',
		message: 'denied',
		detail: {
			reason: 'policy_filter_unindexed',
			missing_index: 'team_id',
			collection: 'invoices',
		},
		...overrides,
	});

	test('skips diagnostics that are not policy_filter_unindexed', () => {
		expect(
			buildGraphqlDiagnostics([
				{ code: 'OTHER', message: 'no', detail: { reason: 'something_else' } },
			]),
		).toEqual([]);
	});

	test('matches by detail.reason', () => {
		const result = buildGraphqlDiagnostics([
			{
				code: null,
				message: 'denied',
				detail: { reason: 'policy_filter_unindexed', missing_index: 'team_id' },
			},
		]);
		expect(result).toHaveLength(1);
		expect(result[0]?.missingField).toBe('team_id');
	});

	test('matches by uppercase code', () => {
		const result = buildGraphqlDiagnostics([baseDiag()]);
		expect(result).toHaveLength(1);
		const [first] = result;
		expect(first?.source).toBe('graphql');
		expect(first?.summary).toContain('invoices');
		expect(first?.summary).toContain('"team_id"');
	});

	test('includes saturation suffix when candidate_count and cost_limit are present', () => {
		const result = buildGraphqlDiagnostics([
			baseDiag({
				detail: {
					reason: 'policy_filter_unindexed',
					missing_index: 'team_id',
					collection: 'invoices',
					candidate_count: 1500,
					cost_limit: 1000,
				},
			}),
		]);
		expect(result[0]?.summary).toContain('Candidate set 1500 exceeded the limit 1000.');
	});

	test('falls back to generic remediation when missing_index is absent', () => {
		const result = buildGraphqlDiagnostics([
			baseDiag({
				detail: { reason: 'policy_filter_unindexed', collection: 'invoices' },
			}),
		]);
		const [first] = result;
		expect(first?.missingField).toBeNull();
		expect(first?.remediation).toBe(
			'Add the missing index or simplify the policy filter before retrying.',
		);
	});
});

describe('dedupeDiagnostics', () => {
	test('collapses duplicates by code/missingField/source tuple', () => {
		const a = {
			source: 'schema' as const,
			code: 'policy_filter_unindexed',
			summary: 'a',
			missingField: 'team_id',
			remediation: 'r',
		};
		const result = dedupeDiagnostics([a, a, { ...a, source: 'graphql' }]);
		expect(result).toHaveLength(2);
		expect(result.map((d) => d.source)).toEqual(['schema', 'graphql']);
	});
});

describe('formatDiagnosticError', () => {
	test('prepends code when present', () => {
		expect(formatDiagnosticError({ code: 'X', message: 'denied', detail: null })).toBe('X: denied');
	});
	test('returns plain message when code is null', () => {
		expect(formatDiagnosticError({ code: null, message: 'denied', detail: null })).toBe('denied');
	});
});

describe('buildExplainInput', () => {
	test('read operation includes entityId when entity supplied', () => {
		expect(buildExplainInput(baseExplainArgs())).toEqual({
			operation: 'read',
			collection: 'invoices',
			entityId: 'inv-1',
		});
	});

	test('omits collection when blank', () => {
		expect(
			buildExplainInput(baseExplainArgs({ collection: '', entity: null, operation: 'read' })),
		).toEqual({ operation: 'read' });
	});

	test('throws when entity is required but missing', () => {
		expect(() => buildExplainInput(baseExplainArgs({ operation: 'update', entity: null }))).toThrow(
			'Select an entity before running this evaluator operation',
		);
	});

	test('parses expected version when supplied for update', () => {
		const input = buildExplainInput(
			baseExplainArgs({
				operation: 'update',
				expectedVersionText: '7',
				dataFixtureText: '{"amount_cents":2000}',
			}),
		);
		expect(input).toEqual({
			operation: 'update',
			collection: 'invoices',
			entityId: 'inv-1',
			expectedVersion: 7,
			data: { amount_cents: 2000 },
		});
	});

	test('create drops entityId/expectedVersion and uses data fixture', () => {
		expect(
			buildExplainInput(
				baseExplainArgs({
					operation: 'create',
					entity: null,
					dataFixtureText: '{"title":"x"}',
				}),
			),
		).toEqual({
			operation: 'create',
			collection: 'invoices',
			data: { title: 'x' },
		});
	});

	test('patch uses patch fixture', () => {
		expect(
			buildExplainInput(baseExplainArgs({ operation: 'patch', patchFixtureText: '{"a":1}' })),
		).toMatchObject({
			operation: 'patch',
			patch: { a: 1 },
		});
	});

	test('transition requires lifecycleName and targetState', () => {
		expect(() =>
			buildExplainInput(
				baseExplainArgs({ operation: 'transition', lifecycleName: '   ', targetState: 'a' }),
			),
		).toThrow('Lifecycle name is required for transition evaluation');
		expect(() =>
			buildExplainInput(
				baseExplainArgs({ operation: 'transition', lifecycleName: 'status', targetState: '' }),
			),
		).toThrow('Target state is required for transition evaluation');
	});

	test('transition trims lifecycle and target', () => {
		expect(
			buildExplainInput(
				baseExplainArgs({
					operation: 'transition',
					lifecycleName: '  status  ',
					targetState: '  approved  ',
				}),
			),
		).toMatchObject({
			operation: 'transition',
			lifecycleName: 'status',
			targetState: 'approved',
		});
	});

	test('rollback applies optional toVersion', () => {
		expect(
			buildExplainInput(baseExplainArgs({ operation: 'rollback', rollbackVersionText: '2' })),
		).toMatchObject({ operation: 'rollback', toVersion: 2 });
		const noVersion = buildExplainInput(
			baseExplainArgs({ operation: 'rollback', rollbackVersionText: '' }),
		);
		expect(noVersion.toVersion).toBeUndefined();
	});

	test('transaction parses operations and bypasses entity rules', () => {
		const result = buildExplainInput(
			baseExplainArgs({
				operation: 'transaction',
				entity: null,
				transactionFixtureText: '[{"updateEntity":{"id":"a"}}]',
			}),
		);
		expect(result).toEqual({
			operation: 'transaction',
			operations: [{ updateEntity: { id: 'a' } }],
		});
	});

	test('transaction rejects non-array fixture', () => {
		expect(() =>
			buildExplainInput(
				baseExplainArgs({ operation: 'transaction', transactionFixtureText: '{}' }),
			),
		).toThrow('Transaction fixture must be a JSON array');
	});
});

describe('tryBuildExplainInput', () => {
	test('returns input on success', () => {
		const result = tryBuildExplainInput(baseExplainArgs());
		expect(result.error).toBeNull();
		expect(result.input).not.toBeNull();
	});
	test('captures the thrown error message', () => {
		const result = tryBuildExplainInput(baseExplainArgs({ operation: 'update', entity: null }));
		expect(result.input).toBeNull();
		expect(result.error).toBe('Select an entity before running this evaluator operation');
	});
});

describe('console preset builders', () => {
	const ctx = { baseHref: '/base/graphql', subject: 'finance-agent' };

	test('buildGraphqlConsolePreset encodes preset, query, variables, actor', () => {
		const result = buildGraphqlConsolePreset(ctx, 'effectivePolicy', 'QUERY', { a: 1 });
		expect(result.preset).toBe('effectivePolicy');
		expect(result.href.startsWith('/base/graphql?')).toBe(true);
		const url = new URL(result.href, 'http://example.test');
		expect(url.searchParams.get('preset')).toBe('effectivePolicy');
		expect(url.searchParams.get('query')).toBe('QUERY');
		expect(JSON.parse(url.searchParams.get('variables') ?? '{}')).toEqual({ a: 1 });
		expect(url.searchParams.get('actor')).toBe('finance-agent');
	});

	test('omits actor when subject blank', () => {
		const result = buildGraphqlConsolePreset(
			{ baseHref: '/base/graphql', subject: '' },
			'p',
			'q',
			{},
		);
		const url = new URL(result.href, 'http://example.test');
		expect(url.searchParams.has('actor')).toBe(false);
	});

	test('buildEffectiveConsolePreset returns null without a collection', () => {
		expect(buildEffectiveConsolePreset(ctx, '', null)).toBeNull();
	});

	test('buildEffectiveConsolePreset embeds collection and entityId', () => {
		const result = buildEffectiveConsolePreset(ctx, 'invoices', entity());
		expect(result).not.toBeNull();
		const variables = JSON.parse(
			new URL(result?.href ?? '', 'http://example.test').searchParams.get('variables') ?? '{}',
		);
		expect(variables).toEqual({ collection: 'invoices', entityId: 'inv-1' });
	});

	test('buildEffectiveConsolePreset uses null entityId when entity is null', () => {
		const result = buildEffectiveConsolePreset(ctx, 'invoices', null);
		const variables = JSON.parse(
			new URL(result?.href ?? '', 'http://example.test').searchParams.get('variables') ?? '{}',
		);
		expect(variables).toEqual({ collection: 'invoices', entityId: null });
	});

	test('buildExplainConsolePreset returns null when input is null', () => {
		expect(buildExplainConsolePreset(ctx, null)).toBeNull();
	});

	test('buildExplainConsolePreset embeds the input variable', () => {
		const result = buildExplainConsolePreset(ctx, {
			operation: 'read',
			collection: 'invoices',
		});
		const variables = JSON.parse(
			new URL(result?.href ?? '', 'http://example.test').searchParams.get('variables') ?? '{}',
		);
		expect(variables).toEqual({ input: { operation: 'read', collection: 'invoices' } });
	});
});

describe('console query constants', () => {
	test('effective policy query references effectivePolicy field', () => {
		expect(EFFECTIVE_POLICY_CONSOLE_QUERY).toContain('effectivePolicy(');
	});
	test('explain policy query references explainPolicy field', () => {
		expect(EXPLAIN_POLICY_CONSOLE_QUERY).toContain('explainPolicy(');
	});
});

describe('buildImpactMatrixInputs', () => {
	const subjects = [
		{ id: 'finance-agent', label: 'Finance', detail: null },
		{ id: 'contractor', label: 'Contractor', detail: null },
	];
	const entities = [
		entity({ id: 'inv-1', version: 3, data: { amount_cents: 100 } }),
		entity({ id: 'inv-2', version: 5, data: { status: 'approved' } }),
	];

	test('returns an empty list when collection is blank', () => {
		expect(buildImpactMatrixInputs('', entities, subjects)).toEqual([]);
	});

	test('produces one request per (entity, subject, operation) tuple', () => {
		const result = buildImpactMatrixInputs('invoices', entities, subjects);
		expect(result).toHaveLength(
			entities.length * subjects.length * IMPACT_MATRIX_OPERATIONS.length,
		);
		const seen = new Set(result.map((r) => `${r.entity.id}|${r.subjectId}|${r.operation}`));
		expect(seen.size).toBe(result.length);
	});

	test('default ops are read/patch/delete with appropriate input shape', () => {
		const firstEntity = entities[0];
		const firstSubject = subjects[0];
		expect(firstEntity).toBeDefined();
		expect(firstSubject).toBeDefined();
		if (!firstEntity || !firstSubject) return;
		const result = buildImpactMatrixInputs('invoices', [firstEntity], [firstSubject]);
		const byOp = Object.fromEntries(result.map((r) => [r.operation, r.explainInput]));
		expect(byOp.read).toEqual({ operation: 'read', collection: 'invoices', entityId: 'inv-1' });
		expect(byOp.delete).toEqual({
			operation: 'delete',
			collection: 'invoices',
			entityId: 'inv-1',
			expectedVersion: 3,
		});
		expect(byOp.patch).toMatchObject({
			operation: 'patch',
			collection: 'invoices',
			entityId: 'inv-1',
			expectedVersion: 3,
		});
		expect(byOp.patch?.patch).toEqual({ amount_cents: 500_100 });
	});

	test('honours a custom operations list', () => {
		const firstEntity = entities[0];
		const firstSubject = subjects[0];
		expect(firstEntity).toBeDefined();
		expect(firstSubject).toBeDefined();
		if (!firstEntity || !firstSubject) return;
		const result = buildImpactMatrixInputs('invoices', [firstEntity], [firstSubject], ['update']);
		expect(result).toHaveLength(1);
		expect(result[0]?.explainInput).toEqual({
			operation: 'update',
			collection: 'invoices',
			entityId: 'inv-1',
			expectedVersion: 3,
			data: { amount_cents: 100 },
		});
	});
});

describe('resolveImpactCell', () => {
	const ctx = { baseHref: '/base/graphql', subject: 'finance-agent' };
	const baseRequest = {
		subjectId: 'finance-agent',
		operation: 'read' as const,
		entity: entity(),
		explainInput: {
			operation: 'read' as const,
			collection: 'invoices',
			entityId: 'inv-1',
		},
	};

	const explanation = (overrides: Partial<PolicyExplanation> = {}): PolicyExplanation => ({
		operation: 'read',
		decision: 'allowed',
		reason: 'allowed',
		policyVersion: 1,
		ruleIds: [],
		policyIds: [],
		fieldPaths: [],
		deniedFields: [],
		rules: [],
		approval: null,
		operations: [],
		...overrides,
	});

	const effective = (
		overrides: Partial<EffectiveCollectionPolicy> = {},
	): EffectiveCollectionPolicy => ({
		collection: 'invoices',
		canRead: true,
		canCreate: true,
		canUpdate: true,
		canDelete: true,
		redactedFields: [],
		deniedFields: [],
		policyVersion: 1,
		...overrides,
	});

	const result = (overrides: Partial<PolicyExplainResult> = {}): PolicyExplainResult => ({
		explanation: null,
		diagnostics: [],
		...overrides,
	});

	test('maps allowed explanation to allowed decision and pulls redactedFields from effective', () => {
		const cell = resolveImpactCell({
			request: baseRequest,
			explainResult: result({ explanation: explanation({ decision: 'allowed', reason: 'ok' }) }),
			effective: effective({ redactedFields: ['amount_cents', 'commercial_terms'] }),
			presetCtxForSubject: ctx,
		});
		expect(cell.decision).toBe('allowed');
		expect(cell.reason).toBe('ok');
		expect(cell.redactedFields).toEqual(['amount_cents', 'commercial_terms']);
		expect(cell.explainHref).toContain('/base/graphql?');
		expect(cell.explainHref).toContain('preset=explainPolicy');
	});

	test('normalises server allow/deny to allowed/denied', () => {
		const allowed = resolveImpactCell({
			request: baseRequest,
			explainResult: result({ explanation: explanation({ decision: 'allow', reason: 'ok' }) }),
			effective: null,
			presetCtxForSubject: ctx,
		});
		expect(allowed.decision).toBe('allowed');

		const denied = resolveImpactCell({
			request: baseRequest,
			explainResult: result({
				explanation: explanation({ decision: 'deny', reason: 'forbidden' }),
			}),
			effective: null,
			presetCtxForSubject: ctx,
		});
		expect(denied.decision).toBe('denied');
	});

	test('maps needs_approval and surfaces approval role', () => {
		const cell = resolveImpactCell({
			request: baseRequest,
			explainResult: result({
				explanation: explanation({
					decision: 'needs_approval',
					reason: 'large_invoice',
					approval: {
						name: 'finance-approver',
						decision: 'pending',
						role: 'finance_approver',
					},
				}),
			}),
			effective: null,
			presetCtxForSubject: ctx,
		});
		expect(cell.decision).toBe('needs_approval');
		expect(cell.approvalRole).toBe('finance_approver');
	});

	test('maps denied explanation and includes deniedFields', () => {
		const cell = resolveImpactCell({
			request: baseRequest,
			explainResult: result({
				explanation: explanation({
					decision: 'denied',
					reason: 'forbidden',
					deniedFields: ['amount_cents'],
				}),
			}),
			effective: effective(),
			presetCtxForSubject: ctx,
		});
		expect(cell.decision).toBe('denied');
		expect(cell.deniedFields).toEqual(['amount_cents']);
	});

	test('surfaces the policy_filter_unindexed diagnostic from explain diagnostics', () => {
		const cell = resolveImpactCell({
			request: baseRequest,
			explainResult: result({
				explanation: null,
				diagnostics: [
					{
						code: 'POLICY_FILTER_UNINDEXED',
						message: 'denied',
						detail: {
							reason: 'policy_filter_unindexed',
							missing_index: 'reviewer_email',
							collection: 'policy_filter_unindexed',
						},
					},
				],
			}),
			effective: null,
			presetCtxForSubject: ctx,
		});
		expect(cell.decision).toBe('error');
		expect(cell.reason).toBe('POLICY_FILTER_UNINDEXED');
		expect(cell.diagnostic).not.toBeNull();
		expect(cell.diagnostic?.missingField).toBe('reviewer_email');
		expect(cell.diagnostic?.code).toBe('policy_filter_unindexed');
	});

	test('falls back to error decision when both explanation and diagnostics are missing', () => {
		const cell = resolveImpactCell({
			request: baseRequest,
			explainResult: null,
			effective: null,
			presetCtxForSubject: ctx,
		});
		expect(cell.decision).toBe('error');
		expect(cell.reason).toBe('unknown');
		expect(cell.explainHref).toContain('preset=explainPolicy');
	});

	test('omits explainHref when no preset context is supplied', () => {
		const cell = resolveImpactCell({
			request: baseRequest,
			explainResult: result({ explanation: explanation() }),
			effective: null,
			presetCtxForSubject: null,
		});
		expect(cell.explainHref).toBeNull();
	});
});

describe('mcpToolNameForOperation', () => {
	test('maps each evaluator operation to a stable MCP tool suffix', () => {
		expect(mcpToolNameForOperation('invoices', 'read')).toBe('invoices.get');
		expect(mcpToolNameForOperation('invoices', 'create')).toBe('invoices.create');
		expect(mcpToolNameForOperation('invoices', 'update')).toBe('invoices.patch');
		expect(mcpToolNameForOperation('invoices', 'patch')).toBe('invoices.patch');
		expect(mcpToolNameForOperation('invoices', 'delete')).toBe('invoices.delete');
		expect(mcpToolNameForOperation('invoices', 'transition')).toBe('invoices.patch');
		expect(mcpToolNameForOperation('invoices', 'rollback')).toBe('invoices.patch');
		expect(mcpToolNameForOperation('invoices', 'transaction')).toBe('invoices.transaction');
	});
	test('returns axon prefix when collection is empty', () => {
		expect(mcpToolNameForOperation('', 'read')).toBe('axon.get');
	});
});

describe('mcpOutcomeFromExplanation', () => {
	const explanation = (overrides: Partial<PolicyExplanation> = {}): PolicyExplanation => ({
		operation: 'patch',
		decision: 'allowed',
		reason: 'allowed',
		policyVersion: 1,
		ruleIds: [],
		policyIds: [],
		fieldPaths: [],
		deniedFields: [],
		rules: [],
		approval: null,
		operations: [],
		...overrides,
	});

	test('maps server decision strings to MCP outcomes', () => {
		expect(mcpOutcomeFromExplanation(explanation({ decision: 'allowed' })).outcome).toBe('allowed');
		expect(mcpOutcomeFromExplanation(explanation({ decision: 'allow' })).outcome).toBe('allowed');
		expect(mcpOutcomeFromExplanation(explanation({ decision: 'needs_approval' })).outcome).toBe(
			'needs_approval',
		);
		expect(mcpOutcomeFromExplanation(explanation({ decision: 'deny' })).outcome).toBe('denied');
		expect(mcpOutcomeFromExplanation(explanation({ decision: 'denied' })).outcome).toBe('denied');
	});

	test('returns conflict outcome when reason code matches a stale-binding code', () => {
		const result = mcpOutcomeFromExplanation(
			explanation({ decision: 'denied', reason: 'intent_pre_image_stale' }),
		);
		expect(result.outcome).toBe('conflict');
		expect(result.conflict?.dimension).toBe('intent_pre_image_stale');
		expect(result.conflict?.detail).toBe('intent_pre_image_stale');
	});

	test('returns denied outcome with no conflict when explanation is null', () => {
		expect(mcpOutcomeFromExplanation(null)).toEqual({ outcome: 'denied', conflict: null });
	});
});

describe('buildMcpEnvelopePreview', () => {
	const explanation = (overrides: Partial<PolicyExplanation> = {}): PolicyExplanation => ({
		operation: 'patch',
		decision: 'allowed',
		reason: 'allowed',
		policyVersion: 7,
		ruleIds: ['rule-1'],
		policyIds: ['policy-1'],
		fieldPaths: [],
		deniedFields: [],
		rules: [],
		approval: null,
		operations: [],
		...overrides,
	});

	const effective = (
		overrides: Partial<EffectiveCollectionPolicy> = {},
	): EffectiveCollectionPolicy => ({
		collection: 'invoices',
		canRead: true,
		canCreate: true,
		canUpdate: true,
		canDelete: true,
		redactedFields: ['amount_cents', 'commercial_terms'],
		deniedFields: [],
		policyVersion: 7,
		...overrides,
	});

	test('returns null when subject or collection is missing', () => {
		expect(
			buildMcpEnvelopePreview({
				subject: '',
				collection: 'invoices',
				operation: 'read',
				explanation: explanation(),
				effective: effective(),
				explainInput: { operation: 'read', collection: 'invoices' },
			}),
		).toBeNull();
		expect(
			buildMcpEnvelopePreview({
				subject: 'finance-agent',
				collection: '',
				operation: 'read',
				explanation: explanation(),
				effective: effective(),
				explainInput: { operation: 'read' },
			}),
		).toBeNull();
	});

	test('builds an allowed envelope with the policy version drawn from the explanation', () => {
		const preview = buildMcpEnvelopePreview({
			subject: 'finance-agent',
			collection: 'invoices',
			operation: 'patch',
			explanation: explanation(),
			effective: effective(),
			explainInput: {
				operation: 'patch',
				collection: 'invoices',
				entityId: 'inv-1',
				patch: { amount_cents: 1500 },
			},
		});
		expect(preview).not.toBeNull();
		expect(preview?.tool).toBe('invoices.patch');
		expect(preview?.outcome).toBe('allowed');
		expect(preview?.policyVersion).toBe(7);
		expect(preview?.reasonCode).toBe('allowed');
		expect(preview?.subject).toBe('finance-agent');
		expect(preview?.reproduction.outcome).toBe('allowed');
		expect(preview?.reproduction.tool).toBe('invoices.patch');
		expect(preview?.reproduction.arguments.entityId).toBe('inv-1');
	});

	test('redacts redacted fields in the reproduction patch payload', () => {
		const preview = buildMcpEnvelopePreview({
			subject: 'contractor',
			collection: 'invoices',
			operation: 'patch',
			explanation: explanation({ decision: 'denied', reason: 'forbidden_field_write' }),
			effective: effective(),
			explainInput: {
				operation: 'patch',
				collection: 'invoices',
				entityId: 'inv-1',
				patch: { amount_cents: 50_000, status: 'submitted' },
			},
		});
		expect(preview?.outcome).toBe('denied');
		expect((preview?.reproduction.arguments.patch as Record<string, unknown>).amount_cents).toBe(
			'[redacted]',
		);
		expect((preview?.reproduction.arguments.patch as Record<string, unknown>).status).toBe(
			'submitted',
		);
	});

	test('emits a conflict outcome when the explanation reason indicates a stale binding', () => {
		const preview = buildMcpEnvelopePreview({
			subject: 'finance-agent',
			collection: 'invoices',
			operation: 'patch',
			explanation: explanation({
				decision: 'denied',
				reason: 'intent_grant_version_stale',
			}),
			effective: effective(),
			explainInput: { operation: 'patch', collection: 'invoices', entityId: 'inv-1' },
		});
		expect(preview?.outcome).toBe('conflict');
		expect(preview?.conflict?.dimension).toBe('intent_grant_version_stale');
		expect(preview?.reproduction.outcome).toBe('conflict');
	});

	test('formatMcpReproduction emits stable JSON', () => {
		const preview = buildMcpEnvelopePreview({
			subject: 'finance-agent',
			collection: 'invoices',
			operation: 'read',
			explanation: explanation({ decision: 'allowed', reason: 'allowed' }),
			effective: effective(),
			explainInput: { operation: 'read', collection: 'invoices', entityId: 'inv-1' },
		});
		const json = preview ? formatMcpReproduction(preview) : '';
		expect(json).toContain('"tool": "invoices.get"');
		expect(json).toContain('"subject": "finance-agent"');
		expect(json).toContain('"outcome": "allowed"');
	});
});
