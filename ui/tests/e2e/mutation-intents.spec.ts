import { type Page, expect, test } from '@playwright/test';
import {
	TASK_COLLECTION,
	type TestDatabase,
	dbCollectionUrl,
	patchBudgetRecordAs,
	routeGraphqlAs,
	seedIntentCollection,
} from './helpers';

const COLLECTION = TASK_COLLECTION;
const ENTITY_ID = 'task-a';

type PreviewPayload = {
	data?: {
		previewMutation?: {
			decision?: string;
			intentToken?: string | null;
			intent?: { id?: string } | null;
		};
	};
};

async function openEntityEditor(page: Page, db: TestDatabase) {
	await page.goto(dbCollectionUrl(db, COLLECTION));
	await expect(page.locator('.entity-rail tbody tr', { hasText: ENTITY_ID })).toBeVisible({
		timeout: 10_000,
	});
	await page.getByRole('button', { name: 'Edit' }).click();
}

async function fillJsonField(page: Page, field: string, value: string) {
	const input = page.locator('.tree-row', { hasText: field }).locator('input.leaf-input');
	await expect(input).toBeVisible();
	await input.fill(value);
}

async function previewIntent(page: Page): Promise<PreviewPayload> {
	const responsePromise = page.waitForResponse(
		(response) =>
			response.url().endsWith('/graphql') &&
			response.request().method() === 'POST' &&
			(response.request().postData() ?? '').includes('previewMutation'),
	);
	await page.getByRole('button', { name: 'Preview' }).click();
	const response = await responsePromise;
	return (await response.json()) as PreviewPayload;
}

