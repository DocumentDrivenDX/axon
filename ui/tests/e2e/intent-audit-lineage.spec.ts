import { expect, test } from '@playwright/test';

import {
	SCN017_COLLECTIONS,
	TASK_COLLECTION,
	activateProposedPolicy,
	approveIntent,
	createBudgetRecord,
	dbIntentUrl,
	dbIntentsUrl,
	graphqlPath,
	patchBudgetRecordAs,
	previewBudgetIntent,
	proposedPolicyDraftDenyHigh,
	routeGraphqlAs,
	seedApprovalCollections,
	seedIntentStates,
	seedScn017PolicyUiFixture,
} from './helpers';

test.describe('Intent audit lineage', () => {
	test('shows delegated MCP intent metadata in the inbox and detail panels', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'intent-audit-lineage');
		const ids = await seedIntentStates(request, db);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentsUrl(db));

		const row = page.getByTestId(`intent-row-${ids.approveTarget}`);
		await expect(row).toBeVisible();
		await expect(page.getByTestId(`intent-origin-${ids.approveTarget}`)).toContainText('MCP');
		await expect(page.getByTestId(`intent-origin-${ids.approveTarget}`)).toContainText(
			'tool.review-console',
		);
		await expect(page.getByTestId(`intent-origin-${ids.approveTarget}`)).toContainText(
			'finance-agent',
		);
		await expect(page.getByTestId(`intent-outcome-${ids.approveTarget}`)).toContainText(
			'needs_approval',
		);
		await expect(page.getByTestId(`intent-outcome-${ids.approveTarget}`)).toContainText('pending');

		await row.click();
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('tool.review-console');
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('finance-agent');
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('cred-finance-bot');
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('13');
		await expect(page.getByTestId('intent-inline-tool-arguments')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-inline-tool-arguments')).not.toContainText('23000');
		await expect(page.getByTestId('intent-inline-structured-outcome')).toContainText(
			'needs_approval',
		);
		await expect(page.getByTestId('intent-inline-structured-outcome')).toContainText('pending');

		await page.getByTestId('intent-open-detail').click();
		await expect(page).toHaveURL(dbIntentUrl(db, ids.approveTarget));
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('tool.review-console');
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('finance-agent');
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('cred-finance-bot');
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('13');
		await expect(page.getByTestId('intent-tool-arguments')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-tool-arguments')).not.toContainText('23000');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('needs_approval');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('pending');
	});

	test('shows conflict outcomes for stale MCP-originated intent commits', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'intent-audit-conflict');
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-conflict');
		const staleIntent = await previewBudgetIntent(request, db, 'task-conflict', 27_000, {
			agentId: 'mcp.gateway',
			grantVersion: 21,
		});
		await approveIntent(request, db, staleIntent.intentId);
		await patchBudgetRecordAs(request, db, 'finance-agent', TASK_COLLECTION, 'task-conflict', 5100);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(dbIntentUrl(db, staleIntent.intentId));

		await expect(page.getByTestId('intent-origin-metadata')).toContainText('mcp.gateway');
		await expect(page.getByTestId('intent-tool-arguments')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-tool-arguments')).not.toContainText('27000');

		await page.getByTestId('intent-commit-token').fill(staleIntent.token);
		await page.getByTestId('intent-commit-action').click();
		await expect(page.getByTestId('intent-commit-error')).toContainText('intent_stale');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('conflict');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('intent_stale');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText(
			'commit_validation_failed',
		);
	});

	test('records administrative audit evidence for policy activation', async ({ request }) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'schema-policy-audit');
		const proposed = proposedPolicyDraftDenyHigh();
		const result = await activateProposedPolicy(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
			proposed,
			{ actor: 'admin' },
		);
		expect(result.schema.version).toBe(2);

		// Probe the audit log for the schema_update entry on the invoices
		// collection. Activation must record old + new schema/policy versions.
		const response = await request.post(graphqlPath(fixture.db), {
			data: {
				query: `query($collection: String!) {
					auditLog(collection: $collection, operation: "schema.update") {
						totalCount
						edges { node { metadata } }
					}
				}`,
				variables: { collection: SCN017_COLLECTIONS.invoices },
			},
		});
		const body = (await response.json()) as {
			data?: {
				auditLog?: {
					totalCount: number;
					edges: Array<{ node: { metadata: Record<string, string> } }>;
				};
			};
			errors?: unknown;
		};
		expect(response.ok(), `${response.status()} ${JSON.stringify(body)}`).toBe(true);
		expect(body.errors ?? null).toBeNull();
		const auditLog = body.data?.auditLog;
		expect(auditLog?.totalCount ?? 0).toBeGreaterThanOrEqual(1);
		// The most recent schema.update entry corresponds to our activation.
		const metadata = auditLog?.edges.at(-1)?.node.metadata ?? {};
		expect(metadata.old_schema_version).toBe('1');
		expect(metadata.new_schema_version).toBe('2');
		expect(metadata.old_policy_version).toBe('1');
		expect(metadata.new_policy_version).toBe('2');
	});
});
