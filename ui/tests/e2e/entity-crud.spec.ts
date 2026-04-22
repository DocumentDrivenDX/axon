import { expect, test } from '@playwright/test';
import {
	createTestCollection,
	createTestDatabase,
	createTestTenant,
	dbCollectionUrl,
	type TestDatabase,
	type TestTenant,
} from './helpers';

test.describe('Entity CRUD', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'crud');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'notes');
	});

	test('creates, reads, updates, and deletes an entity from the collection detail route', async ({
		page,
	}) => {
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

		await page.goto(dbCollectionUrl(db, 'notes'));

		await page.getByLabel('Entity ID').fill('note-001');
		await page.getByLabel('Entity JSON').fill('{"title":"Draft","count":1}');
		await page
			.locator('section', { hasText: 'Entity JSON' })
			.getByRole('button', { name: 'Create Entity' })
			.click();
		const entityRow = page.locator('.entity-rail tbody tr', { hasText: 'note-001' });
		await expect(entityRow).toBeVisible({ timeout: 10_000 });
		await expect(page.getByText('Draft').first()).toBeVisible();

		await page.getByRole('button', { name: 'Edit' }).click();
		await page
			.locator('.tree-row', { hasText: 'title' })
			.locator('input.leaf-input')
			.fill('Published');
		await page.getByRole('button', { name: 'Save' }).click();
		await expect(page.getByText('Saved v2.')).toBeVisible({ timeout: 10_000 });
		await expect(page.getByText('Published').first()).toBeVisible();

		await page.getByRole('button', { name: 'Delete' }).click();
		await page.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.getByText('Deleted note-001.')).toBeVisible({ timeout: 10_000 });
		await expect(entityRow).toHaveCount(0);

		expect(dataPlaneRequests.some((path) => path.endsWith('/graphql'))).toBe(true);
		expect(
			dataPlaneRequests.filter((path) => !path.endsWith('/graphql')),
			'Entity CRUD UI should use tenant-scoped GraphQL for data-plane calls',
		).toEqual([]);
	});
});
