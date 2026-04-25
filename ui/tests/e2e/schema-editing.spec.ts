import { expect, test } from '@playwright/test';
import {
	createTestCollection,
	createTestDatabase,
	createTestTenant,
	dbCollectionsUrl,
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

	test('creates, edits, and drops a collection through the schema and collection routes', async ({
		page,
	}) => {
		const url = `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/schemas`;
		const collectionName = `books-${Date.now().toString(36)}`;
		await page.goto(url);

		await page.getByLabel('Name').fill(collectionName);
		await page.getByLabel('Entity Schema JSON').fill(`{
  "type": "object",
  "properties": {
    "title": { "type": "string" }
  },
  "required": ["title"]
}`);
		await page.getByRole('button', { name: 'Create Collection' }).click();
		await expect(page.getByText('Collection created.')).toBeVisible({ timeout: 5_000 });
		await expect(page.getByRole('button', { name: new RegExp(collectionName) })).toBeVisible();

		await page.getByRole('button', { name: new RegExp(collectionName) }).click();
		await page.getByRole('button', { name: 'Edit' }).click();
		await page
			.locator('textarea')
			.nth(1)
			.fill(`{
  "collection": "${collectionName}",
  "description": "Edited through E2E",
  "version": 1,
  "entity_schema": {
    "type": "object",
    "properties": {
      "title": { "type": "string" },
      "author": { "type": "string" }
    },
    "required": ["title"]
  },
  "link_types": {}
}`);
		await page.getByRole('button', { name: 'Preview Changes' }).click();
		await expect(page.getByText('Schema Change Preview')).toBeVisible({ timeout: 10_000 });
		await page.getByRole('button', { name: 'Save Schema' }).click();
		await expect(page.getByText('Edited through E2E')).toBeVisible({ timeout: 5_000 });
		await expect(page.getByText('author').first()).toBeVisible();

		await page.goto(dbCollectionsUrl(db));
		const row = page.locator('tr', { hasText: collectionName });
		await expect(row).toBeVisible({ timeout: 5_000 });
		await row.getByRole('button', { name: 'Drop' }).click();
		await row.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.locator('tr', { hasText: collectionName })).toHaveCount(0);
	});
});
