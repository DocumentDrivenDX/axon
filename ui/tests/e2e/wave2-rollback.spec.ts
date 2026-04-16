import { expect, test } from '@playwright/test';
import {
	createTestCollection,
	createTestDatabase,
	createTestEntity,
	createTestTenant,
	dbCollectionUrl,
	type TestDatabase,
	type TestTenant,
	updateTestEntity,
} from './helpers';

/**
 * Wave 2 capability coverage: entity rollback with dry-run preview.
 */
test.describe('entity rollback', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'rb');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'docs');
		// Create entity at v1, then update twice → v1, v2, v3
		await createTestEntity(request, db, 'docs', 'doc-001', { title: 'Version 1', note: 'first' });
		await updateTestEntity(request, db, 'docs', 'doc-001', { title: 'Version 2', note: 'second' }, 1);
		await updateTestEntity(request, db, 'docs', 'doc-001', { title: 'Version 3', note: 'third' }, 2);
	});

	test('rollback tab shows prior versions from audit history', async ({ page }) => {
		await page.goto(dbCollectionUrl(db, 'docs'));
		// Wait for entity list to load.
		await expect(page.getByText('Version 3').first()).toBeVisible({ timeout: 10_000 });

		// Click the Rollback tab.
		await page.getByTestId('entity-tab-rollback').click();

		// The rollback pane should be visible.
		const pane = page.getByTestId('entity-rollback-pane');
		await expect(pane).toBeVisible({ timeout: 5_000 });

		// Should list prior versions in the rollback table.
		const table = page.getByTestId('entity-rollback-table');
		await expect(table).toBeVisible({ timeout: 5_000 });

		// v1 and v2 are prior versions (current is v3).
		await expect(table.getByText('v1')).toBeVisible();
		await expect(table.getByText('v2')).toBeVisible();
	});

	test('clicking Preview shows dry-run diff', async ({ page }) => {
		await page.goto(dbCollectionUrl(db, 'docs'));
		await expect(page.getByText('Version 3').first()).toBeVisible({ timeout: 10_000 });

		await page.getByTestId('entity-tab-rollback').click();

		const table = page.getByTestId('entity-rollback-table');
		await expect(table).toBeVisible({ timeout: 5_000 });

		// Click Preview for v1.
		await page.getByTestId('rollback-preview-v1').click();

		// Preview panel should appear.
		const preview = page.getByTestId('entity-rollback-preview');
		await expect(preview).toBeVisible({ timeout: 10_000 });

		// Apply button should be present.
		await expect(page.getByTestId('rollback-apply-button')).toBeVisible();
	});

	// TODO: test full apply flow once backend is available in CI
	test.skip('applying rollback updates entity to target version', async () => {
		// Placeholder: navigate to entity, click Rollback tab, preview v2,
		// click Apply, verify success message and updated entity version.
	});
});
