import { type Page, expect, test } from '@playwright/test';
import {
	EXPENSE_COLLECTION,
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
	updateApprovalRole,
} from './helpers';

async function selectStatus(page: Page, status: string) {
	await page.getByRole('tab', { name: status }).click();
}

test.describe('Approval inbox', () => {
	test('lists scoped intents across review states and opens detail', async ({ page, request }) => {
		const db = await seedApprovalCollections(request, 'approval-inbox');
		const ids = await seedIntentStates(request, db);
		const foreignDb = await seedApprovalCollections(request, 'approval-foreign');
		await createBudgetRecord(request, foreignDb, TASK_COLLECTION, 'task-foreign');
		const foreign = await previewBudgetIntent(request, foreignDb, 'task-foreign', 20_000);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentsUrl(db));

		await expect(page.getByRole('heading', { name: 'Mutation Intents' })).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.pending}`)).toBeVisible();
		await expect(page.getByText(foreign.intentId)).toHaveCount(0);
		await selectStatus(page, 'history');
		await expect(page.getByTestId(`intent-row-${ids.approved}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.rejected}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.expired}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.committed}`)).toBeVisible();

		for (const [status, intentId] of [
			['approved', ids.approved],
			['rejected', ids.rejected],
			['expired', ids.expired],
			['committed', ids.committed],
		] as const) {
			await selectStatus(page, status);
			await expect(page.getByTestId(`intent-row-${intentId}`)).toBeVisible();
		}

		await selectStatus(page, 'approved');
		await page.getByTestId(`intent-row-${ids.approved}`).click();
		await page.getByTestId('intent-open-detail').click();
		const detail = page.getByTestId('intent-detail');
		await expect(detail).toContainText(ids.approved);
		await expect(detail).toContainText('approved');
		await expect(detail).toContainText('large-budget-needs-finance-approval');
		await expect(detail).toContainText('task/task-approved');
		await expect(page.getByTestId('intent-bindings')).toContainText('cred-finance-agent');
		await expect(page.getByTestId('intent-bindings')).toContainText('11');
		await expect(page.getByTestId('intent-diff')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-audit-trail')).toContainText('intent.approve');
		await expect(page.getByTestId('intent-audit-trail')).toContainText('approved');
		await expect(page.getByTestId('intent-deep-links')).toContainText('Open audit log');
	});

	test('supports dense filters, keyboard selection, and inline review without leaving inbox', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'approval-filters');
		const ids = await seedIntentStates(request, db);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentsUrl(db));

		await page.getByTestId('intent-filter-requester').fill('finance-bot');
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.pending}`)).toHaveCount(0);

		await page.getByTestId('intent-filter-requester').fill('');
		await page.getByTestId('intent-filter-subject').fill('expense-ui-reject');
		await page.getByTestId('intent-filter-role').selectOption('finance_approver');
		await page.getByTestId('intent-filter-risk').fill('large-budget-needs-finance-approval');
		await page.getByTestId('intent-filter-collection').selectOption(EXPENSE_COLLECTION);
		await page.getByTestId('intent-filter-origin').selectOption('mcp.gateway');
		await expect(page.getByTestId(`intent-row-${ids.rejectTarget}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toHaveCount(0);

		await page.getByTestId('intent-filter-age').selectOption('older_than_day');
		await expect(page.getByTestId('intent-empty')).toBeVisible();
		await page.getByTestId('intent-filter-age').selectOption('last_hour');
		await expect(page.getByTestId(`intent-row-${ids.rejectTarget}`)).toBeVisible();
		await page.getByRole('button', { name: 'Clear filters' }).click();

		await page.getByTestId('intent-inbox-grid').focus();
		await page.keyboard.press('Home');
		await page.keyboard.press('ArrowDown');
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toHaveAttribute(
			'aria-selected',
			'true',
		);
		await page.getByTestId('intent-inline-approve').focus();
		await page.keyboard.press('Enter');
		await expect(page.getByTestId('intent-inline-reason-error')).toContainText(
			'Approval reason is required',
		);
		await expect(page.getByTestId('intent-inline-reason')).toBeFocused();
		await page.getByTestId('intent-inline-reason').fill('approved in inbox');
		await page.getByTestId('intent-inline-approve').click();
		await expect(page.getByTestId('intent-inline-message')).toContainText('Intent approved.');
		await expect(page).toHaveURL(dbIntentsUrl(db));
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toHaveCount(0);

		await page.getByTestId(`intent-row-${ids.rejectTarget}`).click();
		await page.getByTestId('intent-inline-reason').fill('not enough context');
		await page.getByTestId('intent-inline-reject').click();
		await expect(page.getByTestId('intent-inline-message')).toContainText('Intent rejected.');
		await expect(page).toHaveURL(dbIntentsUrl(db));
		await expect(page.getByTestId(`intent-row-${ids.rejectTarget}`)).toHaveCount(0);
	});

	test('approves and rejects pending intents from the detail route', async ({ page, request }) => {
		const db = await seedApprovalCollections(request, 'approval-actions');
		const ids = await seedIntentStates(request, db);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentUrl(db, ids.approveTarget));
		await page.getByTestId('intent-reason').fill('approved in inbox');
		await page.getByTestId('intent-approve').click();
		await expect(page.getByText('Intent approved.')).toBeVisible({ timeout: 10_000 });
		await expect(page.getByTestId('intent-detail')).toContainText('approved');

		await page.goto(dbIntentUrl(db, ids.rejectTarget));
		await page.getByTestId('intent-reason').fill('not enough context');
		await page.getByTestId('intent-reject').click();
		await expect(page.getByText('Intent rejected.')).toBeVisible({ timeout: 10_000 });
		await expect(page.getByTestId('intent-detail')).toContainText('rejected');
	});

	test('shows authorization failures without clearing the entered reason', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'approval-errors');
		await seedIntentStates(request, db);
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-self-approve');
		const selfApproval = await previewBudgetIntent(request, db, 'task-self-approve', 25_000, {
			actor: 'finance-approver',
			grantVersion: 17,
			tenantRole: 'finance_approver',
		});
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-lost-role');
		const lostRole = await previewBudgetIntent(request, db, 'task-lost-role', 26_000, {
			grantVersion: 19,
		});

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentUrl(db, selfApproval.intentId));
		await page.getByTestId('intent-reason').fill('self review attempt');
		await page.getByTestId('intent-approve').click();
		await expect(page.getByTestId('intent-action-error')).toContainText(
			'intent_authorization_failed',
		);
		await expect(page.getByTestId('intent-action-error')).toContainText(
			'cannot review their own mutation intent',
		);
		await expect(page.getByTestId('intent-reason')).toHaveValue('self review attempt');

		await updateApprovalRole(request, db, 'finance-approver', 'contractor');
		await page.goto(dbIntentUrl(db, lostRole.intentId));
		await page.getByTestId('intent-reason').fill('lost role attempt');
		await page.getByTestId('intent-approve').click();
		await expect(page.getByTestId('intent-action-error')).toContainText(
			'intent_authorization_failed',
		);
		await expect(page.getByTestId('intent-action-error')).toContainText(
			'does not satisfy required approver role',
		);
		await expect(page.getByTestId('intent-reason')).toHaveValue('lost role attempt');
	});

	test('shows disabled action states for rejected, expired, committed, and stale intents', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'approval-disabled');
		const ids = await seedIntentStates(request, db);
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-stale');
		const staleIntent = await previewBudgetIntent(request, db, 'task-stale', 27_000, {
			grantVersion: 21,
		});
		await approveIntent(request, db, staleIntent.intentId);
		await patchBudgetRecordAs(request, db, 'finance-agent', TASK_COLLECTION, 'task-stale', 5100);

		await routeGraphqlAs(page, 'finance-agent');

		for (const [intentId, reviewStatus, commitStatus] of [
			[
				ids.rejected,
				'Rejected intents cannot be reviewed again.',
				'Rejected intents cannot be committed.',
			],
			[
				ids.expired,
				'Expired intents cannot be approved or rejected.',
				'Expired intents cannot be committed.',
			],
			[
				ids.committed,
				'Committed intents are already consumed.',
				'Committed intents cannot be committed again.',
			],
		] as const) {
			await page.goto(dbIntentUrl(db, intentId));
			await expect(page.getByTestId('intent-review-status')).toContainText(reviewStatus);
			await expect(page.getByTestId('intent-commit-status')).toContainText(commitStatus);
			await expect(page.getByTestId('intent-approve')).toBeDisabled();
			await expect(page.getByTestId('intent-reject')).toBeDisabled();
			await expect(page.getByTestId('intent-commit-action')).toBeDisabled();
		}

		await page.goto(dbIntentUrl(db, staleIntent.intentId));
		await page.getByTestId('intent-commit-token').fill(staleIntent.token);
		await page.getByTestId('intent-commit-action').click();
		await expect(page.getByTestId('intent-commit-error')).toContainText('intent_stale');
		await expect(page.getByTestId('intent-commit-status')).toContainText(
			'disabled because the latest validation returned intent_stale',
		);
		await expect(page.getByTestId('intent-commit-token')).toBeDisabled();
		await expect(page.getByTestId('intent-commit-action')).toBeDisabled();
	});

	test('distinguishes loading, empty, and error states', async ({ page, request }) => {
		const db = await seedApprovalCollections(request, 'approval-states');
		const ids = await seedIntentStates(request, db);
		let releaseLoad!: () => void;
		const loadGate = new Promise<void>((resolve) => {
			releaseLoad = resolve;
		});

		await page.route('**/graphql', async (route) => {
			const body = route.request().postDataJSON() as { query?: string };
			if (body.query?.includes('pendingMutationIntents')) {
				await loadGate;
			}
			await route.continue({
				headers: {
					...route.request().headers(),
					'x-axon-actor': 'finance-approver',
				},
			});
		});

		const navigation = page.goto(dbIntentsUrl(db));
		await expect(page.getByTestId('intent-loading')).toBeVisible();
		releaseLoad();
		await navigation;
		await expect(page.getByTestId(`intent-row-${ids.pending}`)).toBeVisible();

		await page.getByTestId('intent-filter-subject').fill('does-not-exist');
		await expect(page.getByTestId('intent-empty')).toBeVisible();

		await page.unroute('**/graphql');
		await page.route('**/graphql', async (route) => {
			const body = route.request().postDataJSON() as { query?: string };
			if (body.query?.includes('pendingMutationIntents')) {
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({
						errors: [{ message: 'forced inbox error' }],
					}),
				});
				return;
			}
			await route.continue({
				headers: {
					...route.request().headers(),
					'x-axon-actor': 'finance-approver',
				},
			});
		});

		await page.reload();
		await expect(page.getByTestId('intent-error')).toContainText('forced inbox error');
	});
});
