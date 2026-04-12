import { expect, test } from '@playwright/test';

/**
 * E2E tests for the Audit Log page against a real axon-server.
 *
 * Uses collection "e2e-audit" and creates an entity via the API in beforeAll so
 * at least one audit entry exists when the log page is loaded.
 */

const COLLECTION_NAME = 'e2e-audit';
const ENTITY_ID = 'audit-entity-001';

test.describe('Audit Log', () => {
	test.beforeAll(async ({ request }) => {
		// Create collection.
		const collResp = await request.post(
			`http://localhost:4170/collections/${COLLECTION_NAME}`,
			{
				data: {
					schema: {
						description: null,
						version: 1,
						entity_schema: { type: 'object', properties: {} },
						link_types: {},
					},
					actor: 'e2e-test',
				},
			},
		);
		expect([201, 409]).toContain(collResp.status());

		// Create an entity so there is an audit entry to show.
		const entityResp = await request.post(
			`http://localhost:4170/entities/${COLLECTION_NAME}/${ENTITY_ID}`,
			{
				data: { data: { note: 'audit test' }, actor: 'e2e-test' },
			},
		);
		expect([201, 409]).toContain(entityResp.status());
	});

	test('Audit Log heading is visible', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		await expect(page.getByRole('heading', { name: 'Audit Log' })).toBeVisible({ timeout: 15000 });
	});

	test('audit log page has filter controls', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		await expect(page.getByPlaceholder('All collections')).toBeVisible({ timeout: 15000 });
		await expect(page.getByPlaceholder('All actors')).toBeVisible({ timeout: 15000 });
		await expect(page.getByRole('button', { name: 'Apply Filters' })).toBeVisible();
	});

	test('audit log contains entries after entity creation', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		// Wait for the "Recent Entries" panel to load (loading spinner disappears).
		const recentPanel = page.locator('section.panel').filter({ hasText: 'Recent Entries' });
		await expect(recentPanel).toBeVisible({ timeout: 15000 });
		await expect(recentPanel.getByText('Loading audit entries')).not.toBeVisible({
			timeout: 15000,
		});

		// There should be at least one audit entry row visible.
		const auditTable = recentPanel.locator('table');
		await expect(auditTable).toBeVisible({ timeout: 15000 });

		// Our collection should appear in the audit table (multiple rows may match).
		await expect(auditTable.getByText(COLLECTION_NAME).first()).toBeVisible({ timeout: 15000 });
	});

	test('clicking an audit row shows entry detail', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		const recentPanel = page.locator('section.panel').filter({ hasText: 'Recent Entries' });
		await expect(recentPanel).toBeVisible({ timeout: 15000 });

		const auditTable = recentPanel.locator('table');
		await expect(auditTable).toBeVisible({ timeout: 15000 });

		// Click the first data row.
		const firstRow = auditTable.locator('tbody tr').first();
		await expect(firstRow).toBeVisible({ timeout: 15000 });
		await firstRow.click();

		// The detail panel is the second panel in the two-column grid.
		// The audit page auto-selects the first entry, so the heading shows "Entry #N".
		const detailPanel = page.locator('.two-column section.panel').nth(1);
		await expect(detailPanel.getByRole('heading', { name: /Entry #\d+/ })).toBeVisible({
			timeout: 15000,
		});

		// The before/after sections should be rendered.
		await expect(detailPanel.getByRole('heading', { name: 'Before' })).toBeVisible();
		await expect(detailPanel.getByRole('heading', { name: 'After' })).toBeVisible();
	});
});

