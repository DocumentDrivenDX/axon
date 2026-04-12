import { expect, test } from '@playwright/test';

/**
 * E2E tests for entity CRUD against a real axon-server.
 *
 * Uses collection "e2e-entities" (unique per file) to avoid interference.
 * The collection is created once in beforeAll, then entity tests run in order.
 */

const COLLECTION_NAME = 'e2e-entities';
const ENTITY_ID = 'entity-001';
const ENTITY_DATA = JSON.stringify({ name: 'Test Entity', value: 42 }, null, 2);

test.describe('Entity CRUD workflow', () => {
	test.beforeAll(async ({ request }) => {
		// Create the collection via the API directly so we don't depend on the
		// schemas UI test completing first.
		const response = await request.post(
			`http://localhost:4170/collections/${COLLECTION_NAME}`,
			{
				data: {
					schema: {
						description: null,
						version: 1,
						entity_schema: { type: 'object', properties: {} },
						link_types: {},
					},
					actor: 'e2e-test',
				},
			},
		);
		// 201 Created or 409 Conflict (already exists) are both acceptable.
		expect([201, 409]).toContain(response.status());
	});

	test('collection detail page loads with 0 entities', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		await expect(page.getByRole('heading', { name: COLLECTION_NAME })).toBeVisible({
			timeout: 15000,
		});
		await expect(page.getByText('0 entities')).toBeVisible({ timeout: 15000 });
	});

	test('create entity via the UI form', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		// The create form is auto-shown when the collection is empty.
		// If it's not visible (entities exist from a prior run), open it manually.
		const createPanel = page.locator('section.panel').filter({ hasText: 'Create Entity' });
		const panelVisible = await createPanel.isVisible().catch(() => false);

		if (!panelVisible) {
			const toggleButton = page
				.locator('.page-header')
				.getByRole('button', { name: 'Create Entity' });
			await expect(toggleButton).toBeVisible({ timeout: 10000 });
			await toggleButton.click();
		}

		await expect(createPanel).toBeVisible({ timeout: 10000 });

		// Fill in Entity ID.
		const idInput = createPanel.getByPlaceholder('task-001');
		await expect(idInput).toBeVisible();
		await idInput.fill(ENTITY_ID);

		// Fill in Entity JSON data.
		const jsonTextarea = createPanel.locator('textarea');
		await expect(jsonTextarea).toBeVisible();
		await jsonTextarea.fill(ENTITY_DATA);

		// Submit the form.
		await createPanel.getByRole('button', { name: 'Create Entity' }).click();

		// Verify success message.
		await expect(page.getByText(`Created ${ENTITY_ID}.`)).toBeVisible({ timeout: 15000 });
	});

	test('created entity appears in the entity table', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		// The entity table should now show our entity.
		const entityTable = page
			.locator('section.panel')
			.filter({ hasText: 'Entities' })
			.locator('table');
		await expect(entityTable).toBeVisible({ timeout: 15000 });
		await expect(entityTable.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });
	});

	test('clicking entity row shows entity detail', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		// Wait for the entity table to be populated.
		const entityTable = page
			.locator('section.panel')
			.filter({ hasText: 'Entities' })
			.locator('table');
		await expect(entityTable).toBeVisible({ timeout: 15000 });

		// Click the entity row.
		const entityRow = entityTable.locator('tr').filter({ hasText: ENTITY_ID });
		await expect(entityRow).toBeVisible({ timeout: 15000 });
		await entityRow.click();

		// The detail panel is the second panel in the two-column grid.
		// The page auto-selects the first entity on load, so the heading shows the entity ID.
		const detailPanel = page.locator('.two-column section.panel').nth(1);
		await expect(detailPanel.getByRole('heading', { name: ENTITY_ID })).toBeVisible({
			timeout: 15000,
		});

		// The entity meta section should show the ID as a code element.
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });
	});
});
