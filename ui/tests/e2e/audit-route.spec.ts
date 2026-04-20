import { expect, test } from '@playwright/test';
import {
	createTestCollection,
	createTestDatabase,
	createTestEntity,
	createTestTenant,
	dbAuditUrl,
	dbCollectionUrl,
	type TestDatabase,
	type TestTenant,
	updateTestEntity,
} from './helpers';

test.describe('Audit route', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'audroute');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'notes');
		await createTestCollection(request, db, 'other');
		await createTestEntity(request, db, 'notes', 'note-001', { title: 'Original' });
		await updateTestEntity(request, db, 'notes', 'note-001', { title: 'Changed' }, 1);
		await createTestEntity(request, db, 'other', 'other-001', { title: 'Other' });
	});

	test('filters audit entries and reverts an update entry', async ({ page }) => {
		const dataPlaneRequests: string[] = [];
		page.on('request', (request) => {
			const path = new URL(request.url()).pathname;
			if (
				path.startsWith(
					`/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/`,
				)
			) {
				dataPlaneRequests.push(path);
			}
		});

		await page.goto(dbAuditUrl(db));
		await expect(page.getByRole('heading', { name: 'Audit Log', level: 1 })).toBeVisible();

		await page.getByLabel('Collection').fill('notes');
		await page.getByRole('button', { name: 'Apply Filters' }).click();

		await expect(page.locator('tr', { hasText: 'notes' }).first()).toBeVisible({
			timeout: 10_000,
		});
		await expect(page.locator('tr', { hasText: 'other' })).toHaveCount(0);

		await page.locator('tr', { hasText: 'entity.update' }).first().click();
		await expect(page.getByRole('heading', { name: /Entry #/ })).toBeVisible();
		await page.getByRole('button', { name: 'Revert this change' }).click();
		await page.getByRole('button', { name: 'Yes' }).click();
		await expect(page.getByText(/reverted successfully/)).toBeVisible({ timeout: 10_000 });

		await page.goto(dbCollectionUrl(db, 'notes'));
		await expect(page.getByText('Original').first()).toBeVisible({ timeout: 10_000 });

		expect(dataPlaneRequests.some((path) => path.endsWith('/graphql'))).toBe(true);
		expect(
			dataPlaneRequests.filter((path) => !path.endsWith('/graphql')),
			'Audit route filter/revert workflow should use tenant-scoped GraphQL for data-plane calls',
		).toEqual([]);
	});
});
