import type {
	AuditEntry,
	CommitMutationIntentOutcome,
	MutationIntent,
	MutationIntentCanonicalOperation,
} from '$lib/api';
import type { JsonValue } from '$lib/components/json-tree-types';

type JsonRecord = Record<string, JsonValue>;
type UnknownRecord = Record<string, unknown>;

const FIELD_PATH_LIMIT = 12;
const TOOL_NAME_SPLIT_PATTERN = /[./:]/;

export function asRecord(value: unknown): UnknownRecord | null {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	return value as UnknownRecord;
}

export function stringMember(value: UnknownRecord | null, ...keys: string[]): string | null {
	if (!value) return null;
	for (const key of keys) {
		const candidate = value[key];
		if (typeof candidate === 'string' && candidate.trim().length > 0) {
			return candidate;
		}
	}
	return null;
}

export function numberMember(value: UnknownRecord | null, ...keys: string[]): number | null {
	if (!value) return null;
	for (const key of keys) {
		const candidate = value[key];
		if (typeof candidate === 'number' && Number.isFinite(candidate)) {
			return candidate;
		}
	}
	return null;
}

export function subjectRecord(intent: MutationIntent): UnknownRecord | null {
	return asRecord(intent.subject);
}

export function subjectField(intent: MutationIntent, ...keys: string[]): string {
	return stringMember(subjectRecord(intent), ...keys) ?? '-';
}

export function grantVersionLabel(intent: MutationIntent): string {
	const value = numberMember(subjectRecord(intent), 'grant_version', 'grantVersion');
	return value === null ? '-' : String(value);
}

export function requesterLabel(intent: MutationIntent): string {
	return subjectField(intent, 'user_id', 'userId');
}

export function agentIdentityLabel(intent: MutationIntent): string {
	return subjectField(intent, 'agent_id', 'agentId');
}

export function delegatedByLabel(intent: MutationIntent): string {
	return subjectField(intent, 'delegated_by', 'delegatedBy');
}

export function credentialLabel(intent: MutationIntent): string {
	return subjectField(intent, 'credential_id', 'credentialId');
}

export function tenantRoleLabel(intent: MutationIntent): string {
	return subjectField(intent, 'tenant_role', 'tenantRole');
}

export function isMcpOriginated(intent: MutationIntent): boolean {
	return agentIdentityLabel(intent) !== '-';
}

export function originBadge(intent: MutationIntent): 'MCP' | 'UI' {
	return isMcpOriginated(intent) ? 'MCP' : 'UI';
}

function latestAuditEntry(auditEntries: AuditEntry[] = []): AuditEntry | null {
	let latest: AuditEntry | null = null;
	for (const entry of auditEntries) {
		if (!latest || entry.timestamp_ns > latest.timestamp_ns) {
			latest = entry;
		}
	}
	return latest;
}

function latestOrigin(auditEntries: AuditEntry[] = []): UnknownRecord | null {
	return asRecord(latestAuditEntry(auditEntries)?.intent_lineage)?.origin
		? (asRecord(latestAuditEntry(auditEntries)?.intent_lineage)?.origin as UnknownRecord)
		: null;
}

function inferToolName(agentIdentity: string): string {
	if (agentIdentity === '-') return 'ui.graphql';
	const parts = agentIdentity.split(TOOL_NAME_SPLIT_PATTERN).filter((part) => part.length > 0);
	return parts.at(-1) ?? agentIdentity;
}

export function toolNameLabel(intent: MutationIntent, auditEntries: AuditEntry[] = []): string {
	const origin = latestOrigin(auditEntries);
	const originTool = stringMember(origin, 'tool_name', 'toolName');
	if (originTool) return originTool;
	return inferToolName(agentIdentityLabel(intent));
}

function pushPath(paths: string[], path: string) {
	if (path.length === 0 || paths.includes(path)) return;
	paths.push(path);
}

function collectFieldPaths(value: unknown, prefix = ''): string[] {
	if (!value || typeof value !== 'object') {
		return prefix ? [prefix] : [];
	}
	if (Array.isArray(value)) {
		return prefix ? [`${prefix}[]`] : [];
	}
	const object = value as UnknownRecord;
	const entries = Object.entries(object);
	if (entries.length === 0) return prefix ? [prefix] : [];
	const paths: string[] = [];
	for (const [key, child] of entries) {
		const path = prefix ? `${prefix}.${key}` : key;
		const childPaths = collectFieldPaths(child, path);
		if (childPaths.length === 0) {
			pushPath(paths, path);
			continue;
		}
		for (const childPath of childPaths) {
			pushPath(paths, childPath);
		}
	}
	return paths;
}

function fieldSummary(value: unknown): JsonRecord {
	const paths = collectFieldPaths(value);
	return {
		field_count: paths.length,
		fields: paths.slice(0, FIELD_PATH_LIMIT),
		...(paths.length > FIELD_PATH_LIMIT
			? { truncated_field_count: paths.length - FIELD_PATH_LIMIT }
			: {}),
	};
}

function assignIfPresent(target: JsonRecord, key: string, value: unknown): void {
	if (typeof value === 'string' && value.length > 0) {
		target[key] = value;
		return;
	}
	if (typeof value === 'number' && Number.isFinite(value)) {
		target[key] = value;
	}
}

