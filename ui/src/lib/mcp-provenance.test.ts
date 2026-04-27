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

// Minimal POSIX-style shell parser used to validate that commandText round-trips
// back to the original argv. Handles unquoted tokens, single-quoted segments
// (literal — no escapes), and backslash escapes outside of single quotes.
function parseShellCommand(text: string): string[] {
	const tokens: string[] = [];
	let current = '';
	let inToken = false;
	let i = 0;
	while (i < text.length) {
		const c = text[i];
		if (c === ' ' || c === '\t') {
			if (inToken) {
				tokens.push(current);
				current = '';
				inToken = false;
			}
			i++;
			continue;
		}
		inToken = true;
		if (c === "'") {
			i++;
			while (i < text.length && text[i] !== "'") {
				current += text[i];
				i++;
			}
			i++;
			continue;
		}
		if (c === '\\' && i + 1 < text.length) {
			current += text[i + 1];
			i += 2;
			continue;
		}
		current += c;
		i++;
	}
	if (inToken) tokens.push(current);
	return tokens;
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

	test('detects adversarial key shapes that embed secret-marker substrings', () => {
		// Keys with secret markers in the middle, separator variants, mixed
		// case, and tail variants must all be flagged. These exercise the
		// regex set against shapes a sloppy operator config might produce.
		expect(isSecretEnvKey('OAUTH2_BEARER_TOKEN')).toBe(true);
		expect(isSecretEnvKey('x-api-key')).toBe(true);
		expect(isSecretEnvKey('myAPIKey')).toBe(true);
		expect(isSecretEnvKey('SERVICE_AUTHORIZATION_URL')).toBe(true);
		expect(isSecretEnvKey('ROTATING_CREDENTIAL_REF')).toBe(true);
		expect(isSecretEnvKey('SMTP_PASSWORD_FILE')).toBe(true);
	});

	test('passes through non-secret keys', () => {
		expect(isSecretEnvKey('AXON_TENANT')).toBe(false);
		expect(isSecretEnvKey('AXON_DATABASE')).toBe(false);
		expect(isSecretEnvKey('AXON_AGENT_ID')).toBe(false);
	});

	test('does not over-match keys that merely embed a secret-marker prefix', () => {
		// The /key$/i pattern anchors on word-end, so 'KEYBOARD' must not
		// match. 'BEAVER_DAM' must not match /bearer/i because 'bearer' is
		// not present. These are the false-positives that would erode
		// operator trust if the redaction layer were too eager.
		expect(isSecretEnvKey('AXON_KEYBOARD')).toBe(false);
		expect(isSecretEnvKey('BEAVER_DAM')).toBe(false);
		expect(isSecretEnvKey('AXON_REGION')).toBe(false);
	});
});

