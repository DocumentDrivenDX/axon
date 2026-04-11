import { expect, test } from './fixtures';

test.describe('Navigation', () => {
	test('root redirects to /ui/collections', async ({ page }) => {
		await page.goto('/ui/');
		await expect(page).toHaveURL(/\/ui\/collections$/);
	});

	test('sidebar contains nav links', async ({ page }) => {
		await page.goto('/ui/collections');

		const sidebar = page.locator('aside.sidebar');
		await expect(sidebar.getByRole('link', { name: 'Collections' })).toBeVisible();
		await expect(sidebar.getByRole('link', { name: 'Schemas' })).toBeVisible();
		await expect(sidebar.getByRole('link', { name: 'Audit Log' })).toBeVisible();
	});

	test('brand link navigates home', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.getByRole('link', { name: 'Axon Admin' }).click();
		await expect(page).toHaveURL(/\/ui\/collections$/);
	});

	test('navigate to schemas page', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.locator('aside.sidebar').getByRole('link', { name: 'Schemas' }).click();
		await expect(page).toHaveURL(/\/ui\/schemas$/);
		await expect(page.getByRole('heading', { name: 'Schemas' })).toBeVisible();
	});

	test('navigate to audit page', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.locator('aside.sidebar').getByRole('link', { name: 'Audit Log' }).click();
		await expect(page).toHaveURL(/\/ui\/audit$/);
		await expect(page.getByRole('heading', { name: 'Audit Log' })).toBeVisible();
	});

	test('health panel is visible in sidebar', async ({ page }) => {
		await page.goto('/ui/collections');
		const healthSection = page.locator('section.health');
		await expect(healthSection.getByRole('heading', { name: 'Health' })).toBeVisible();
	});
});
