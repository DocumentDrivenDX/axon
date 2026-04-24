import { describe, expect, test } from 'bun:test';

import type { AuditEntry, MutationIntent } from '$lib/api';
import {
	agentIdentityLabel,
	grantVersionLabel,
	isMcpOriginated,
	originMetadataSummary,
	structuredOutcomeSummary,
	toolArgumentsSummary,
	toolNameLabel,
} from '$lib/intent-metadata';

function sampleIntent(): MutationIntent {
	return {
		id: 'mint_test_1',
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
		policyVersion: 1,
		operation: {
			operationKind: 'patch_entity',
			operationHash: 'sha256:preview',
			operation: {
				collection: 'task',
				id: 'task-a',
				expected_version: 1,
				patch: {
					budget_cents: 23000,
					secret: 'super-secret',
					nested: {
						api_token: 'tok_live_123',
					},
				},
			},
		},
		operationHash: 'sha256:preview',
		preImages: [{ kind: 'entity', collection: 'task', id: 'task-a', version: 1 }],
		decision: 'needs_approval',
		approvalState: 'pending',
		approvalRoute: {
			role: 'finance_approver',
			reasonRequired: true,
			separationOfDuties: true,
			deadlineSeconds: 86_400,
		},
		expiresAtNs: '1000000000',
		reviewSummary: {
			summary: 'needs_approval',
			affected_fields: ['budget_cents'],
			policy_explanation: ['needs_approval: large-budget-needs-finance-approval'],
		},
	};
}

function conflictAuditEntry(): AuditEntry {
	return {
		id: 3,
		timestamp_ns: 3_000_000_000,
		collection: '__axon_intents__',
		entity_id: 'mint_test_1',
		version: 0,
		mutation: 'intent.commit',
		data_before: null,
		data_after: null,
		actor: 'finance-agent',
		transaction_id: null,
		metadata: {
			event: 'commit_validation_failed',
			error_code: 'intent_stale',
		},
		intent_lineage: null,
	};
}

describe('intent metadata helpers', () => {
	test('identifies MCP-originated intents and infers a tool name', () => {
		const intent = sampleIntent();
		expect(isMcpOriginated(intent)).toBe(true);
		expect(agentIdentityLabel(intent)).toBe('tool.review-console');
		expect(toolNameLabel(intent)).toBe('review-console');
		expect(grantVersionLabel(intent)).toBe('13');
	});

	test('builds a redacted tool argument summary without leaking values', () => {
		const summary = toolArgumentsSummary(sampleIntent().operation);
		const rendered = JSON.stringify(summary);

		expect(rendered).toContain('budget_cents');
		expect(rendered).toContain('secret');
		expect(rendered).toContain('nested.api_token');
		expect(rendered).not.toContain('23000');
		expect(rendered).not.toContain('super-secret');
		expect(rendered).not.toContain('tok_live_123');
	});

	test('derives conflict outcomes from audit lineage metadata', () => {
		const summary = structuredOutcomeSummary(sampleIntent(), {
			auditEntries: [conflictAuditEntry()],
		});
		expect(summary).toMatchObject({
			origin: 'mcp',
			outcome: 'conflict',
			error_code: 'intent_stale',
			latest_event: 'commit_validation_failed',
		});
	});

	test('includes stable origin metadata for detail panels', () => {
		const summary = originMetadataSummary(sampleIntent());
		expect(summary).toMatchObject({
			origin: 'mcp',
			requester: 'finance-bot',
			agent_identity: 'tool.review-console',
			tool_name: 'review-console',
			delegated_by: 'finance-agent',
			credential_id: 'cred-finance-bot',
			grant_version: '13',
		});
	});
});