function summarizeTransactionOperations(value: unknown): JsonRecord {
	const operations = Array.isArray(value) ? value : [];
	return {
		operation_count: operations.length,
		operation_kinds: operations
			.map((entry) => asRecord(entry))
			.map((entry) => stringMember(entry, 'operation_kind', 'operationKind', 'variant'))
			.filter((entry): entry is string => Boolean(entry))
			.slice(0, FIELD_PATH_LIMIT),
	};
}

export function toolArgumentsSummary(operation: MutationIntentCanonicalOperation): JsonValue {
	const payload = asRecord(operation.operation);
	const summary: JsonRecord = {
		operation_kind: operation.operationKind,
	};

	if (!payload) return summary;

	assignIfPresent(summary, 'collection', payload.collection);
	assignIfPresent(summary, 'entity_id', payload.id);
	assignIfPresent(summary, 'expected_version', payload.expected_version ?? payload.expectedVersion);
	assignIfPresent(
		summary,
		'source_collection',
		payload.source_collection ?? payload.sourceCollection,
	);
	assignIfPresent(summary, 'source_id', payload.source_id ?? payload.sourceId);
	assignIfPresent(
		summary,
		'target_collection',
		payload.target_collection ?? payload.targetCollection,
	);
	assignIfPresent(summary, 'target_id', payload.target_id ?? payload.targetId);
	assignIfPresent(summary, 'link_type', payload.link_type ?? payload.linkType);
	assignIfPresent(summary, 'lifecycle_name', payload.lifecycle_name ?? payload.lifecycleName);
	assignIfPresent(summary, 'target_state', payload.target_state ?? payload.targetState);
	assignIfPresent(summary, 'to_version', payload.to_version ?? payload.toVersion);
	assignIfPresent(summary, 'rollback_scope', payload.rollback_scope ?? payload.rollbackScope);

	if (payload.patch !== undefined) {
		summary.patch = fieldSummary(payload.patch);
	}
	if (payload.data !== undefined) {
		summary.data = fieldSummary(payload.data);
	}
	if (payload.target !== undefined) {
		summary.target = fieldSummary(payload.target);
	}
	if (payload.operations !== undefined) {
		summary.transaction = summarizeTransactionOperations(payload.operations);
	}

	return summary;
}

function commitOutcomeErrorCode(
	commitOutcome: CommitMutationIntentOutcome | null | undefined,
): string | null {
	if (!commitOutcome || commitOutcome.ok) return null;
	return commitOutcome.error.code ?? null;
}

function derivedOutcomeLabel(intent: MutationIntent, errorCode: string | null): string {
	if (errorCode) return 'conflict';
	switch (intent.approvalState) {
		case 'committed':
			return 'committed';
		case 'rejected':
			return 'rejected';
		case 'expired':
			return 'expired';
		case 'approved':
			return intent.decision === 'allow' ? 'allowed' : 'approved';
		case 'pending':
			return intent.decision === 'needs_approval' ? 'needs_approval' : intent.decision;
		case 'none':
			if (intent.decision === 'allow') return 'allowed';
			if (intent.decision === 'deny') return 'denied';
			return intent.decision;
		default:
			return intent.decision;
	}
}

export function structuredOutcomeSummary(
	intent: MutationIntent,
	options: {
		auditEntries?: AuditEntry[];
		commitOutcome?: CommitMutationIntentOutcome | null;
	} = {},
): JsonValue {
	const latest = latestAuditEntry(options.auditEntries ?? []);
	const auditErrorCode = latest?.metadata?.error_code ?? null;
	const commitErrorCode = commitOutcomeErrorCode(options.commitOutcome);
	const errorCode = commitErrorCode ?? auditErrorCode;
	const latestEvent =
		commitErrorCode !== null
			? 'commit_validation_failed'
			: (latest?.metadata?.event ?? latest?.mutation ?? null);

	const summary: JsonRecord = {
		origin: isMcpOriginated(intent) ? 'mcp' : 'human',
		outcome: derivedOutcomeLabel(intent, errorCode),
		decision: intent.decision,
		approval_state: intent.approvalState,
	};

	if (intent.approvalRoute?.role) {
		summary.required_approver_role = intent.approvalRoute.role;
	}
	if (errorCode) {
		summary.error_code = errorCode;
	}
	if (latestEvent) {
		summary.latest_event = latestEvent;
	}
	if (latest?.mutation) {
		summary.audit_mutation = latest.mutation;
	}

	return summary;
}

export function originMetadataSummary(
	intent: MutationIntent,
	auditEntries: AuditEntry[] = [],
): JsonValue {
	const summary: JsonRecord = {
		origin: isMcpOriginated(intent) ? 'mcp' : 'human',
		requester: requesterLabel(intent),
		agent_identity: agentIdentityLabel(intent),
		tool_name: toolNameLabel(intent, auditEntries),
		delegated_by: delegatedByLabel(intent),
		tenant_role: tenantRoleLabel(intent),
		credential_id: credentialLabel(intent),
		grant_version: grantVersionLabel(intent),
	};

	const origin = latestOrigin(auditEntries);
	const requestId = stringMember(origin, 'request_id', 'requestId');
	if (requestId) {
		summary.request_id = requestId;
	}

	return summary;
}