describe('isSecretEnvValue', () => {
	test('detects bearer prefixes and api-style tokens', () => {
		expect(isSecretEnvValue('Bearer abc')).toBe(true);
		expect(isSecretEnvValue('sk-anything-here')).toBe(true);
	});

	test('detects adversarial token-shaped values', () => {
		// Mixed-case bearer prefix, tab separator, and the dotenv-style
		// 'NAME_TOKEN=' leak shape must all be flagged. Without these the
		// redaction layer would let common copy-pasted credential strings
		// through to the env preview.
		expect(isSecretEnvValue('BEARER xyz')).toBe(true);
		expect(isSecretEnvValue('Bearer\teyJhbGciOi')).toBe(true);
		expect(isSecretEnvValue('SK-ABCDEF1234')).toBe(true);
		expect(isSecretEnvValue('AXON_API_TOKEN=secret-value')).toBe(true);
	});

	test('passes through plain identifiers', () => {
		expect(isSecretEnvValue('finance-agent')).toBe(false);
		expect(isSecretEnvValue('default')).toBe(false);
	});

	test('does not over-match values that look superficially similar', () => {
		// Bearer without a separator is just a word; sk without a hyphen is
		// not the OpenAI/Stripe-style prefix; lowercase env-style assignment
		// is not the uppercase TOKEN= leak shape. These three near-misses
		// guard against the redaction layer becoming a denylist of
		// substrings.
		expect(isSecretEnvValue('BearerToken')).toBe(false);
		expect(isSecretEnvValue('sk_underscore')).toBe(false);
		expect(isSecretEnvValue('lowercase_token=foo')).toBe(false);
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

	test('redacts when only the value matches even if the key looks innocuous', () => {
		// The key 'PROXY_USER' is not a secret-pattern key, but the value
		// is a real-shaped bearer token. Operators paste these into env
		// configs by accident; redactEnvEntry must catch the value side
		// independently of the key.
		const result = redactEnvEntry({ key: 'PROXY_USER', value: 'Bearer eyJhbGciOiJIUzI1NiJ9' });
		expect(result).toEqual({ key: 'PROXY_USER', value: '[redacted]', redacted: true });
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
		// Within 60 minutes: idle, label includes the lastSeen timestamp.
		const idleActivityNs = 0;
		const idleNowNs = 30 * 60 * 1_000 * 1_000_000;
		const idleProv = buildMcpStdioProvenance({
			intent: mcpIntent(),
			audit: [mcpAuditEntry(idleActivityNs)],
			scope,
			now: idleNowNs,
		});
		expect(idleProv?.configStatus).toBe('idle');
		expect(idleProv?.configStatusLabel).toContain('Idle');
		expect(idleProv?.configStatusLabel).toContain('last stdio activity');

		// Older than 60 minutes: stale, not the "no activity" message.
		// Operators must see the timestamp so they can correlate with the
		// audit timeline rather than a misleading "no activity" string.
		const staleActivityNs = 0;
		const staleNowNs = 90 * 60 * 1_000 * 1_000_000;
		const staleProv = buildMcpStdioProvenance({
			intent: mcpIntent(),
			audit: [mcpAuditEntry(staleActivityNs)],
			scope,
			now: staleNowNs,
		});
		expect(staleProv?.configStatus).toBe('stale');
		expect(staleProv?.configStatusLabel).toContain('Stale');
		expect(staleProv?.configStatusLabel).toContain('last stdio activity');
		expect(staleProv?.configStatusLabel).not.toContain('No recent stdio activity recorded');
	});

	test('env preview surfaces only routing-scoped keys with no leaked secret values', () => {
		const audit = [mcpAuditEntry(2_000_000_000)];
		const provenance = buildMcpStdioProvenance({
			intent: mcpIntent(),
			audit,
			scope,
			now: 2_000_000_000,
		});
		const envByKey = new Map(provenance?.env.map((entry) => [entry.key, entry]) ?? []);
		// Allowlist: env preview is only the deterministic operator-facing
		// routing scope. Anything outside this set would mean buildEnvPreview
		// added an entry that operators cannot derive from public config.
		const allowedKeys = new Set([
			'AXON_TENANT',
			'AXON_DATABASE',
			'AXON_AGENT_ID',
			'AXON_CREDENTIAL_ID',
		]);
		for (const key of envByKey.keys()) {
			expect(allowedKeys.has(key)).toBe(true);
		}
		// Inputs that came from the test fixture (not synthesized inside
		// buildEnvPreview) round-trip verbatim and unredacted.
		expect(envByKey.get('AXON_TENANT')?.value).toBe('acme');
		expect(envByKey.get('AXON_TENANT')?.redacted).toBe(false);
		expect(envByKey.get('AXON_DATABASE')?.value).toBe('default');
		expect(envByKey.get('AXON_DATABASE')?.redacted).toBe(false);
		expect(envByKey.get('AXON_AGENT_ID')?.value).toBe('tool.review-console');
		// Structural invariant: regardless of which keys buildEnvPreview
		// chooses to surface, no displayed value may match a secret-shaped
		// pattern, and any entry whose key matches a secret-pattern key
		// must already be marked redacted. This assertion does not depend
		// on the implementation's choice of entries — adding or removing
		// keys cannot make it tautological.
		for (const entry of provenance?.env ?? []) {
			expect(isSecretEnvValue(entry.value)).toBe(false);
			if (isSecretEnvKey(entry.key)) {
				expect(entry.redacted).toBe(true);
				expect(entry.value).toBe('[redacted]');
			}
		}
	});

	test('commandText is shell-safe under adversarial tenant and database names', () => {
		const adversarialScopes = [
			{ tenant: 'multi word tenant', database: 'default' },
			{ tenant: 'acme', database: 'db; rm -rf /' },
			{ tenant: 'a$b', database: "it's-fine" },
		];
		const audit = [mcpAuditEntry(2_000_000_000)];
		for (const adversarialScope of adversarialScopes) {
			const provenance = buildMcpStdioProvenance({
				intent: mcpIntent(),
				audit,
				scope: adversarialScope,
				now: 2_000_000_000,
			});
			expect(provenance).not.toBeNull();
			const tokens = parseShellCommand(provenance?.commandText ?? '');
			// Original argv layout: ['axon-server', '--mcp-stdio',
			// '--tenant', tenant, '--database', database]. After parsing the
			// rendered commandText through a shell, tenant/database must
			// arrive as exactly one token each, byte-for-byte equal to the
			// input. Any other behaviour means the displayed recipe would
			// either split arguments on whitespace or trigger metacharacter
			// expansion when copied into a terminal.
			expect(tokens).toEqual([
				'axon-server',
				'--mcp-stdio',
				'--tenant',
				adversarialScope.tenant,
				'--database',
				adversarialScope.database,
			]);
		}
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
