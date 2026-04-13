import { expect, test } from '@playwright/test';

/**
 * E2E tests for entity CRUD against a real axon-server.
 *
 * Uses collection "e2e-entities" (unique per file) to avoid interference.
 * Uses relative URLs so the baseURL from the active Playwright config applies.
 */

const COLLECTION_NAME = 'e2e-entities';
const ENTITY_ID = 'entity-001';
const ENTITY_DATA = JSON.stringify({ name: 'Test Entity', value: 42 }, null, 2);

test.describe('Entity CRUD workflow', () => {
	test.beforeAll(async ({ request }) => {
		// Create the collection via the API directly so we don't depend on the
		// schemas UI test completing first.
		const response = await request.post(`/collections/${COLLECTION_NAME}`, {
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

	test('collection detail page loads and shows entity count', async ({ page }) => {
		await page.goto(`/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		await expect(page.getByRole('heading', { name: COLLECTION_NAME })).toBeVisible({
			timeout: 15000,
		});
		// Entity count paragraph renders with any non-negative count.
		await expect(page.getByText(/\d+ entities/)).toBeVisible({ timeout: 15000 });
	});

	test('create entity via the UI form', async ({ page }) => {
		await page.goto(`/ui/collections/${COLLECTION_NAME}`);
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
		await page.goto(`/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		const entityTable = page
			.locator('section.panel')
			.filter({ has: page.locator('h2', { hasText: /^Entities$/ }) })
			.locator('table');
		await expect(entityTable).toBeVisible({ timeout: 15000 });
		await expect(entityTable.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });
	});

	test('clicking entity row shows entity detail', async ({ page }) => {
		await page.goto(`/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		const entityTable = page
			.locator('section.panel')
			.filter({ has: page.locator('h2', { hasText: /^Entities$/ }) })
			.locator('table');
		await expect(entityTable).toBeVisible({ timeout: 15000 });

		const entityRow = entityTable.locator('tr').filter({ hasText: ENTITY_ID });
		await expect(entityRow).toBeVisible({ timeout: 15000 });
		await entityRow.click();

		const detailPanel = page.locator('.two-column section.panel').nth(1);
		await expect(detailPanel.getByRole('heading', { name: ENTITY_ID })).toBeVisible({
			timeout: 15000,
		});

		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(ENTITY_ID)).toBeVisible({ timeout: 15000 });
	});
});

test.describe('Entity empty state', () => {
	const EMPTY_COLLECTION = 'e2e-empty-state';

	test.beforeAll(async ({ request }) => {
		const response = await request.post(`/collections/${EMPTY_COLLECTION}`, {
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
		expect([201, 409]).toContain(response.status());
	});

	test('new empty collection shows empty state message', async ({ page }) => {
		await page.goto(`/ui/collections/${EMPTY_COLLECTION}`);
		await page.waitForLoadState('networkidle');

		await expect(
			page.getByText('This collection is empty. Create the first entity to populate the browser.'),
		).toBeVisible({ timeout: 15000 });

		const createPanel = page.locator('section.panel').filter({ hasText: 'Create Entity' });
		await expect(createPanel).toBeVisible({ timeout: 15000 });
		await expect(createPanel.getByRole('heading', { name: 'Create Entity' })).toBeVisible();
		await expect(createPanel.getByPlaceholder('task-001')).toBeVisible();
	});
});

test.describe('Entity edit/update', () => {
	const EDIT_COLLECTION = 'e2e-edit';
	const EDIT_ENTITY_ID = 'edit-001';

	test.beforeAll(async ({ request }) => {
		const collResp = await request.post(`/collections/${EDIT_COLLECTION}`, {
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

		const entityResp = await request.post(`/entities/${EDIT_COLLECTION}/${EDIT_ENTITY_ID}`, {
			data: { data: { name: 'original', value: 1 }, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(entityResp.status());
	});

	test('entity edit mode opens with current data', async ({ page }) => {
		await page.goto(`/ui/collections/${EDIT_COLLECTION}`);
		await page.waitForLoadState('networkidle');

		const entityTable = page
			.locator('section.panel')
			.filter({ has: page.locator('h2', { hasText: /^Entities$/ }) })
			.locator('table');
		await expect(entityTable).toBeVisible({ timeout: 15000 });
		await expect(entityTable.getByText(EDIT_ENTITY_ID)).toBeVisible({ timeout: 15000 });

		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta.getByText(EDIT_ENTITY_ID)).toBeVisible({ timeout: 15000 });

		await detailPanel.getByRole('button', { name: 'Edit' }).click();

		await expect(detailPanel.getByRole('button', { name: 'Cancel' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Save' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Edit' })).not.toBeVisible();
		await expect(detailPanel.locator('.tree-container')).toBeVisible();
	});

	test('cancel edit restores read-only view', async ({ page }) => {
		await page.goto(`/ui/collections/${EDIT_COLLECTION}`);
		await page.waitForLoadState('networkidle');

		const detailPanel = page.locator('.two-column section.panel').nth(1);
		const entityMeta = detailPanel.locator('.entity-meta');
		await expect(entityMeta).toBeVisible({ timeout: 15000 });

		await detailPanel.getByRole('button', { name: 'Edit' }).click();
		await expect(detailPanel.getByRole('button', { name: 'Cancel' })).toBeVisible({
			timeout: 15000,
		});

		await detailPanel.getByRole('button', { name: 'Cancel' }).click();

		await expect(detailPanel.getByRole('button', { name: 'Edit' })).toBeVisible({
			timeout: 15000,
		});
		await expect(detailPanel.getByRole('button', { name: 'Save' })).not.toBeVisible();
	});
});

test.describe('Entity pagination', () => {
	test('pagination buttons exist and previous is disabled initially', async ({ page }) => {
		await page.goto(`/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		const entitiesPanel = page.locator('section.panel').filter({
			has: page.locator('h2', { hasText: /^Entities$/ }),
		});
		await expect(entitiesPanel).toBeVisible({ timeout: 15000 });

		const previousButton = entitiesPanel.getByRole('button', { name: 'Previous' });
		await expect(previousButton).toBeVisible({ timeout: 15000 });
		await expect(previousButton).toBeDisabled();

		const nextButton = entitiesPanel.getByRole('button', { name: 'Next' });
		await expect(nextButton).toBeVisible({ timeout: 15000 });
	});
});
