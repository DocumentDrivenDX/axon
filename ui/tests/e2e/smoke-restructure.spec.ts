import { expect, test } from '@playwright/test';
import { createTestTenant, tenantUrl } from './helpers';

/**
 * End-to-end smoke test for the post-restructure admin UI.
 *
 * Walks the golden path that a user cares about:
 *   /ui/            → redirects to /ui/tenants
 *   /ui/tenants     → list, create, open a tenant
 *   /ui/tenants/<t> → tenant page, create a database, open it
 *   /ui/tenants/<t>/databases/<d>{,/collections,/schemas,/audit}
 *   /ui/users       → global ACL page
 *
 * Uses a unique per-run tenant name so it can run repeatedly against
 * the same axon instance without collisions.
 */
test.describe('UI restructure smoke', () => {
	const unique = Date.now().toString(36);
	const tenantName = `smoke${unique}`;
	const dbName = 'default';

	test('root redirects to /ui/tenants', async ({ page }) => {
		await page.goto('/ui/');
		await expect(page).toHaveURL(/\/ui\/tenants$/);
		await expect(page.getByRole('heading', { name: 'Tenants', level: 1 })).toBeVisible();
	});

	test('top nav shows Tenants and Users only (no Collections/Schemas/Audit)', async ({
		page,
	}) => {
		await page.goto('/ui/tenants');
		const topnav = page.locator('header.topnav nav.topnav-links');
		await expect(topnav.getByRole('link', { name: 'Tenants' })).toBeVisible();
		await expect(topnav.getByRole('link', { name: 'Users' })).toBeVisible();
		await expect(topnav.getByRole('link', { name: 'Collections' })).toHaveCount(0);
		await expect(topnav.getByRole('link', { name: 'Schemas' })).toHaveCount(0);
		await expect(topnav.getByRole('link', { name: 'Audit Log' })).toHaveCount(0);
	});

	test('create tenant, open it, create database, walk sub-nav', async ({ page }) => {
		// Create the tenant.
		await page.goto('/ui/tenants');
		await page.getByPlaceholder('Tenant name').fill(tenantName);
		await page.getByRole('button', { name: 'Create', exact: true }).click();

		// After create, we navigate to /ui/tenants/<db_name>.
		await expect(page).toHaveURL(new RegExp(`/ui/tenants/${tenantName}-[0-9a-f]+$`));

		// Tenant layout sub-nav (Databases / Members / Credentials).
		const tenantSubnav = page.locator('.tenant-header .subnav');
		await expect(tenantSubnav.getByRole('link', { name: 'Databases' })).toBeVisible();
		await expect(tenantSubnav.getByRole('link', { name: 'Members' })).toBeVisible();
		await expect(tenantSubnav.getByRole('link', { name: 'Credentials' })).toBeVisible();

		// No databases yet — the create form should be visible.
		await expect(page.getByRole('heading', { name: 'Create Database' })).toBeVisible();

		await page.getByPlaceholder(/Database name/).fill(dbName);
		await page.getByRole('button', { name: 'Create', exact: true }).click();

		// The newly-created database should now appear as a clickable row.
		await expect(page.getByRole('link', { name: 'Open' }).first()).toBeVisible();
		await page.getByRole('link', { name: 'Open' }).first().click();

		// Database overview page.
		await expect(page).toHaveURL(
			new RegExp(`/ui/tenants/${tenantName}-[0-9a-f]+/databases/${dbName}$`),
		);
		await expect(page.getByRole('heading', { name: dbName, level: 1 })).toBeVisible();

		// Database sub-nav (Collections / Schemas / Audit Log).
		const dbSubnav = page.locator('.db-header .subnav');
		await expect(dbSubnav.getByRole('link', { name: 'Collections' })).toBeVisible();
		await expect(dbSubnav.getByRole('link', { name: 'Schemas' })).toBeVisible();
		await expect(dbSubnav.getByRole('link', { name: 'Audit Log' })).toBeVisible();

		// Collections page.
		await dbSubnav.getByRole('link', { name: 'Collections' }).click();
		await expect(page).toHaveURL(/\/collections$/);
		await expect(page.getByRole('heading', { name: 'Collections', level: 1 })).toBeVisible();

		// Schemas page.
		await dbSubnav.getByRole('link', { name: 'Schemas' }).click();
		await expect(page).toHaveURL(/\/schemas$/);
		await expect(page.getByRole('heading', { name: 'Schemas', level: 1 })).toBeVisible();

		// Audit Log page.
		await dbSubnav.getByRole('link', { name: 'Audit Log' }).click();
		await expect(page).toHaveURL(/\/audit$/);
		await expect(page.getByRole('heading', { name: 'Audit Log', level: 1 })).toBeVisible();
	});

	test('tenant Members page loads', async ({ page, request }) => {
		const tenant = await createTestTenant(request, 'smoke-members');
		await page.goto(tenantUrl(tenant));
		await page.locator('.tenant-header .subnav').getByRole('link', { name: 'Members' }).click();
		await expect(page).toHaveURL(/\/members$/);
		await expect(page.getByRole('heading', { name: 'Members', level: 1 })).toBeVisible();
	});

	test('tenant Credentials page loads', async ({ page, request }) => {
		const tenant = await createTestTenant(request, 'smoke-creds');
		await page.goto(tenantUrl(tenant));
		await page
			.locator('.tenant-header .subnav')
			.getByRole('link', { name: 'Credentials' })
			.click();
		await expect(page).toHaveURL(/\/credentials$/);
		await expect(page.getByRole('heading', { name: 'Credentials', level: 1 })).toBeVisible();
	});

	test('Users page loads from top nav', async ({ page }) => {
		await page.goto('/ui/tenants');
		await page.locator('header.topnav').getByRole('link', { name: 'Users' }).click();
		await expect(page).toHaveURL(/\/ui\/users$/);
		await expect(page.getByRole('heading', { name: 'Users', level: 1 })).toBeVisible();
	});

	test('unknown tenant shows 404', async ({ page }) => {
		const resp = await page.goto('/ui/tenants/this-tenant-does-not-exist');
		// SvelteKit SPA fallback serves index.html with 200, but the page
		// content should render the error() call's 404 state.
		expect(resp?.status()).toBeLessThan(500);
		await expect(page.getByText(/not found/i).first()).toBeVisible();
	});
});
