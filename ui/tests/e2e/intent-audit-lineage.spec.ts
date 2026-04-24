import { expect, test } from '@playwright/test';

import {
	TASK_COLLECTION,
	approveIntent,
	createBudgetRecord,
	dbIntentUrl,
	dbIntentsUrl,
	patchBudgetRecordAs,
	previewBudgetIntent,
	routeGraphqlAs,
	seedApprovalCollections,
	seedIntentStates,
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
});
