import type {
	CollectionDetail,
	CollectionSummary,
	EffectiveCollectionPolicy,
	EntityRecord,
	ExplainPolicyInput,
	PolicyApprovalSummary,
	PolicyExplainDiagnostic,
	PolicyExplainResult,
	PolicyExplanation,
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

/**
 * Build a GraphQL preset that mirrors the MCP `axon.query` bridge for the
 * current envelope. The MCP `axon.query` tool routes through the same
 * GraphQL surface as the policy console, so the bridge preset is the
 * `explainPolicy` operation labelled distinctly so an operator can confirm
 * that what an agent saw matches what GraphQL returns directly.
 */
export function buildMcpBridgePreset(
	ctx: ConsolePresetContext,
	input: ExplainPolicyInput | null,
): GraphqlConsolePreset | null {
	if (!input) return null;
	return buildGraphqlConsolePreset(ctx, 'axon.query', EXPLAIN_POLICY_CONSOLE_QUERY, { input });
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

export type ImpactDecisionKind = 'allowed' | 'denied' | 'needs_approval' | 'error';

export type ImpactCell = {
	subjectId: string;
	operation: EvaluationOperation;
	entityId: string;
	decision: ImpactDecisionKind;
	reason: string;
	redactedFields: string[];
	deniedFields: string[];
	approvalRole: string | null;
	diagnostic: WorkspaceDiagnostic | null;
	explainHref: string | null;
};

export type ImpactMatrixRequest = {
	subjectId: string;
	operation: EvaluationOperation;
	entity: EntityRecord;
	explainInput: ExplainPolicyInput;
};

export const IMPACT_MATRIX_OPERATIONS: EvaluationOperation[] = ['read', 'patch', 'delete'];
export const IMPACT_MATRIX_SUBJECT_LIMIT = 3;
export const IMPACT_MATRIX_ENTITY_LIMIT = 2;

function normaliseDecisionKind(decision: string): ImpactDecisionKind {
	const kind = decision.trim().toLowerCase();
	if (kind === 'allow' || kind === 'allowed') return 'allowed';
	if (kind === 'deny' || kind === 'denied') return 'denied';
	if (kind === 'needs_approval' || kind === 'needs-approval') return 'needs_approval';
	return 'error';
}

function buildImpactExplainInput(
	collection: string,
	entity: EntityRecord,
	operation: EvaluationOperation,
): ExplainPolicyInput {
	const base: ExplainPolicyInput = { operation, collection, entityId: entity.id };
	switch (operation) {
		case 'read':
			return base;
		case 'delete':
			return { ...base, expectedVersion: entity.version };
		case 'update':
			return { ...base, expectedVersion: entity.version, data: entity.data };
		case 'patch':
			return {
				...base,
				expectedVersion: entity.version,
				patch: defaultPatchFixture(entity),
			};
		default:
			return base;
	}
}

export function buildImpactMatrixInputs(
	collection: string,
	entities: EntityRecord[],
	subjects: SubjectOption[],
	operations: EvaluationOperation[] = IMPACT_MATRIX_OPERATIONS,
): ImpactMatrixRequest[] {
	if (!collection) return [];
	return entities.flatMap((entity) =>
		subjects.flatMap((subject) =>
			operations.map((operation) => ({
				subjectId: subject.id,
				operation,
				entity,
				explainInput: buildImpactExplainInput(collection, entity, operation),
			})),
		),
	);
}

export type ResolveImpactCellArgs = {
	request: ImpactMatrixRequest;
	explainResult: PolicyExplainResult | null;
	effective: EffectiveCollectionPolicy | null;
	presetCtxForSubject: ConsolePresetContext | null;
};

export function resolveImpactCell({
	request,
	explainResult,
	effective,
	presetCtxForSubject,
}: ResolveImpactCellArgs): ImpactCell {
	const explanation = explainResult?.explanation ?? null;
	const explainDiagnostics = explainResult?.diagnostics ?? [];
	const graphqlDiagnostics = buildGraphqlDiagnostics(explainDiagnostics);
	const diagnostic = graphqlDiagnostics[0] ?? null;

	let decision: ImpactDecisionKind;
	let reason: string;
	if (explanation) {
		decision = normaliseDecisionKind(explanation.decision);
		reason = explanation.reason;
	} else if (explainDiagnostics.length) {
		decision = 'error';
		reason = explainDiagnostics[0]?.code ?? 'error';
	} else {
		decision = 'error';
		reason = 'unknown';
	}

	const explainHref = presetCtxForSubject
		? (buildExplainConsolePreset(presetCtxForSubject, request.explainInput)?.href ?? null)
		: null;

	return {
		subjectId: request.subjectId,
		operation: request.operation,
		entityId: request.entity.id,
		decision,
		reason,
		redactedFields: effective?.redactedFields ?? [],
		deniedFields: explanation?.deniedFields ?? [],
		approvalRole: explanation?.approval?.role ?? null,
		diagnostic,
		explainHref,
	};
}

export type McpEnvelopeOutcome = 'allowed' | 'needs_approval' | 'denied' | 'conflict';

export type McpEnvelopePreview = {
	tool: string;
	subject: string;
	collection: string;
	operation: EvaluationOperation;
	policyVersion: number | null;
	outcome: McpEnvelopeOutcome;
	reasonCode: string;
	ruleIds: string[];
	policyIds: string[];
	approval: PolicyApprovalSummary | null;
	redactedFields: string[];
	deniedFields: string[];
	conflict: McpEnvelopeConflict | null;
	reproduction: McpEnvelopeReproduction;
};

export type McpEnvelopeConflict = {
	dimension: string;
	detail: string;
};

export type McpEnvelopeReproduction = {
	tool: string;
	subject: string;
	policy_version: number | null;
	outcome: McpEnvelopeOutcome;
	reason_code: string;
	arguments: Record<string, unknown>;
};

const CONFLICT_REASON_CODES = new Set([
	'intent_stale',
	'intent_mismatch',
	'intent_grant_version_stale',
	'intent_schema_version_stale',
	'intent_policy_version_stale',
	'intent_pre_image_stale',
	'expected_version_mismatch',
	'version_conflict',
	'stale',
]);

const POLICY_OPERATION_TOOL_SUFFIX: Record<EvaluationOperation, string> = {
	read: 'get',
	create: 'create',
	update: 'patch',
	patch: 'patch',
	delete: 'delete',
	transition: 'patch',
	rollback: 'patch',
	transaction: 'transaction',
};

export function mcpToolNameForOperation(
	collection: string,
	operation: EvaluationOperation,
): string {
	const suffix = POLICY_OPERATION_TOOL_SUFFIX[operation] ?? operation;
	if (!collection) return `axon.${suffix}`;
	return `${collection}.${suffix}`;
}

export function mcpOutcomeFromExplanation(explanation: PolicyExplanation | null): {
	outcome: McpEnvelopeOutcome;
	conflict: McpEnvelopeConflict | null;
} {
	if (!explanation) {
		return { outcome: 'denied', conflict: null };
	}
	const decision = explanation.decision.trim().toLowerCase();
	const reason = explanation.reason.trim().toLowerCase();
	if (CONFLICT_REASON_CODES.has(reason)) {
		return {
			outcome: 'conflict',
			conflict: { dimension: reason, detail: explanation.reason },
		};
	}
	if (decision === 'needs_approval' || decision === 'needs-approval') {
		return { outcome: 'needs_approval', conflict: null };
	}
	if (decision === 'deny' || decision === 'denied') {
		return { outcome: 'denied', conflict: null };
	}
	return { outcome: 'allowed', conflict: null };
}

function sanitizeReproductionArguments(
	input: ExplainPolicyInput,
	redactedFields: string[],
): Record<string, unknown> {
	const args: Record<string, unknown> = {};
	if (input.collection) args.collection = input.collection;
	if (input.entityId) args.entityId = input.entityId;
	if (typeof input.expectedVersion === 'number') args.expectedVersion = input.expectedVersion;
	if (input.lifecycleName) args.lifecycleName = input.lifecycleName;
	if (input.targetState) args.targetState = input.targetState;
	if (typeof input.toVersion === 'number') args.toVersion = input.toVersion;
	if (input.data !== undefined) args.data = redactPayload(input.data, redactedFields);
	if (input.patch !== undefined) args.patch = redactPayload(input.patch, redactedFields);
	if (input.operations) args.operations = input.operations;
	return args;
}

function redactPayload(value: unknown, redactedFields: string[]): unknown {
	if (!redactedFields.length) return value;
	if (Array.isArray(value)) {
		return value.map((item) => redactPayload(item, redactedFields));
	}
	if (value && typeof value === 'object') {
		const fields = new Set(redactedFields);
		return Object.fromEntries(
			Object.entries(value as Record<string, unknown>).map(([key, current]) => [
				key,
				fields.has(key) ? '[redacted]' : current,
			]),
		);
	}
	return value;
}

export type BuildMcpEnvelopePreviewArgs = {
	subject: string;
	collection: string;
	operation: EvaluationOperation;
	explanation: PolicyExplanation | null;
	effective: EffectiveCollectionPolicy | null;
	explainInput: ExplainPolicyInput | null;
};

export function buildMcpEnvelopePreview(
	args: BuildMcpEnvelopePreviewArgs,
): McpEnvelopePreview | null {
	const { subject, collection, operation, explanation, effective, explainInput } = args;
	if (!collection || !subject) return null;
	// Require an actual explainPolicy result before rendering the envelope.
	// During reactive transitions (operation/subject changes that re-fetch
	// the explanation), the previous render briefly held a `null` explanation
	// alongside a non-null `explainInput`, which produced a misleading
	// envelope where reasonCode showed `unknown` but the policy explanation
	// panel had not yet caught up. Gating on explanation makes both panels
	// render coherently from the same evaluation result.
	if (!explanation) return null;

	const tool = mcpToolNameForOperation(collection, operation);
	const { outcome, conflict } = mcpOutcomeFromExplanation(explanation);
	const policyVersion = explanation.policyVersion ?? effective?.policyVersion ?? null;
	const reasonCode = explanation.reason;
	const redactedFields = effective?.redactedFields ?? [];
	const reproductionArgs = explainInput
		? sanitizeReproductionArguments(explainInput, redactedFields)
		: { collection };

	return {
		tool,
		subject,
		collection,
		operation,
		policyVersion,
		outcome,
		reasonCode,
		ruleIds: explanation?.ruleIds ?? [],
		policyIds: explanation?.policyIds ?? [],
		approval: explanation?.approval ?? null,
		redactedFields,
		deniedFields: explanation?.deniedFields ?? [],
		conflict,
		reproduction: {
			tool,
			subject,
			policy_version: policyVersion,
			outcome,
			reason_code: reasonCode,
			arguments: reproductionArgs,
		},
	};
}

export function formatMcpReproduction(preview: McpEnvelopePreview): string {
	return prettyJson(preview.reproduction);
}

export type McpEnvelopeMatchState = 'match' | 'mismatch' | 'unknown';

export type McpEnvelopeComparison = {
	outcomeMatch: 'match' | 'mismatch';
	policyVersionMatch: McpEnvelopeMatchState;
	mcpReasonCode: string;
	mcpPolicyVersion: number | null;
	explainDecision: string;
	explainReason: string;
	explainPolicyVersion: number | null;
};

export function buildMcpEnvelopeComparison(
	preview: McpEnvelopePreview | null,
	explanation: PolicyExplanation | null,
): McpEnvelopeComparison | null {
	if (!preview || !explanation) return null;
	const explainPolicyVersion =
		typeof explanation.policyVersion === 'number' ? explanation.policyVersion : null;
	const outcomeMatch =
		preview.reasonCode.trim().toLowerCase() === explanation.reason.trim().toLowerCase()
			? 'match'
			: 'mismatch';
	let policyVersionMatch: McpEnvelopeMatchState;
	if (preview.policyVersion === null && explainPolicyVersion === null) {
		policyVersionMatch = 'unknown';
	} else if (preview.policyVersion === explainPolicyVersion) {
		policyVersionMatch = 'match';
	} else {
		policyVersionMatch = 'mismatch';
	}
	return {
		outcomeMatch,
		policyVersionMatch,
		mcpReasonCode: preview.reasonCode,
		mcpPolicyVersion: preview.policyVersion,
		explainDecision: explanation.decision,
		explainReason: explanation.reason,
		explainPolicyVersion,
	};
}