test.describe('Mutation intents', () => {
	test('renders and commits an allowed mutation intent without showing the token', async ({
		page,
		request,
	}) => {
		const db = await seedIntentCollection(request, 'intent-allow');
		await routeGraphqlAs(page, 'finance-agent');
		await openEntityEditor(page, db);
		await fillJsonField(page, 'budget_cents', '6000');

		const payload = await previewIntent(page);
		expect(payload.data?.previewMutation?.decision).toBe('allow');
		const token = payload.data?.previewMutation?.intentToken;
		expect(token).toEqual(expect.any(String));

		const modal = page.getByTestId('mutation-intent-modal');
		await expect(modal).toBeVisible();
		await expect(modal).toContainText('allow');
		await expect(modal).toContainText('budget_cents');
		await expect(modal).toContainText(`${COLLECTION}/${ENTITY_ID}`);
		await expect(page.getByTestId('intent-policy-explanation')).toContainText('finance-update');
		await expect(modal).not.toContainText(token as string);

		await page.getByTestId('intent-commit').click();
		await expect(page.getByText('Saved v2.')).toBeVisible({ timeout: 10_000 });
		await expect(modal).toContainText('committed');
	});

	test('renders a needs-approval preview with approval route details', async ({
		page,
		request,
	}) => {
		const db = await seedIntentCollection(request, 'intent-approval');
		await routeGraphqlAs(page, 'finance-agent');
		await openEntityEditor(page, db);
		await fillJsonField(page, 'budget_cents', '20000');

		const payload = await previewIntent(page);
		expect(payload.data?.previewMutation?.decision).toBe('needs_approval');
		const token = payload.data?.previewMutation?.intentToken;
		expect(token).toEqual(expect.any(String));

		const modal = page.getByTestId('mutation-intent-modal');
		await expect(modal).toContainText('needs_approval');
		await expect(modal).toContainText('finance_approver');
		await expect(modal).toContainText('large-budget-needs-finance-approval');
		await expect(modal).not.toContainText(token as string);
		await expect(page.getByTestId('intent-commit')).toBeDisabled();
	});

	test('renders a denied preview without an executable intent token', async ({ page, request }) => {
		const db = await seedIntentCollection(request, 'intent-deny', true);
		await routeGraphqlAs(page, 'finance-agent');
		await openEntityEditor(page, db);
		await fillJsonField(page, 'secret', 'bravo');

		const payload = await previewIntent(page);
		expect(payload.data?.previewMutation?.decision).toBe('deny');
		expect(payload.data?.previewMutation?.intentToken).toBeNull();

		const modal = page.getByTestId('mutation-intent-modal');
		await expect(modal).toContainText('deny');
		await expect(modal).toContainText('secret');
		await expect(modal).toContainText('finance-agent-cannot-write-secret');
		await expect(modal).not.toContainText('intentToken');
		await expect(page.getByTestId('intent-commit')).toBeDisabled();
	});

	test('renders stale pre-image conflict details after preview drift', async ({
		page,
		request,
	}) => {
		const db = await seedIntentCollection(request, 'intent-stale');
		await routeGraphqlAs(page, 'finance-agent');
		await openEntityEditor(page, db);
		await fillJsonField(page, 'status', 'ready');

		const payload = await previewIntent(page);
		expect(payload.data?.previewMutation?.decision).toBe('allow');

		await patchBudgetRecordAs(request, db, 'finance-agent', COLLECTION, ENTITY_ID, 5000);

		await page.getByTestId('intent-commit').click();
		const error = page.getByTestId('intent-error');
		await expect(error).toContainText('intent_stale', { timeout: 10_000 });
		await expect(error).toContainText('pre_image');
		await expect(error).toContainText('1');
		await expect(error).toContainText('2');
	});

	test('renders mismatch GraphQL error payloads returned during commit', async ({
		page,
		request,
	}) => {
		const db = await seedIntentCollection(request, 'intent-mismatch');
		let mockMismatchCommit = false;
		await routeGraphqlAs(page, 'finance-agent', (postData) => {
			if (!mockMismatchCommit || !postData.includes('commitMutationIntent')) return null;
			return {
				errors: [
					{
						message: 'committed operation does not match previewed intent',
						extensions: {
							code: 'intent_mismatch',
							stale: [
								{
									dimension: 'operation_hash',
									expected: 'sha256:preview',
									actual: 'sha256:commit',
									path: 'operation',
								},
							],
						},
					},
				],
			};
		});
		await openEntityEditor(page, db);
		await fillJsonField(page, 'status', 'ready');

		const payload = await previewIntent(page);
		expect(payload.data?.previewMutation?.decision).toBe('allow');

		mockMismatchCommit = true;
		await page.getByTestId('intent-commit').click();
		const error = page.getByTestId('intent-error');
		await expect(error).toContainText('intent_mismatch');
		await expect(error).toContainText('operation_hash');
		await expect(error).toContainText('sha256:preview');
		await expect(error).toContainText('sha256:commit');
	});

	test('re-previews after a stale commit, creates a new intent ID, and preserves the lineage link', async ({
		page,
		request,
	}) => {
		const db = await seedIntentCollection(request, 'intent-stale-relink');
		await routeGraphqlAs(page, 'finance-agent');
		await openEntityEditor(page, db);
		await fillJsonField(page, 'status', 'ready');

		const initialPayload = await previewIntent(page);
		expect(initialPayload.data?.previewMutation?.decision).toBe('allow');
		const initialIntentId = initialPayload.data?.previewMutation?.intent?.id;
		expect(initialIntentId).toEqual(expect.any(String));

		// Drive a real intent_stale by patching the entity out from under
		// the previewed pre-image. The committed pre-image dimension
		// becomes stale (version 1 → version 2) which is exactly what the
		// re-preview workflow must guide the operator through.
		await patchBudgetRecordAs(request, db, 'finance-agent', COLLECTION, ENTITY_ID, 5000);

		await page.getByTestId('intent-commit').click();
		const error = page.getByTestId('intent-error');
		await expect(error).toContainText('intent_stale');
		// Commit must be disabled while the active token is bound to a
		// stale pre-image: the user's only forward path is "Re-preview".
		await expect(page.getByTestId('intent-commit')).toBeDisabled();

		const rePreviewResponse = page.waitForResponse(
			(response) =>
				response.url().endsWith('/graphql') &&
				response.request().method() === 'POST' &&
				(response.request().postData() ?? '').includes('previewMutation'),
		);
		await page.getByTestId('intent-re-preview').click();
		const refreshed = (await (await rePreviewResponse).json()) as PreviewPayload;
		const refreshedIntentId = refreshed.data?.previewMutation?.intent?.id;
		expect(refreshedIntentId).toEqual(expect.any(String));
		expect(refreshedIntentId).not.toBe(initialIntentId);

		// Lineage banner shows the superseded intent ID with a link to
		// its audit detail. This is the visible "preserves lineage link"
		// half of the acceptance criterion; the other half is the new
		// intent ID being distinct, which we just asserted above.
		const lineage = page.getByTestId('intent-lineage-supersedes');
		await expect(lineage).toBeVisible();
		await expect(lineage).toContainText(initialIntentId as string);
		const lineageLink = page.getByTestId('intent-lineage-link');
		await expect(lineageLink).toHaveAttribute(
			'href',
			new RegExp(`/intents/${initialIntentId}$`),
		);

		// The fresh token is bound to the newly observed pre-image
		// (version 2), so commit becomes available again and succeeds.
		await page.getByTestId('intent-commit').click();
		await expect(page.getByText(/Saved v\d+\./)).toBeVisible({ timeout: 10_000 });
	});
});
