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

	test('collection detail page loads and shows entity count', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		await expect(page.getByRole('heading', { name: COLLECTION_NAME })).toBeVisible({
			timeout: 15000,
		});
		// Entity count paragraph renders with any non-negative count.
		// Using a regex so the test is robust when the server already has entities
		// from a previous test run (in-memory server persists across runs).
		await expect(page.getByText(/\d+ entities/)).toBeVisible({ timeout: 15000 });
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
			.filter({ has: page.locator('h2', { hasText: /^Entities$/ }) })
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
			.filter({ has: page.locator('h2', { hasText: /^Entities$/ }) })
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

test.describe('Entity empty state', () => {
	const EMPTY_COLLECTION = 'e2e-empty-state';

	test.beforeAll(async ({ request }) => {
		const response = await request.post(
			`http://localhost:4170/collections/${EMPTY_COLLECTION}`,
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

	test('new empty collection shows empty state message', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${EMPTY_COLLECTION}`);
		await page.waitForLoadState('networkidle');

		// The empty state message should be visible inside the Create Entity panel.
		await expect(
			page.getByText('This collection is empty. Create the first entity to populate the browser.'),
		).toBeVisible({ timeout: 15000 });

		// The Create Entity panel should be auto-shown.
		const createPanel = page.locator('section.panel').filter({ hasText: 'Create Entity' });
		await expect(createPanel).toBeVisible({ timeout: 15000 });
		await expect(createPanel.getByRole('heading', { name: 'Create Entity' })).toBeVisible();

		// The entity ID input should be visible.
		await expect(createPanel.getByPlaceholder('task-001')).toBeVisible();
	});
});

test.describe('Entity edit/update', () => {
	const EDIT_COLLECTION = 'e2e-edit';
	const EDIT_ENTITY_ID = 'edit-001';

	test.beforeAll(async ({ request }) => {
		// Create the collection.
		const collResp = await request.post(
			`http://localhost:4170/collections/${EDIT_COLLECTION}`,
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
		expect([201, 409]).toContain(collResp.status());

		// Create an entity with initial data.
		const entityResp = await request.post(
			`http://localhost:4170/entities/${EDIT_COLLECTION}/${EDIT_ENTITY_ID}`,
			{
				data: { data: { name: 'original', value: 1 }, actor: 'e2e-test' },
			},
		);
		expect([201, 409]).toContain(entityResp.status());
	});

	test('entity edit mode opens with current data', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${EDIT_COLLECTION}`);
		await page.waitForLoadState('networkidle');

		// Verify the entity appears in the table.
		const entityTable = page
			.locator('section.panel')
			.filter({ has: page.locator('h2', { hasText: /^Entities$/ }) })
			.locator('table');
		await expect(entityTable).toBeVisible({ timeout: 15000 });
		await expect(entityTable.getByText(EDIT_ENTITY_ID)).toBeVisible({ timeout: 15000 });

		// The detail panel auto-selects the first entity — wait for .entity-meta to show the ID.
		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(EDIT_ENTITY_ID)).toBeVisible({ timeout: 15000 });

		// Click the Edit button in the detail panel header.
		await detailPanel.getByRole('button', { name: 'Edit' }).click();

		// Cancel and Save buttons should appear; Edit button should be gone.
		await expect(detailPanel.getByRole('button', { name: 'Cancel' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Save' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Edit' })).not.toBeVisible();

		// The JsonTree editor should be rendered in editing mode (tree-container is visible).
		await expect(detailPanel.locator('.tree-container')).toBeVisible();
	});

	test('cancel edit restores read-only view', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${EDIT_COLLECTION}`);
		await page.waitForLoadState('networkidle');

		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta).toBeVisible({ timeout: 15000 });

		// Enter edit mode.
		await detailPanel.getByRole('button', { name: 'Edit' }).click();
		await expect(detailPanel.getByRole('button', { name: 'Cancel' })).toBeVisible({
			timeout: 15000,
		});

		// Cancel out of edit mode.
		await detailPanel.getByRole('button', { name: 'Cancel' }).click();

		// Edit button should be back; Save button should be gone.
		await expect(detailPanel.getByRole('button', { name: 'Edit' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Save' })).not.toBeVisible();
	});
});

test.describe('Entity pagination', () => {
	test('pagination buttons exist and previous is disabled initially', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		// Wait for the Entities panel to finish loading.
		// Use exact h2 match to avoid matching the detail panel which also contains "entities" text.
		const entitiesPanel = page.locator('section.panel').filter({
			has: page.locator('h2', { hasText: /^Entities$/ }),
		});
		await expect(entitiesPanel).toBeVisible({ timeout: 15000 });

		// Previous button should be visible and disabled on the first page.
		const previousButton = entitiesPanel.getByRole('button', { name: 'Previous' });
		await expect(previousButton).toBeVisible({ timeout: 15000 });
		await expect(previousButton).toBeDisabled();

		// Next button should be visible (disabled when fewer than 50 entities exist).
		const nextButton = entitiesPanel.getByRole('button', { name: 'Next' });
		await expect(nextButton).toBeVisible({ timeout: 15000 });
	});
});