test.describe('Audit Log filtering', () => {
	// The beforeAll from "Audit Log" already runs when tests execute in order,
	// but we need our own to guarantee the collection and entity exist regardless
	// of test ordering.
	test.beforeAll(async ({ request }) => {
		const collResp = await request.post(
			`http://localhost:4170/collections/${COLLECTION_NAME}`,
			{
				data: {
					schema: {
						description: null,
						version: 1,
						entity_schema: { type: 'object', properties: {} },
						link_types: {},
					},
					actor: 'e2e-test',
				},
			},
		);
		expect([201, 409]).toContain(collResp.status());

		const entityResp = await request.post(
			`http://localhost:4170/entities/${COLLECTION_NAME}/${ENTITY_ID}`,
			{
				data: { data: { note: 'audit filter test' }, actor: 'e2e-test' },
			},
		);
		expect([201, 409]).toContain(entityResp.status());
	});

	test('audit table shows all expected columns', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		const recentPanel = page.locator('section.panel').filter({ hasText: 'Recent Entries' });
		await expect(recentPanel).toBeVisible({ timeout: 15000 });

		const auditTable = recentPanel.locator('table');
		await expect(auditTable).toBeVisible({ timeout: 15000 });

		const thead = auditTable.locator('thead');
		await expect(thead.locator('th', { hasText: 'ID' })).toBeVisible();
		await expect(thead.locator('th', { hasText: 'Timestamp' })).toBeVisible();
		await expect(thead.locator('th', { hasText: 'Collection' })).toBeVisible();
		await expect(thead.locator('th', { hasText: 'Entity' })).toBeVisible();
		await expect(thead.locator('th', { hasText: 'Operation' })).toBeVisible();
		await expect(thead.locator('th', { hasText: 'Actor' })).toBeVisible();
	});

	test('filter by collection narrows results', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		const recentPanel = page.locator('section.panel').filter({ hasText: 'Recent Entries' });
		await expect(recentPanel).toBeVisible({ timeout: 15000 });
		const auditTable = recentPanel.locator('table');
		await expect(auditTable).toBeVisible({ timeout: 15000 });

		// Apply a collection filter for "e2e-audit".
		await page.getByPlaceholder('All collections').fill(COLLECTION_NAME);
		await page.getByRole('button', { name: 'Apply Filters' }).click();
		await page.waitForLoadState('networkidle');

		// All visible Collection cells (3rd column) should contain "e2e-audit".
		const collectionCells = auditTable.locator('tbody tr td:nth-child(3)');
		const count = await collectionCells.count();
		expect(count).toBeGreaterThan(0);
		for (let i = 0; i < count; i++) {
			await expect(collectionCells.nth(i)).toContainText(COLLECTION_NAME);
		}

		// Negative test: filter by a non-existent collection name.
		await page.getByPlaceholder('All collections').fill('zzznonexistent');
		await page.getByRole('button', { name: 'Apply Filters' }).click();
		await page.waitForLoadState('networkidle');

		await expect(
			page.getByText('No audit entries matched the current filters.'),
		).toBeVisible({ timeout: 15000 });
	});

	test('filter by actor shows entries for that actor', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		const recentPanel = page.locator('section.panel').filter({ hasText: 'Recent Entries' });
		await expect(recentPanel).toBeVisible({ timeout: 15000 });

		// In --no-auth mode all entries are recorded with actor="anonymous" regardless of
		// the actor field sent in the request body.
		await page.getByPlaceholder('All actors').fill('anonymous');
		await page.getByRole('button', { name: 'Apply Filters' }).click();
		await page.waitForLoadState('networkidle');

		const auditTable = recentPanel.locator('table');
		await expect(auditTable).toBeVisible({ timeout: 15000 });

		// All visible Actor cells (last column, 6th) should contain "anonymous".
		const actorCells = auditTable.locator('tbody tr td:nth-child(6)');
		const count = await actorCells.count();
		expect(count).toBeGreaterThan(0);
		for (let i = 0; i < count; i++) {
			await expect(actorCells.nth(i)).toContainText('anonymous');
		}
	});

	test('clear filters restores all entries', async ({ page }) => {
		await page.goto('http://localhost:4170/ui/audit');
		await page.waitForLoadState('networkidle');

		const recentPanel = page.locator('section.panel').filter({ hasText: 'Recent Entries' });
		await expect(recentPanel).toBeVisible({ timeout: 15000 });

		// First apply a collection filter.
		await page.getByPlaceholder('All collections').fill(COLLECTION_NAME);
		await page.getByRole('button', { name: 'Apply Filters' }).click();
		await page.waitForLoadState('networkidle');

		// Now clear the filter and re-apply.
		await page.getByPlaceholder('All collections').fill('');
		await page.getByRole('button', { name: 'Apply Filters' }).click();
		await page.waitForLoadState('networkidle');

		// The audit table should be visible and contain entries.
		const auditTable = recentPanel.locator('table');
		await expect(auditTable).toBeVisible({ timeout: 15000 });
		await expect(auditTable.locator('tbody tr').first()).toBeVisible({ timeout: 15000 });
	});
});
