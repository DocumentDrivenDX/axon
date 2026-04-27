import { describe, expect, test } from 'bun:test';

import type { AuditEntry, MutationIntent } from '$lib/api';
import {
	buildMcpStdioProvenance,
	isSecretEnvKey,
	isSecretEnvValue,
	redactEnvEntry,
} from '$lib/mcp-provenance';

function mcpIntent(): MutationIntent {
	return {
		id: 'mint_mcp_1',
		tenantId: 'acme',
		databaseId: 'default',
		subject: {
			user_id: 'finance-bot',
			agent_id: 'tool.review-console',
			delegated_by: 'finance-agent',
			tenant_role: 'finance_agent',
			credential_id: 'cred-finance-bot',
			grant_version: 13,
		},
		schemaVersion: 1,
		policyVersion: 2,
		operation: {
			operationKind: 'patch_entity',
			operationHash: 'sha256:preview',
			operation: { collection: 'task', id: 'task-a', expected_version: 1 },
		},
		operationHash: 'sha256:preview',
		preImages: [{ kind: 'entity', collection: 'task', id: 'task-a', version: 1 }],
		decision: 'needs_approval',
		approvalState: 'pending',
		approvalRoute: null,
		expiresAtNs: '1000000000',
		reviewSummary: { summary: 'needs_approval' },
	};
}

function humanIntent(): MutationIntent {
	const intent = mcpIntent();
	intent.subject = {
		user_id: 'finance-agent',
		tenant_role: 'finance_agent',
		credential_id: 'cred-finance-agent',
		grant_version: 7,
	};
	return intent;
}

function mcpAuditEntry(timestampNs: number, requestId = 'req-mcp-1'): AuditEntry {
	return {
		id: 1,
		timestamp_ns: timestampNs,
		collection: 'task',
		entity_id: 'task-a',
		version: 1,
		mutation: 'intent.preview',
		data_before: null,
		data_after: null,
		actor: 'finance-bot',
		transaction_id: null,
		metadata: { event: 'preview' },
		intent_lineage: {
			intent_id: 'mint_mcp_1',
			decision: 'needs_approval',
			policy_version: 2,
			schema_version: 1,
			subject_snapshot: null,
			origin: {
				surface: 'mcp',
				tool_name: 'tool.review-console',
				request_id: requestId,
				operation_hash: 'sha256:preview',
			},
		},
	};
}

describe('isSecretEnvKey', () => {
	test('detects token, secret, key, password, credential, bearer keys', () => {
		expect(isSecretEnvKey('AXON_API_TOKEN')).toBe(true);
		expect(isSecretEnvKey('GH_SECRET')).toBe(true);
		expect(isSecretEnvKey('SOME_KEY')).toBe(true);
		expect(isSecretEnvKey('DB_PASSWORD')).toBe(true);
		expect(isSecretEnvKey('AUTH_HEADER')).toBe(true);
		expect(isSecretEnvKey('SESSION_ID')).toBe(true);
		expect(isSecretEnvKey('AXON_BEARER')).toBe(true);
		expect(isSecretEnvKey('CREDENTIAL_ID')).toBe(true);
	});

	test('passes through non-secret keys', () => {
		expect(isSecretEnvKey('AXON_TENANT')).toBe(false);
		expect(isSecretEnvKey('AXON_DATABASE')).toBe(false);
		expect(isSecretEnvKey('AXON_AGENT_ID')).toBe(false);
	});
});

describe('isSecretEnvValue', () => {
	test('detects bearer prefixes and api-style tokens', () => {
		expect(isSecretEnvValue('Bearer abc')).toBe(true);
		expect(isSecretEnvValue('sk-anything-here')).toBe(true);
	});

	test('passes through plain identifiers', () => {
		expect(isSecretEnvValue('finance-agent')).toBe(false);
		expect(isSecretEnvValue('default')).toBe(false);
	});
});

