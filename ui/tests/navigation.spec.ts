import { expect, test } from '@playwright/test';

test.describe('Navigation', () => {
	test('root redirects to /collections', async ({ page }) => {
		await page.goto('/');
		await expect(page).toHaveURL(/\/collections$/);
	});

	test('sidebar contains nav links', async ({ page }) => {
		await page.goto('/collections');

		const sidebar = page.locator('aside.sidebar');
		await expect(sidebar.getByRole('link', { name: 'Collections' })).toBeVisible();
		await expect(sidebar.getByRole('link', { name: 'Schemas' })).toBeVisible();
		await expect(sidebar.getByRole('link', { name: 'Audit Log' })).toBeVisible();
	});

	test('brand link navigates home', async ({ page }) => {
		await page.goto('/collections');
		await page.getByRole('link', { name: 'Axon Admin' }).click();
		await expect(page).toHaveURL(/\/collections$/);
	});

	test('navigate to schemas page', async ({ page }) => {
		await page.goto('/collections');
		await page.locator('aside.sidebar').getByRole('link', { name: 'Schemas' }).click();
		await expect(page).toHaveURL(/\/schemas$/);
		await expect(page.getByRole('heading', { name: 'Schemas' })).toBeVisible();
	});

	test('navigate to audit page', async ({ page }) => {
		await page.goto('/collections');
		await page.locator('aside.sidebar').getByRole('link', { name: 'Audit Log' }).click();
		await expect(page).toHaveURL(/\/audit$/);
		await expect(page.getByRole('heading', { name: 'Audit Log' })).toBeVisible();
	});

	test('health panel is visible in sidebar', async ({ page }) => {
		await page.goto('/collections');
		const healthSection = page.locator('section.health');
		await expect(healthSection.getByRole('heading', { name: 'Health' })).toBeVisible();
	});
});
