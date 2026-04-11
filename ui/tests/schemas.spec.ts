import { expect, test } from '@playwright/test';

test.describe('Schemas page', () => {
	test('page heading is visible', async ({ page }) => {
		await page.goto('/schemas');
		await expect(page.getByRole('heading', { name: 'Schemas' })).toBeVisible();
		await expect(
			page.getByText('View and update collection schemas through the live HTTP endpoints.'),
		).toBeVisible();
	});

	test('collections panel is present', async ({ page }) => {
		await page.goto('/schemas');
		await expect(
			page.locator('section.panel').filter({ hasText: 'Collections' }),
		).toBeVisible();
	});

	test('create collection form is visible', async ({ page }) => {
		await page.goto('/schemas');

		await expect(page.getByRole('heading', { name: 'Create Collection' })).toBeVisible();
		await expect(page.getByPlaceholder('tasks')).toBeVisible();
		await expect(
			page.getByRole('button', { name: 'Create Collection' }),
		).toBeVisible();
	});

	test('create collection button is disabled when name is empty', async ({ page }) => {
		await page.goto('/schemas');

		const nameInput = page.getByPlaceholder('tasks');
		await nameInput.fill('');

		const createButton = page.getByRole('button', { name: 'Create Collection' });
		await expect(createButton).toBeDisabled();
	});

	test('create collection button enables when name is entered', async ({ page }) => {
		await page.goto('/schemas');

		const nameInput = page.getByPlaceholder('tasks');
		await nameInput.fill('test-collection');

		const createButton = page.getByRole('button', { name: 'Create Collection' });
		await expect(createButton).toBeEnabled();
	});

	test('schema detail shows placeholder when no collection selected', async ({ page }) => {
		await page.goto('/schemas');

		await expect(
			page.getByText('Select a collection to inspect its schema.'),
		).toBeVisible();
	});
});

test.describe('Schema detail view', () => {
	// These tests assume at least one collection exists so the schema panel
	// can display structured field information.

	test('selecting a collection shows schema detail', async ({ page }) => {
		await page.goto('/schemas');
		await page.waitForLoadState('networkidle');

		// Click the first collection button if any exist.
		const collectionButtons = page
			.locator('section.panel')
			.filter({ hasText: 'Collections' })
			.locator('.panel-body button');

		const count = await collectionButtons.count();
		if (count > 0) {
			await collectionButtons.first().click();

			// Schema detail should now show version info.
			await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 5_000 });
		}
	});

	test('entity fields table has correct headers', async ({ page }) => {
		await page.goto('/schemas');
		await page.waitForLoadState('networkidle');

		const collectionButtons = page
			.locator('section.panel')
			.filter({ hasText: 'Collections' })
			.locator('.panel-body button');

		const count = await collectionButtons.count();
		if (count > 0) {
			await collectionButtons.first().click();
			await page.waitForLoadState('networkidle');

			const fieldsSection = page.locator('.schema-section').filter({ hasText: 'Entity Fields' });
			const hasFields = await fieldsSection.isVisible().catch(() => false);
			if (hasFields) {
				const table = fieldsSection.locator('table');
				await expect(table.locator('th', { hasText: 'Field' })).toBeVisible();
				await expect(table.locator('th', { hasText: 'Type' })).toBeVisible();
				await expect(table.locator('th', { hasText: 'Required' })).toBeVisible();
				await expect(table.locator('th', { hasText: 'Constraints' })).toBeVisible();
			}
		}
	});

	test('view mode toggle between structured and raw', async ({ page }) => {
		await page.goto('/schemas');
		await page.waitForLoadState('networkidle');

		const collectionButtons = page
			.locator('section.panel')
			.filter({ hasText: 'Collections' })
			.locator('.panel-body button');

		const count = await collectionButtons.count();
		if (count > 0) {
			await collectionButtons.first().click();
			await page.waitForLoadState('networkidle');

			const rawButton = page.getByRole('button', { name: 'Raw JSON' });
			const hasRawButton = await rawButton.isVisible().catch(() => false);
			if (hasRawButton) {
				await rawButton.click();
				await expect(page.locator('pre')).toBeVisible();

				await page.getByRole('button', { name: 'Structured' }).click();
			}
		}
	});

	test('edit mode opens textarea', async ({ page }) => {
		await page.goto('/schemas');
		await page.waitForLoadState('networkidle');

		const collectionButtons = page
			.locator('section.panel')
			.filter({ hasText: 'Collections' })
			.locator('.panel-body button');

		const count = await collectionButtons.count();
		if (count > 0) {
			await collectionButtons.first().click();
			await page.waitForLoadState('networkidle');

			const editButton = page.getByRole('button', { name: 'Edit' });
			const hasEdit = await editButton.isVisible().catch(() => false);
			if (hasEdit) {
				await editButton.click();
				await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
				await expect(page.getByRole('button', { name: 'Save Schema' })).toBeVisible();
			}
		}
	});
});
