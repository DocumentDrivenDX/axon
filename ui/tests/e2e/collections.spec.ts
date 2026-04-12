import { expect, test } from '@playwright/test';

/**
 * E2E tests for collection creation and browsing against a real axon-server.
 *
 * Uses collection name "e2e-tasks" to avoid interference with other test files.
 * Memory storage resets between server restarts but NOT between tests in a run,
 * so the collection created in step 1 is visible in later tests.
 */

const COLLECTION_NAME = 'e2e-tasks';

test.describe('Collections workflow', () => {
	test('create collection via schemas page', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/schemas');
		await page.waitForLoadState('networkidle');

		// Fill in the collection name in the Create Collection form.
		const nameInput = page.locator('input[placeholder="tasks"]');
		await expect(nameInput).toBeVisible({ timeout: 15000 });
		await nameInput.fill(COLLECTION_NAME);

		// Click the primary Create Collection button.
		const createButton = page
			.locator('section.panel')
			.filter({ hasText: 'Create Collection' })
			.getByRole('button', { name: 'Create Collection' });
		await expect(createButton).toBeVisible();
		await createButton.click();

		// Verify success: the status message appears.
		await expect(page.getByText('Collection created.')).toBeVisible({ timeout: 15000 });
	});

	test('new collection appears in schemas collection list', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/schemas');
		await page.waitForLoadState('networkidle');

		// The left-hand panel lists registered collections.
		const collectionsPanel = page
			.locator('section.panel')
			.filter({ hasText: 'Collections' })
			.first();
		await expect(collectionsPanel).toBeVisible({ timeout: 15000 });
		await expect(collectionsPanel.getByText(COLLECTION_NAME)).toBeVisible({ timeout: 15000 });
	});

	test('new collection appears in collections table', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		// The collections table should show the collection we created.
		const table = page.locator('table');
		await expect(table).toBeVisible({ timeout: 15000 });
		await expect(table.getByRole('link', { name: COLLECTION_NAME })).toBeVisible({
			timeout: 15000,
		});
	});

	test('collection detail page shows 0 entities', async ({ page }) => {
		await page.goto(`http://localhost:4170/ui/collections/${COLLECTION_NAME}`);
		await page.waitForLoadState('networkidle');

		// Page heading matches the collection name.
		await expect(page.getByRole('heading', { name: COLLECTION_NAME })).toBeVisible({
			timeout: 15000,
		});

		// The entity count muted text: "0 entities · no schema"
		await expect(page.getByText('0 entities')).toBeVisible({ timeout: 15000 });
	});
});
