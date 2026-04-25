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
	createTestEntity,
	createTestTenant,
	dbCollectionUrl,
	dbCollectionsUrl,
	dbOverviewUrl,
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

		// Use the same database and collection names under each tenant to prove
		// the tenant path segment is part of the isolation boundary.
		dbA = await createTestDatabase(request, tenantA, 'default');
		dbB = await createTestDatabase(request, tenantB, 'default');

		await createTestCollection(request, dbA, 'orders');
		await createTestCollection(request, dbB, 'orders');
		await createTestEntity(request, dbA, 'orders', 'order-1', {
			tenant: 'A',
			title: 'Tenant A order',
		});
		await createTestEntity(request, dbB, 'orders', 'order-1', {
			tenant: 'B',
			title: 'Tenant B order',
		});
	});

	test('tenant-A collections page shows its orders collection', async ({ page }) => {
		await page.goto(dbCollectionsUrl(dbA));

		// Should see coll-A header.
		await expect(page.getByRole('heading', { name: 'Collections', level: 1 })).toBeVisible();

		await expect(page.getByRole('link', { name: 'orders' })).toBeVisible({
			timeout: 5_000,
		});
	});

	test('tenant-B collections page shows its orders collection', async ({ page }) => {
		await page.goto(dbCollectionsUrl(dbB));

		// Should see coll-B header.
		await expect(page.getByRole('heading', { name: 'Collections', level: 1 })).toBeVisible();

		await expect(page.getByRole('link', { name: 'orders' })).toBeVisible({
			timeout: 5_000,
		});
	});

	test('same entity id under same database and collection name stays tenant-isolated', async ({
		page,
	}) => {
		await page.goto(dbCollectionUrl(dbA, 'orders'));
		await expect(page.getByText('Tenant A order').first()).toBeVisible({ timeout: 10_000 });
		await expect(page.getByText('Tenant B order')).toHaveCount(0);

		await page.goto(dbCollectionUrl(dbB, 'orders'));
		await expect(page.getByText('Tenant B order').first()).toBeVisible({ timeout: 10_000 });
		await expect(page.getByText('Tenant A order')).toHaveCount(0);
	});

	test('breadcrumb navigates back to tenant list', async ({ page }) => {
		// Start on tenant-A's collections page.
		await page.goto(dbCollectionsUrl(dbA));
		await expect(page.getByRole('heading', { name: 'Collections', level: 1 })).toBeVisible();

		// Click the first breadcrumb "Tenants" link to go back to the tenant list.
		const breadcrumbs = page.locator('.db-header .crumbs');
		await breadcrumbs.getByRole('link', { name: 'Tenants' }).click();

		// Should land on the tenants list page.
		await expect(page).toHaveURL(/\/ui\/tenants$/);
		await expect(page.getByRole('heading', { name: 'Tenants', level: 1 })).toBeVisible();
	});

	test('both tenants appear in the tenant list', async ({ page }) => {
		await page.goto('/ui/tenants');

		// Both tenant names should appear in the list.
		await expect(page.getByRole('link', { name: tenantA.name })).toBeVisible({
			timeout: 5_000,
		});
		await expect(page.getByRole('link', { name: tenantB.name })).toBeVisible({
			timeout: 5_000,
		});
	});

	test('tenant-A database overview page', async ({ page }) => {
		await page.goto(dbOverviewUrl(dbA));

		// Should see the database heading.
		await expect(page.getByRole('heading', { name: dbA.name, level: 1 })).toBeVisible();

		// Should see the tenant name in the description.
		await expect(page.locator('main .page-header').last().getByText(tenantA.name)).toBeVisible();

		// Breadcrumb chain: Tenants / <tenantA.name> / <dbA.name>.
		const breadcrumbs = page.locator('.db-header .crumbs');
		await expect(breadcrumbs.getByRole('link', { name: 'Tenants' })).toBeVisible();
		await expect(breadcrumbs.getByRole('link', { name: tenantA.name })).toBeVisible();
		await expect(breadcrumbs.getByRole('link', { name: dbA.name })).toHaveCount(0); // db name is current, not a link.
	});

	test('tenant-B database overview page', async ({ page }) => {
		await page.goto(dbOverviewUrl(dbB));

		// Should see the database heading.
		await expect(page.getByRole('heading', { name: dbB.name, level: 1 })).toBeVisible();

		// Should see the tenant name in the description.
		await expect(page.locator('main .page-header').last().getByText(tenantB.name)).toBeVisible();
	});
});
