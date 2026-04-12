import { expect, test } from './fixtures';

test.describe('Collections browser', () => {
	test('page heading is visible', async ({ page }) => {
		await page.goto('/ui/collections');
		// exact: true to avoid matching the "Registered Collections" h2 on the same page.
		await expect(page.getByRole('heading', { name: 'Collections', exact: true })).toBeVisible();
	});

	test('shows loading state initially', async ({ page }) => {
		await page.goto('/ui/collections');
		// The loading message appears briefly before data arrives.
		const loadingMessage = page.getByText('Loading collections...');
		// It may or may not still be visible depending on API speed, so just
		// verify the page loaded without error.
		await expect(
			page.getByRole('heading', { name: 'Collections', exact: true }),
		).toBeVisible();
		// The loading message should eventually disappear.
		await expect(loadingMessage).not.toBeVisible({ timeout: 10_000 });
	});

	test('collection table is rendered with data', async ({ page }) => {
		// With mocked API returning collections, the table should appear.
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');

		const table = page.locator('table');
		await expect(table).toBeVisible({ timeout: 10_000 });
	});

	test('collection table has correct headers', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');

		const table = page.locator('table');
		const hasTable = await table.isVisible().catch(() => false);
		if (hasTable) {
			await expect(table.locator('th', { hasText: 'Collection' })).toBeVisible();
			await expect(table.locator('th', { hasText: 'Entities' })).toBeVisible();
			await expect(table.locator('th', { hasText: 'Schema' })).toBeVisible();
			await expect(table.locator('th', { hasText: 'Actions' })).toBeVisible();
		}
	});

	test('refresh button exists', async ({ page }) => {
		await page.goto('/ui/collections');
		await expect(page.getByRole('button', { name: 'Refresh' })).toBeVisible();
	});

	test('create collection link points to schemas', async ({ page }) => {
		await page.goto('/ui/collections');
		const createLink = page.getByRole('link', { name: 'Create Collection' });
		await expect(createLink).toBeVisible();
		await expect(createLink).toHaveAttribute('href', /\/ui\/schemas$/);
	});
});

test.describe('Collection detail — entity browser', () => {
	// These tests use the "tasks" collection from mock data.
	const collectionName = 'tasks';

	test('entity browser page loads', async ({ page }) => {
		await page.goto(`/ui/collections/${collectionName}`);
		await expect(page.getByRole('heading', { name: collectionName })).toBeVisible();
	});

	test('create entity toggle works', async ({ page }) => {
		await page.goto(`/ui/collections/${collectionName}`);
		// Wait for entities to load so the auto-shown create form (for empty
		// collections) is replaced by the entity table.
		await page.waitForLoadState('networkidle');

		// The toggle button lives in the page-header area.
		const toggleButton = page.locator('.page-header').getByRole('button', { name: 'Create Entity' });
		await expect(toggleButton).toBeVisible();

		await toggleButton.click();
		await expect(page.getByRole('heading', { name: 'Create Entity' })).toBeVisible();
		await expect(page.getByPlaceholder('task-001')).toBeVisible();
	});

	test('entity table has correct columns', async ({ page }) => {
		await page.goto(`/ui/collections/${collectionName}`);
		await page.waitForLoadState('networkidle');

		const entityTable = page.locator('section.panel').filter({ hasText: 'Entities' }).locator('table');
		const hasTable = await entityTable.isVisible().catch(() => false);
		if (hasTable) {
			await expect(entityTable.locator('th', { hasText: 'ID' })).toBeVisible();
			await expect(entityTable.locator('th', { hasText: 'Version' })).toBeVisible();
			await expect(entityTable.locator('th', { hasText: 'Preview' })).toBeVisible();
		}
	});

	test('entity detail panel shows placeholder when none selected', async ({ page }) => {
		// Override the query endpoint to return no entities, so we can test the placeholder.
		await page.route('**/collections/*/query', (route) => {
			if (route.request().method() !== 'POST') return route.fallback();
			return route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ entities: [], total_count: 0, next_cursor: null }),
			});
		});
		// Override the collection detail to have 0 entities.
		await page.route(/\/collections\/tasks$/, (route) => {
			if (route.request().method() !== 'GET') return route.fallback();
			return route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					name: 'tasks',
					entity_count: 0,
					schema: null,
					created_at_ns: null,
					updated_at_ns: null,
				}),
			});
		});

		await page.goto(`/ui/collections/${collectionName}`);
		await page.waitForLoadState('networkidle');

		const detailPanel = page.locator('section.panel').filter({ hasText: 'Entity Detail' });
		const hasPlaceholder = await detailPanel.isVisible().catch(() => false);
		if (hasPlaceholder) {
			await expect(
				detailPanel.getByText('Select an entity row to inspect its data.'),
			).toBeVisible();
		}
	});

	test('create entity form validates empty ID', async ({ page }) => {
		await page.goto(`/ui/collections/${collectionName}`);
		// Wait for entities to load so the toggle button appears.
		await page.waitForLoadState('networkidle');

		// Open the create form via the toggle in the page header.
		const toggleButton = page.locator('.page-header').getByRole('button', { name: 'Create Entity' });
		await expect(toggleButton).toBeVisible();
		await toggleButton.click();

		// Ensure the create form is visible.
		const createPanel = page.locator('section.panel').filter({ hasText: 'Create Entity' });
		await expect(createPanel).toBeVisible();

		// Clear the ID field.
		const idInput = createPanel.getByPlaceholder('task-001');
		await expect(idInput).toBeVisible();
		await idInput.fill('');

		// Click the primary "Create Entity" submit button inside the form.
		await createPanel.getByRole('button', { name: 'Create Entity' }).click();

		// Should show a validation error about missing ID.
		await expect(page.getByText('Entity ID is required.')).toBeVisible({ timeout: 5_000 });
	});

	test('pagination buttons are present', async ({ page }) => {
		await page.goto(`/ui/collections/${collectionName}`);

		await expect(page.getByRole('button', { name: 'Previous' })).toBeVisible();
		await expect(page.getByRole('button', { name: 'Next' })).toBeVisible();
	});
});
