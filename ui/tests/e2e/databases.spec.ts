import { expect, test } from '@playwright/test';

test.describe('Databases page', () => {
	// Tests in this describe share server-side state (tenant + database assignments),
	// so they must run sequentially in declaration order.
	test.describe.configure({ mode: 'serial' });

	test('sidebar has Databases link', async ({ page }) => {
		await page.goto('/ui/collections');
		await page.waitForLoadState('networkidle');
		const nav = page.locator('nav');
		await expect(nav.getByRole('link', { name: 'Databases' })).toBeVisible();
	});

	test('databases page loads with heading', async ({ page }) => {
		await page.goto('/ui/databases');
		await page.waitForLoadState('networkidle');
		await expect(page.getByRole('heading', { name: 'Databases' })).toBeVisible({ timeout: 15000 });
	});

	test('create tenant form is visible', async ({ page }) => {
		await page.goto('/ui/databases');
		await page.waitForLoadState('networkidle');
		await expect(page.getByRole('heading', { name: 'Create Tenant' })).toBeVisible();
		await expect(page.getByPlaceholder('my-org')).toBeVisible();
		const createBtn = page.getByRole('button', { name: 'Create Tenant' });
		await expect(createBtn).toBeDisabled(); // disabled when input empty
	});

	test('create tenant button enables when name entered', async ({ page }) => {
		await page.goto('/ui/databases');
		await page.waitForLoadState('networkidle');
		await page.getByPlaceholder('my-org').fill('e2e-tenant-test');
		await expect(page.getByRole('button', { name: 'Create Tenant' })).toBeEnabled();
	});

	test('can create a tenant and see it in the list', async ({ page }) => {
		await page.goto('/ui/databases');
		await page.waitForLoadState('networkidle');
		await page.getByPlaceholder('my-org').fill('e2e-org');
		await page.getByRole('button', { name: 'Create Tenant' }).click();
		// Wait for success: the new tenant heading appears
		await expect(page.getByRole('heading', { name: 'e2e-org' })).toBeVisible({ timeout: 15000 });
	});

	test('can assign a database to a tenant', async ({ page }) => {
		await page.goto('/ui/databases');
		await page.waitForLoadState('networkidle');
		// The e2e-org tenant should exist from the previous test (same server instance)
		const tenantPanel = page.locator('section.panel').filter({ hasText: 'e2e-org' });
		await expect(tenantPanel).toBeVisible({ timeout: 15000 });
		// Fill the database name input in that panel
		await tenantPanel.getByPlaceholder('database-name').fill('e2e-db');
		await tenantPanel.getByRole('button', { name: 'Assign Database' }).click();
		// Verify the database appears in the table
		await expect(tenantPanel.getByRole('cell', { name: 'e2e-db' })).toBeVisible({ timeout: 15000 });
	});

	test('can remove a database from a tenant', async ({ page }) => {
		await page.goto('/ui/databases');
		await page.waitForLoadState('networkidle');
		const tenantPanel = page.locator('section.panel').filter({ hasText: 'e2e-org' });
		await expect(tenantPanel).toBeVisible({ timeout: 15000 });
		// Click Remove button for e2e-db
		const dbRow = tenantPanel.locator('table tbody tr').filter({ hasText: 'e2e-db' });
		await expect(dbRow).toBeVisible({ timeout: 15000 });
		await dbRow.getByRole('button', { name: 'Remove' }).click();
		// Confirm
		await dbRow.getByRole('button', { name: 'Confirm' }).click();
		// Verify the database is gone
		await expect(tenantPanel.getByRole('cell', { name: 'e2e-db' })).not.toBeVisible({
			timeout: 15000,
		});
	});
});
