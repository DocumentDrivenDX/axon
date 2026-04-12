import { expect, test } from '@playwright/test';

/**
 * E2E tests for Drop Collection and Delete Entity features.
 *
 * Uses isolated collection names to avoid interference with other test files.
 */

test.describe('Drop collection', () => {
	const COLLECTION_NAME = 'e2e-drop';

	test.beforeAll(async ({ request }) => {
		const response = await request.post(`http://localhost:4170/collections/${COLLECTION_NAME}`, {
			data: {
				schema: {
					description: null,
					version: 1,
					entity_schema: { type: 'object', properties: {} },
					link_types: {},
				},
				actor: 'e2e-test',
			},
		});
		// 201 Created or 409 Conflict (already exists) are both acceptable.
		expect([201, 409]).toContain(response.status());
	});

	test('collections table has Drop button', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		const table = page.locator('table');
		await expect(table).toBeVisible({ timeout: 15000 });

		const row = table.locator('tr').filter({ hasText: COLLECTION_NAME });
		await expect(row).toBeVisible({ timeout: 15000 });

		const dropButton = row.getByRole('button', { name: 'Drop' });
		await expect(dropButton).toBeVisible({ timeout: 15000 });
	});

	test('clicking Drop shows confirmation', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		const table = page.locator('table');
		await expect(table).toBeVisible({ timeout: 15000 });

		const row = table.locator('tr').filter({ hasText: COLLECTION_NAME });
		await expect(row).toBeVisible({ timeout: 15000 });

		await row.getByRole('button', { name: 'Drop' }).click();

		await expect(row.getByText(`Drop ${COLLECTION_NAME}?`)).toBeVisible({ timeout: 15000 });
		await expect(row.getByRole('button', { name: 'Confirm' })).toBeVisible({ timeout: 15000 });
		await expect(row.getByRole('button', { name: 'Cancel' })).toBeVisible({ timeout: 15000 });
	});

	test('clicking Cancel restores Drop button', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		const table = page.locator('table');
		await expect(table).toBeVisible({ timeout: 15000 });

		const row = table.locator('tr').filter({ hasText: COLLECTION_NAME });
		await expect(row).toBeVisible({ timeout: 15000 });

		await row.getByRole('button', { name: 'Drop' }).click();
		await expect(row.getByRole('button', { name: 'Cancel' })).toBeVisible({ timeout: 15000 });

		await row.getByRole('button', { name: 'Cancel' }).click();

		await expect(row.getByRole('button', { name: 'Drop' })).toBeVisible({ timeout: 15000 });
		await expect(row.getByRole('button', { name: 'Confirm' })).not.toBeVisible();
	});

	test('confirming Drop removes the collection', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		const table = page.locator('table');
		await expect(table).toBeVisible({ timeout: 15000 });

		const row = table.locator('tr').filter({ hasText: COLLECTION_NAME });
		await expect(row).toBeVisible({ timeout: 15000 });

		await row.getByRole('button', { name: 'Drop' }).click();
		await expect(row.getByRole('button', { name: 'Confirm' })).toBeVisible({ timeout: 15000 });

		await row.getByRole('button', { name: 'Confirm' }).click();
		await page.waitForLoadState('networkidle');

		// The collection should no longer appear in the table (or the table is gone entirely).
		await expect(page.locator('table').locator('tr').filter({ hasText: COLLECTION_NAME })).not.toBeVisible({
			timeout: 15000,
		});
	});
});

test.describe('Delete entity', () => {
	const COLLECTION_NAME = 'e2e-delete-coll';
	const ENTITY_ID = 'delete-me';

	test.beforeAll(async ({ request }) => {
		// Create the collection.
		const collResp = await request.post(`http://localhost:4170/collections/${COLLECTION_NAME}`, {
			data: {
				schema: {
					description: null,
					version: 1,
					entity_schema: { type: 'object', properties: {} },
					link_types: {},
				},
				actor: 'e2e-test',
			},
		});
		expect([201, 409]).toContain(collResp.status());

		// Create an entity to be deleted.
		const entityResp = await request.post(
			`http://localhost:4170/entities/${COLLECTION_NAME}/${ENTITY_ID}`,
			{
				data: { data: { note: 'to be deleted' }, actor: 'e2e-test' },
			},
		);
		expect([201, 409]).toContain(entityResp.status());
	});

	test('entity detail shows Delete button', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		// Wait for the entity to be auto-selected (entity-meta shows the ID).
		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });

		const deleteButton = detailPanel.getByRole('button', { name: 'Delete' });
		await expect(deleteButton).toBeVisible({ timeout: 15000 });
	});

	test('clicking Delete shows confirmation', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });

		await detailPanel.getByRole('button', { name: 'Delete' }).click();

		await expect(detailPanel.getByText('Delete?')).toBeVisible({ timeout: 15000 });
		await expect(detailPanel.getByRole('button', { name: 'Confirm' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Cancel' })).toBeVisible({
			timeout: 15000,
		});
	});

	test('clicking Cancel restores Delete button', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });

		await detailPanel.getByRole('button', { name: 'Delete' }).click();
		await expect(detailPanel.getByRole('button', { name: 'Cancel' })).toBeVisible({
			timeout: 15000,
		});

		await detailPanel.getByRole('button', { name: 'Cancel' }).click();

		await expect(detailPanel.getByRole('button', { name: 'Delete' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Confirm' })).not.toBeVisible();
	});

	test('confirming Delete removes the entity', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });

		await detailPanel.getByRole('button', { name: 'Delete' }).click();
		await expect(detailPanel.getByRole('button', { name: 'Confirm' })).toBeVisible({
			timeout: 15000,
		});

		await detailPanel.getByRole('button', { name: 'Confirm' }).click();
		await page.waitForLoadState('networkidle');

		// Success message should appear.
		await expect(page.getByText(`Deleted ${ENTITY_ID}.`)).toBeVisible({ timeout: 15000 });

		// Entity should no longer appear in the entity table.
		const entityTable = page
			.locator('section.panel')
			.filter({ hasText: 'Entities' })
			.locator('table');
		const entityRow = entityTable.locator('tr').filter({ hasText: ENTITY_ID });
		await expect(entityRow).not.toBeVisible({ timeout: 15000 });
	});
});