describe('redactEnvEntry', () => {
	test('redacts secret keys and values', () => {
		expect(redactEnvEntry({ key: 'AXON_API_TOKEN', value: 'sk-foo' })).toEqual({
			key: 'AXON_API_TOKEN',
			value: '[redacted]',
			redacted: true,
		});
		expect(redactEnvEntry({ key: 'AXON_BEARER', value: 'Bearer real-token' })).toEqual({
			key: 'AXON_BEARER',
			value: '[redacted]',
			redacted: true,
		});
	});

	test('keeps non-secret entries verbatim', () => {
		expect(redactEnvEntry({ key: 'AXON_TENANT', value: 'acme' })).toEqual({
			key: 'AXON_TENANT',
			value: 'acme',
			redacted: false,
		});
	});
});

describe('buildMcpStdioProvenance', () => {
	const scope = { tenant: 'acme', database: 'default' };

	test('returns null for human-originated intents', () => {
		expect(buildMcpStdioProvenance({ intent: humanIntent(), audit: [], scope, now: 1 })).toBeNull();
	});

	test('builds command, status, and provenance for MCP intents', () => {
		const audit = [mcpAuditEntry(2_000_000_000)];
		const provenance = buildMcpStdioProvenance({
			intent: mcpIntent(),
			audit,
			scope,
			now: 2_000_000_000,
		});
		expect(provenance).not.toBeNull();
		expect(provenance?.surface).toBe('mcp');
		expect(provenance?.transport).toBe('stdio');
		expect(provenance?.commandText).toBe(
			'axon-server --mcp-stdio --tenant acme --database default',
		);
		expect(provenance?.configStatus).toBe('active');
		expect(provenance?.configStatusLabel).toContain('Active');
		expect(provenance?.requestId).toBe('req-mcp-1');
		expect(provenance?.agentIdentity).toBe('tool.review-console');
		expect(provenance?.credentialId).toBe('cred-finance-bot');
		expect(provenance?.delegatedBy).toBe('finance-agent');
		expect(provenance?.toolName).toBe('tool.review-console');
		expect(provenance?.grantVersion).toBe('13');
	});

	test('marks idle status when last activity is hours old', () => {
		const oneHourPlusAgoNs = 0;
		const audit = [mcpAuditEntry(oneHourPlusAgoNs)];
		const nowNs = 30 * 60 * 1_000 * 1_000_000;
		const provenance = buildMcpStdioProvenance({
			intent: mcpIntent(),
			audit,
			scope,
			now: nowNs,
		});
		expect(provenance?.configStatus).toBe('idle');
		expect(provenance?.configStatusLabel).toContain('Idle');
	});

	test('env preview surfaces only routing-scoped keys and redacts credential id', () => {
		const audit = [mcpAuditEntry(2_000_000_000)];
		const provenance = buildMcpStdioProvenance({
			intent: mcpIntent(),
			audit,
			scope,
			now: 2_000_000_000,
		});
		const envByKey = new Map(provenance?.env.map((entry) => [entry.key, entry]) ?? []);
		expect([...envByKey.keys()].sort()).toEqual([
			'AXON_AGENT_ID',
			'AXON_CREDENTIAL_ID',
			'AXON_DATABASE',
			'AXON_TENANT',
		]);
		expect(envByKey.get('AXON_TENANT')?.value).toBe('acme');
		expect(envByKey.get('AXON_TENANT')?.redacted).toBe(false);
		expect(envByKey.get('AXON_DATABASE')?.value).toBe('default');
		expect(envByKey.get('AXON_AGENT_ID')?.value).toBe('tool.review-console');
		// CREDENTIAL_ID matches the secret-key pattern even though we surface
		// the credential ID elsewhere — the env preview must not leak it.
		expect(envByKey.get('AXON_CREDENTIAL_ID')?.redacted).toBe(true);
		expect(envByKey.get('AXON_CREDENTIAL_ID')?.value).toBe('[redacted]');
	});

	test('falls back to mcp surface when no audit activity is available', () => {
		const provenance = buildMcpStdioProvenance({
			intent: mcpIntent(),
			audit: [],
			scope,
			now: 1,
		});
		expect(provenance?.configStatus).toBe('unknown');
		expect(provenance?.requestId).toBeNull();
		// MCP-originated intents always default to the MCP surface even
		// when audit origin metadata has not yet been written.
		expect(provenance?.surface).toBe('mcp');
		expect(provenance?.transport).toBe('stdio');
	});
});
