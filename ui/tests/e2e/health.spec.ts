import { expect, test } from '@playwright/test';

/**
 * E2E tests for the health panel and basic navigation against a real axon-server.
 *
 * The server runs with --storage memory so the backing_store backend will be
 * reported as "memory" (not "sqlite"). Adjust the assertion below if the server
 * health response changes.
 */

test.describe('Health panel and navigation', () => {
	test('root redirects to /ui/collections', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/');
		// The root page uses SvelteKit goto() on mount — wait for the navigation.
		await page.waitForURL(/\/ui\/collections/, { timeout: 10000 });
	});

	test('health panel shows ok status', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		// The health pill in the sidebar shows the status value from /health.
		// The server returns status: "ok" (not "healthy").
		const healthSection = page.locator('section.health');
		await expect(healthSection).toBeVisible({ timeout: 15000 });
		await expect(healthSection.locator('.pill')).toContainText('ok', { timeout: 15000 });
	});

	test('health panel shows backend type', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		// The layout renders: <strong>Backend</strong><p class="muted">{backend}</p>
		const healthSection = page.locator('section.health');
		await expect(healthSection).toBeVisible({ timeout: 15000 });
		// memory storage is used in E2E tests
		await expect(healthSection.getByText('memory')).toBeVisible({ timeout: 15000 });
	});

	test('sidebar has Collections link', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		const nav = page.locator('nav');
		await expect(nav.getByRole('link', { name: 'Collections' })).toBeVisible();
	});

	test('sidebar has Schemas link', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		const nav = page.locator('nav');
		await expect(nav.getByRole('link', { name: 'Schemas' })).toBeVisible();
	});

	test('sidebar has Audit Log link', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/collections');
		await page.waitForLoadState('networkidle');

		const nav = page.locator('nav');
		await expect(nav.getByRole('link', { name: 'Audit Log' })).toBeVisible();
	});
});
