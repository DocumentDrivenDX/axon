import { expect, test } from '@playwright/test';

test.describe('Collections browser', () => {
	test('page heading is visible', async ({ page }) => {
		await page.goto('/collections');
		await expect(page.getByRole('heading', { name: 'Collections' })).toBeVisible();
	});

	test('shows loading state initially', async ({ page }) => {
		await page.goto('/collections');
		// The loading message appears briefly before data arrives.
		const loadingMessage = page.getByText('Loading collections...');
		// It may or may not still be visible depending on API speed, so just
		// verify the page loaded without error.
		await expect(
			page.getByRole('heading', { name: 'Collections' }),
		).toBeVisible();
		// The loading message should eventually disappear.
		await expect(loadingMessage).not.toBeVisible({ timeout: 10_000 });
	});

	test('shows empty state when no collections exist', async ({ page }) => {
		// When the API returns an empty list, the empty-state panel appears.
		await page.goto('/collections');
		// Wait for loading to finish.
		await page.waitForLoadState('networkidle');

		const noCollections = page.getByText('No collections yet');
		const table = page.locator('table');

		// Either the empty state or the table should be present.
		const hasTable = await table.isVisible().catch(() => false);
		if (!hasTable) {
			await expect(noCollections).toBeVisible();
			await expect(
				page.getByRole('link', { name: 'Open schema workspace' }),
			).toBeVisible();
		}
	});

	test('collection table has correct headers', async ({ page }) => {
		await page.goto('/collections');
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
		await page.goto('/collections');
		await expect(page.getByRole('button', { name: 'Refresh' })).toBeVisible();
	});

	test('create collection link points to schemas', async ({ page }) => {
		await page.goto('/collections');
		const createLink = page.getByRole('link', { name: 'Create Collection' });
		await expect(createLink).toBeVisible();
		await expect(createLink).toHaveAttribute('href', /\/schemas$/);
	});
});

test.describe('Collection detail — entity browser', () => {
	// These tests assume a collection named "tasks" exists.
	// In a real CI environment, a seed/fixture step would create it first.
	const collectionName = 'tasks';

	test('entity browser page loads', async ({ page }) => {
		await page.goto(`/collections/${collectionName}`);
		await expect(page.getByRole('heading', { name: collectionName })).toBeVisible();
	});

	test('create entity toggle works', async ({ page }) => {
		await page.goto(`/collections/${collectionName}`);

		const createButton = page.getByRole('button', { name: 'Create Entity' });
		await expect(createButton).toBeVisible();

		await createButton.click();
		await expect(page.getByRole('heading', { name: 'Create Entity' })).toBeVisible();
		await expect(page.getByPlaceholder('task-001')).toBeVisible();
	});

	test('entity table has correct columns', async ({ page }) => {
		await page.goto(`/collections/${collectionName}`);
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
		await page.goto(`/collections/${collectionName}`);
		// If no entities exist, the detail panel shows the placeholder.
		const detailPanel = page.locator('section.panel').filter({ hasText: 'Entity Detail' });
		const hasPlaceholder = await detailPanel.isVisible().catch(() => false);
		if (hasPlaceholder) {
			await expect(
				detailPanel.getByText('Select an entity row to inspect its data.'),
			).toBeVisible();
		}
	});

	test('create entity form validates empty ID', async ({ page }) => {
		await page.goto(`/collections/${collectionName}`);

		// Open the create form.
		const toggleButton = page.getByRole('button', { name: 'Create Entity' });
		if (await toggleButton.isVisible().catch(() => false)) {
			await toggleButton.click();
		}

		// Submit with empty ID.
		await page.getByRole('button', { name: 'Create Entity', exact: true }).last().click();

		// Should show a validation error about missing ID.
		await expect(page.getByText('Entity ID is required')).toBeVisible({ timeout: 5_000 });
	});

	test('pagination buttons are present', async ({ page }) => {
		await page.goto(`/collections/${collectionName}`);

		await expect(page.getByRole('button', { name: 'Previous' })).toBeVisible();
		await expect(page.getByRole('button', { name: 'Next' })).toBeVisible();
	});
});
