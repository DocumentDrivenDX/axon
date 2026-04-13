import { expect, test } from '@playwright/test';

/**
 * E2E tests for the health panel and basic navigation against a real axon-server.
 *
 * Uses relative URLs so the baseURL from the active Playwright config determines
 * the port and host (memory, sqlite, or postgres config can all use the same tests).
 */

test.describe('Health panel and navigation', () => {
	test('root redirects to /ui/collections', async ({ page }) => {
		await page.goto('/ui/');
		// The root page uses SvelteKit goto() on mount — wait for the navigation.
		await page.waitForURL(/\/ui\/collections/, { timeout: 10000 });
	});

	test('health panel shows ok status', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');

		// The health pill in the sidebar shows the status value from /health.
		// The server returns status: "ok" (not "healthy").
		const healthSection = page.locator('section.health');
		await expect(healthSection).toBeVisible({ timeout: 15000 });
		await expect(healthSection.locator('.pill')).toContainText('ok', { timeout: 15000 });
	});

	test('health panel shows backend type', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');

		// The layout renders: <strong>Backend</strong><p class="muted">{backend}</p>
		// The specific value depends on the active config (memory / sqlite / postgres).
		const healthSection = page.locator('section.health');
		await expect(healthSection).toBeVisible({ timeout: 15000 });
		await expect(healthSection).toContainText('Backend', { timeout: 15000 });
		// The muted paragraph under "Backend" must contain some non-empty text.
		const backendValue = healthSection.locator('p.muted').first();
		await expect(backendValue).toBeVisible({ timeout: 15000 });
		await expect(backendValue).not.toBeEmpty({ timeout: 15000 });
	});

	test('sidebar has Collections link', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');

		const nav = page.locator('nav');
		await expect(nav.getByRole('link', { name: 'Collections' })).toBeVisible();
	});

	test('sidebar has Schemas link', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');

		const nav = page.locator('nav');
		await expect(nav.getByRole('link', { name: 'Schemas' })).toBeVisible();
	});

	test('sidebar has Audit Log link', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');

		const nav = page.locator('nav');
		await expect(nav.getByRole('link', { name: 'Audit Log' })).toBeVisible();
	});
});
