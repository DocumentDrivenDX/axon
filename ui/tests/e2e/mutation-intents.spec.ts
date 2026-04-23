import { type APIRequestContext, type Page, expect, test } from '@playwright/test';
import {
	type TestDatabase,
	createTestCollection,
	createTestDatabase,
	createTestEntity,
	createTestTenant,
	dbCollectionUrl,
	updateTestEntity,
} from './helpers';

const COLLECTION = 'task';
const ENTITY_ID = 'task-a';

type PreviewPayload = {
	data?: {
		previewMutation?: {
			decision?: string;
			intentToken?: string | null;
		};
	};
};

type GraphqlMock = (
	postData: string,
) => Promise<Record<string, unknown> | null> | Record<string, unknown> | null;

function taskSchema() {
	return {
		type: 'object',
		properties: {
			title: { type: 'string' },
			budget_cents: { type: 'integer' },
			secret: { type: 'string' },
			status: { type: 'string' },
		},
	};
}

function intentAccessControl() {
	return {
		read: { allow: [{ name: 'all-read' }] },
		create: { allow: [{ name: 'all-create' }] },
		update: { allow: [{ name: 'all-update' }] },
		fields: {
			secret: {
				write: {
					deny: [
						{
							name: 'finance-agent-cannot-write-secret',
							when: { subject: 'user_id', eq: 'finance-agent' },
						},
					],
				},
			},
		},
		envelopes: {
			write: [
				{
					name: 'large-budget-needs-finance-approval',
					when: {
						all: [{ operation: 'update' }, { field: 'budget_cents', gt: 10000 }],
					},
					decision: 'needs_approval',
					approval: {
						role: 'finance_approver',
						reason_required: true,
						deadline_seconds: 86400,
						separation_of_duties: true,
					},
				},
			],
		},
	};
}

async function seedIntentCollection(
	request: APIRequestContext,
	prefix: string,
	includeSecret = false,
): Promise<TestDatabase> {
	const tenant = await createTestTenant(request, prefix);
	const db = await createTestDatabase(request, tenant);
	await createTestCollection(request, db, COLLECTION, {
		entity_schema: taskSchema(),
		access_control: intentAccessControl(),
	});
	const entity: Record<string, unknown> = {
		title: 'Budget request',
		budget_cents: 5000,
		status: 'draft',
	};
	if (includeSecret) {
		entity.secret = 'alpha';
	}
	await createTestEntity(request, db, COLLECTION, ENTITY_ID, entity);
	return db;
}

async function routeGraphqlAs(page: Page, actor: string, mock?: GraphqlMock) {
	await page.route('**/graphql', async (route) => {
		const postData = route.request().postData() ?? '';
		const mocked = mock ? await mock(postData) : null;
		if (mocked) {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(mocked),
			});
			return;
		}

		await route.continue({
			headers: {
				...route.request().headers(),
				'x-axon-actor': actor,
			},
		});
	});
}

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
		await expect(page.getByTestId('intent-policy-explanation')).toContainText('all-update');
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

		await updateTestEntity(
			request,
			db,
			COLLECTION,
			ENTITY_ID,
			{
				title: 'Budget request',
				budget_cents: 5000,
				status: 'external',
			},
			1,
		);

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
});
