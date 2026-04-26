import type {
	CollectionDetail,
	CollectionSummary,
	EntityRecord,
	ExplainPolicyInput,
	PolicyExplainDiagnostic,
} from './api';

export type SubjectOption = {
	id: string;
	label: string;
	detail: string | null;
};

export type EvaluationOperation =
	| 'read'
	| 'create'
	| 'update'
	| 'patch'
	| 'delete'
	| 'transition'
	| 'rollback'
	| 'transaction';

export type WorkspaceDiagnostic = {
	source: 'schema' | 'graphql';
	code: string;
	summary: string;
	missingField: string | null;
	remediation: string;
};

export type GraphqlConsolePreset = {
	href: string;
	preset: string;
};

export const EFFECTIVE_POLICY_CONSOLE_QUERY = `query PolicyWorkspaceEffectivePolicy($collection: String!, $entityId: ID) {
	effectivePolicy(collection: $collection, entityId: $entityId) {
		collection
		canRead
		canCreate
		canUpdate
		canDelete
		redactedFields
		deniedFields
		policyVersion
	}
}`;

export const EXPLAIN_POLICY_CONSOLE_QUERY = `query PolicyWorkspaceExplainPolicy($input: ExplainPolicyInput!) {
	explainPolicy(input: $input) {
		operation
		collection
		entityId
		operationIndex
		decision
		reason
		policyVersion
		ruleIds
		policyIds
		fieldPaths
		deniedFields
		rules {
			ruleId
			name
			kind
			fieldPath
		}
		approval {
			policyId
			name
			decision
			role
			reasonRequired
			deadlineSeconds
			separationOfDuties
		}
		operations {
			operation
			collection
			entityId
			operationIndex
			decision
			reason
			policyVersion
			ruleIds
			policyIds
			fieldPaths
			deniedFields
			rules {
				ruleId
				name
				kind
				fieldPath
			}
			approval {
				policyId
				name
				decision
				role
				reasonRequired
				deadlineSeconds
				separationOfDuties
			}
		}
	}
}`;

export function prettyJson(value: unknown): string {
	return JSON.stringify(value ?? {}, null, 2);
}

export function formatFields(fields: string[]): string {
	return fields.length ? fields.join(', ') : 'None';
}

export function errorMessage(error: unknown, fallback: string): string {
	return error instanceof Error ? error.message : fallback;
}

export function defaultCollectionName(nextCollections: CollectionSummary[]): string {
	for (const preferredName of ['invoices', 'task', 'expense']) {
		const preferred = nextCollections.find((collection) => collection.name === preferredName);
		if (preferred) return preferred.name;
	}
	const preferred = nextCollections.find(
		(collection) => !['user', 'users'].includes(collection.name),
	);
	return preferred?.name ?? nextCollections[0]?.name ?? '';
}

export function parseOptionalInteger(value: string, label: string): number | null {
	const trimmed = value.trim();
	if (!trimmed) return null;
	const parsed = Number(trimmed);
	if (!Number.isInteger(parsed) || parsed < 0) {
		throw new Error(`${label} must be a non-negative integer`);
	}
	return parsed;
}

export function parseJsonFixture(text: string, label: string): unknown {
	try {
		return JSON.parse(text);
	} catch {
		throw new Error(`${label} must be valid JSON`);
	}
}

export function defaultPatchFixture(entity: EntityRecord | null): Record<string, unknown> {
	const data = entity?.data ?? {};
	if (typeof data.amount_cents === 'number') {
		return { amount_cents: data.amount_cents + 500_000 };
	}
	if (typeof data.budget_cents === 'number') {
		return { budget_cents: data.budget_cents + 15_000 };
	}
	if (typeof data.status === 'string') {
		return { status: data.status === 'approved' ? 'draft' : 'approved' };
	}
	if (typeof data.title === 'string') {
		return { title: `${data.title} (policy dry-run)` };
	}
	return {};
}

export function defaultTransactionFixture(
	entity: EntityRecord | null,
): Array<Record<string, unknown>> {
	if (!entity) return [];
	return [
		{
			updateEntity: {
				collection: entity.collection,
				id: entity.id,
				expectedVersion: entity.version,
				data: entity.data,
			},
		},
	];
}

