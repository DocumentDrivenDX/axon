import { expect, test } from '@playwright/test';
import {
	createTestCollection,
	createTestDatabase,
	createTestTenant,
	type TestDatabase,
	type TestTenant,
} from './helpers';

/**
 * Schema editing gap coverage: preview flow, structured vs raw view,
 * collection creation from the workspace.
 */
test.describe('Schema workspace', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'schm');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'items', {
			entity_schema: {
				type: 'object',
				properties: {
					title: { type: 'string' },
				},
				required: ['title'],
			},
		});
	});

	test('schema page renders the existing collection schema', async ({ page }) => {
		const url = `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/schemas`;
		await page.goto(url);
		await expect(page.getByRole('heading', { name: 'Schemas', level: 1 })).toBeVisible();
		// The left pane lists registered collections — 'items' should appear.
		await expect(page.getByRole('button', { name: /items/ }).first()).toBeVisible({
			timeout: 10_000,
		});
		await page.getByRole('button', { name: /items/ }).first().click();
		// The Entity Fields section in structured view should show 'title'.
		await expect(page.getByText(/Entity Fields/)).toBeVisible();
		await expect(page.getByText('title').first()).toBeVisible();
	});

	test('switching to raw JSON view shows the schema payload', async ({ page }) => {
		const url = `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/schemas`;
		await page.goto(url);
		await page.getByRole('button', { name: /items/ }).first().click();
		await page.getByRole('button', { name: 'Raw JSON' }).click();
		// The raw pane should contain the collection name and a field definition.
		await expect(page.locator('pre').first()).toContainText('"collection"');
		await expect(page.locator('pre').first()).toContainText('"title"');
	});
});
