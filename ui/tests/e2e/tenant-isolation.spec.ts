/**
 * Multi-tenant isolation — end-to-end Playwright spec.
 *
 * Proves that the ADR-018 tenant hierarchy isolates data through the UI:
 *   1. Creates tenant-A with database-A and collection coll-A
 *   2. Creates tenant-B with database-B and collection coll-B
 *   3. Navigates to tenant-A/database-A/collections → sees coll-A, not coll-B
 *   4. Navigates to tenant-B/database-B/collections → sees coll-B, not coll-A
 *   5. Uses breadcrumb to navigate back to tenant list
 *   6. Verifies both tenants appear in the list
 */
import { expect, test } from '@playwright/test';
import {
	createTestCollection,
	createTestDatabase,
	createTestTenant,
	dbCollectionsUrl,
	dbGraphqlUrl,
	type TestDatabase,
	type TestTenant,
} from './helpers';

test.describe('Multi-tenant isolation', () => {
	let tenantA: TestTenant;
	let tenantB: TestTenant;
	let dbA: TestDatabase;
	let dbB: TestDatabase;

	test.beforeAll(async ({ request }) => {
		// Create two tenants with unique names.
		tenantA = await createTestTenant(request, 'iso-a');
		tenantB = await createTestTenant(request, 'iso-b');

		// Each tenant gets its own database.
		dbA = await createTestDatabase(request, tenantA, 'db-a');
		dbB = await createTestDatabase(request, tenantB, 'db-b');

		// Each database gets its own collection.
		await createTestCollection(request, dbA, 'coll-a');
		await createTestCollection(request, dbB, 'coll-b');
	});

	test('tenant-A collections page shows coll-A only', async ({ page }) => {
		await page.goto(dbCollectionsUrl(dbA));

		// Should see coll-A header.
		await expect(
			page.getByRole('heading', { name: 'Collections', level: 1 }),
		).toBeVisible();

		// The table should contain coll-A.
		await expect(page.getByRole('link', { name: 'coll-a' })).toBeVisible({
			timeout: 5_000,
		});

		// coll-B must NOT appear on this page.
		await expect(page.getByRole('link', { name: 'coll-b' })).toHaveCount(0);
	});

	test('tenant-B collections page shows coll-B only', async ({ page }) => {
		await page.goto(dbCollectionsUrl(dbB));

		// Should see coll-B header.
		await expect(
			page.getByRole('heading', { name: 'Collections', level: 1 }),
		).toBeVisible();

		// The table should contain coll-B.
		await expect(page.getByRole('link', { name: 'coll-b' })).toBeVisible({
			timeout: 5_000,
		});

		// coll-A must NOT appear on this page.
		await expect(page.getByRole('link', { name: 'coll-a' })).toHaveCount(0);
	});

	test('breadcrumb navigates back to tenant list', async ({ page }) => {
		// Start on tenant-A's collections page.
		await page.goto(dbCollectionsUrl(dbA));
		await expect(page.getByRole('heading', { name: 'Collections', level: 1 })).toBeVisible();

		// Click the first breadcrumb "Tenants" link to go back to the tenant list.
		const breadcrumbs = page.locator('.crumbs');
		await breadcrumbs.getByRole('link', { name: 'Tenants' }).click();

		// Should land on the tenants list page.
		await expect(page).toHaveURL(/\/ui\/tenants$/);
		await expect(
			page.getByRole('heading', { name: 'Tenants', level: 1 }),
		).toBeVisible();
	});

	test('both tenants appear in the tenant list', async ({ page }) => {
		await page.goto('/ui/tenants');

		// Both tenant names should appear in the list.
		await expect(page.getByRole('heading', { name: tenantA.name })).toBeVisible({
			timeout: 5_000,
		});
		await expect(page.getByRole('heading', { name: tenantB.name })).toBeVisible({
			timeout: 5_000,
		});
	});

	test('tenant-A database overview page', async ({ page }) => {
		await page.goto(dbGraphqlUrl(dbA));

		// Should see the database heading.
		await expect(page.getByRole('heading', { name: dbA.name, level: 1 })).toBeVisible();

		// Should see the tenant name in the description.
		await expect(page.getByText(tenantA.name)).toBeVisible();

		// Breadcrumb chain: Tenants / <tenantA.name> / <dbA.name>.
		const breadcrumbs = page.locator('.crumbs');
		await expect(breadcrumbs.getByRole('link', { name: 'Tenants' })).toBeVisible();
		await expect(
			breadcrumbs.getByRole('link', { name: tenantA.name }),
		).toBeVisible();
		await expect(
			breadcrumbs.getByRole('link', { name: dbA.name }),
		).toHaveCount(0); // db name is current, not a link.
	});

	test('tenant-B database overview page', async ({ page }) => {
		await page.goto(dbGraphqlUrl(dbB));

		// Should see the database heading.
		await expect(page.getByRole('heading', { name: dbB.name, level: 1 })).toBeVisible();

		// Should see the tenant name in the description.
		await expect(page.getByText(tenantB.name)).toBeVisible();
	});
});