export function operationRequiresEntity(operation: EvaluationOperation): boolean {
	return ['update', 'patch', 'delete', 'transition', 'rollback'].includes(operation);
}

export function operationRequiresExpectedVersion(operation: EvaluationOperation): boolean {
	return ['update', 'patch', 'delete', 'transition'].includes(operation);
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function collectWhereFields(value: unknown, fields: Set<string>) {
	if (Array.isArray(value)) {
		for (const item of value) {
			collectWhereFields(item, fields);
		}
		return;
	}
	if (!isRecord(value)) return;

	for (const [key, candidate] of Object.entries(value)) {
		if (key === 'where' && isRecord(candidate) && typeof candidate.field === 'string') {
			fields.add(candidate.field);
		}
		collectWhereFields(candidate, fields);
	}
}

export function buildSchemaDiagnostics(
	detail: CollectionDetail | null,
	operation: EvaluationOperation,
): WorkspaceDiagnostic[] {
	if (operation !== 'read') return [];
	const schema = detail?.schema;
	if (!schema) return [];
	const whereFields = new Set<string>();
	collectWhereFields(schema.access_control, whereFields);
	if (!whereFields.size) return [];

	const indexedFields = new Set(
		(schema.indexes ?? [])
			.map((index) => (typeof index.field === 'string' ? index.field : null))
			.filter((value): value is string => Boolean(value)),
	);

	return [...whereFields]
		.filter((field) => !indexedFields.has(field))
		.map((field) => ({
			source: 'schema' as const,
			code: 'policy_filter_unindexed',
			summary: `Collection read filters depend on unindexed field "${field}".`,
			missingField: field,
			remediation: `Add an index on "${field}" or narrow the policy filter so collection reads do not fail with policy_filter_unindexed.`,
		}));
}

export function buildGraphqlDiagnostics(
	diagnostics: PolicyExplainDiagnostic[],
): WorkspaceDiagnostic[] {
	return diagnostics.flatMap((diagnostic) => {
		const reason = typeof diagnostic.detail?.reason === 'string' ? diagnostic.detail.reason : null;
		const missingField =
			typeof diagnostic.detail?.missing_index === 'string' ? diagnostic.detail.missing_index : null;
		const collection =
			typeof diagnostic.detail?.collection === 'string' ? diagnostic.detail.collection : null;
		const candidateCount =
			typeof diagnostic.detail?.candidate_count === 'number'
				? diagnostic.detail.candidate_count
				: null;
		const costLimit =
			typeof diagnostic.detail?.cost_limit === 'number' ? diagnostic.detail.cost_limit : null;

		if (reason !== 'policy_filter_unindexed' && diagnostic.code !== 'POLICY_FILTER_UNINDEXED') {
			return [];
		}

		const saturation =
			candidateCount !== null && costLimit !== null
				? ` Candidate set ${candidateCount} exceeded the limit ${costLimit}.`
				: '';
		return [
			{
				source: 'graphql' as const,
				code: 'policy_filter_unindexed',
				summary: collection
					? `GraphQL denied ${collection} because the policy filter needs an index${
							missingField ? ` on "${missingField}"` : ''
						}.${saturation}`
					: `GraphQL policy evaluation needs a missing index${
							missingField ? ` on "${missingField}"` : ''
						}.${saturation}`,
				missingField,
				remediation: missingField
					? `Add an index on "${missingField}" so the policy can evaluate without post-filter rejection.`
					: 'Add the missing index or simplify the policy filter before retrying.',
			},
		];
	});
}

export function dedupeDiagnostics(diagnostics: WorkspaceDiagnostic[]): WorkspaceDiagnostic[] {
	const seen = new Set<string>();
	return diagnostics.filter((diagnostic) => {
		const key = `${diagnostic.code}:${diagnostic.missingField ?? ''}:${diagnostic.source}`;
		if (seen.has(key)) return false;
		seen.add(key);
		return true;
	});
}

export function formatDiagnosticError(diagnostic: PolicyExplainDiagnostic): string {
	return diagnostic.code ? `${diagnostic.code}: ${diagnostic.message}` : diagnostic.message;
}

export type BuildExplainInputArgs = {
	operation: EvaluationOperation;
	collection: string;
	entity: EntityRecord | null;
	expectedVersionText: string;
	rollbackVersionText: string;
	lifecycleName: string;
	targetState: string;
	dataFixtureText: string;
	patchFixtureText: string;
	transactionFixtureText: string;
};

export function buildExplainInput(args: BuildExplainInputArgs): ExplainPolicyInput {
	const {
		operation,
		collection,
		entity,
		expectedVersionText,
		rollbackVersionText,
		lifecycleName,
		targetState,
		dataFixtureText,
		patchFixtureText,
		transactionFixtureText,
	} = args;

	const input: ExplainPolicyInput = {
		operation,
		...(collection ? { collection } : {}),
	};

	if (operation === 'transaction') {
		const operations = parseJsonFixture(transactionFixtureText, 'Transaction fixture');
		if (!Array.isArray(operations)) {
			throw new Error('Transaction fixture must be a JSON array');
		}
		return {
			operation: 'transaction',
			operations: operations as Record<string, unknown>[],
		};
	}

	if (entity) {
		input.entityId = entity.id;
	}

	if (operationRequiresEntity(operation) && !entity) {
		throw new Error('Select an entity before running this evaluator operation');
	}

	if (operationRequiresExpectedVersion(operation)) {
		const expectedVersion = parseOptionalInteger(expectedVersionText, 'Expected version');
		if (expectedVersion !== null) {
			input.expectedVersion = expectedVersion;
		}
	}

	switch (operation) {
		case 'read':
		case 'delete':
			return input;
		case 'create':
			return {
				operation: 'create',
				...(collection ? { collection } : {}),
				data: parseJsonFixture(dataFixtureText, 'JSON fixture'),
			};
		case 'update':
			return {
				...input,
				data: parseJsonFixture(dataFixtureText, 'JSON fixture'),
			};
		case 'patch':
			return {
				...input,
				patch: parseJsonFixture(patchFixtureText, 'Patch fixture'),
			};
		case 'transition':
			if (!lifecycleName.trim()) {
				throw new Error('Lifecycle name is required for transition evaluation');
			}
			if (!targetState.trim()) {
				throw new Error('Target state is required for transition evaluation');
			}
			return {
				...input,
				lifecycleName: lifecycleName.trim(),
				targetState: targetState.trim(),
			};
		case 'rollback': {
			const toVersion = parseOptionalInteger(rollbackVersionText, 'Rollback version');
			return {
				...input,
				...(toVersion !== null ? { toVersion } : {}),
			};
		}
		default:
			return input;
	}
}

export type ConsolePresetContext = {
	baseHref: string;
	subject: string;
};

export function buildGraphqlConsolePreset(
	ctx: ConsolePresetContext,
	preset: string,
	query: string,
	variables: Record<string, unknown>,
): GraphqlConsolePreset {
	const params = new URLSearchParams();
	params.set('preset', preset);
	params.set('query', query);
	params.set('variables', JSON.stringify(variables, null, 2));
	if (ctx.subject) {
		params.set('actor', ctx.subject);
	}
	return {
		preset,
		href: `${ctx.baseHref}?${params.toString()}`,
	};
}

export function buildEffectiveConsolePreset(
	ctx: ConsolePresetContext,
	collection: string,
	entity: EntityRecord | null,
): GraphqlConsolePreset | null {
	if (!collection) return null;
	return buildGraphqlConsolePreset(ctx, 'effectivePolicy', EFFECTIVE_POLICY_CONSOLE_QUERY, {
		collection,
		entityId: entity?.id ?? null,
	});
}

export function buildExplainConsolePreset(
	ctx: ConsolePresetContext,
	input: ExplainPolicyInput | null,
): GraphqlConsolePreset | null {
	if (!input) return null;
	return buildGraphqlConsolePreset(ctx, 'explainPolicy', EXPLAIN_POLICY_CONSOLE_QUERY, { input });
}

export function tryBuildExplainInput(args: BuildExplainInputArgs): {
	input: ExplainPolicyInput | null;
	error: string | null;
} {
	try {
		return { input: buildExplainInput(args), error: null };
	} catch (error) {
		return {
			input: null,
			error: errorMessage(error, 'Failed to build policy evaluation'),
		};
	}
}
