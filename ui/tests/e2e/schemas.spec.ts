import { expect, test } from '@playwright/test';
import type { APIRequestContext } from '@playwright/test';

/**
 * E2E tests for the schema management workflow.
 *
 * Uses relative URLs so the baseURL from the active Playwright config applies.
 * Collections are prefixed with "schema-e2e-" to avoid conflicts with other test files.
 */

// ── Helpers ──────────────────────────────────────────────────────────────────

async function createCollectionViaApi(
	request: APIRequestContext,
	name: string,
	entitySchema: Record<string, unknown> = {},
	required: string[] = [],
): Promise<void> {
	const schema: Record<string, unknown> = {
		description: null,
		version: 1,
		entity_schema:
			Object.keys(entitySchema).length > 0
				? { type: 'object', properties: entitySchema, required }
				: {},
		link_types: {},
	};

	const response = await request.post(`/collections/${encodeURIComponent(name)}`, {
		data: { schema, actor: 'e2e-test' },
	});

	// 409 Conflict means collection already exists — that is fine.
	if (!response.ok() && response.status() !== 409) {
		const text = await response.text();
		throw new Error(`Failed to create collection "${name}": ${response.status()} ${text}`);
	}
}

/** Click a collection button in the left-hand Collections panel. */
async function clickCollection(page: import('@playwright/test').Page, name: string) {
	const collectionsPanel = page
		.locator('section.panel')
		.filter({ hasText: 'Collections' })
		.first();
	await collectionsPanel
		.locator('.panel-body button')
		.filter({ hasText: name })
		.click();
}

// ── Schema detail view ────────────────────────────────────────────────────────

test.describe('Schema detail view', () => {
	const COLLECTION = 'schema-e2e-typed';

	test.beforeAll(async ({ request }) => {
		await createCollectionViaApi(
			request,
			COLLECTION,
			{
				title: { type: 'string' },
				status: { type: 'string', enum: ['open', 'done'] },
				priority: { type: 'integer' },
			},
			['title', 'status'],
		);
	});

	test('schemas page shows collections panel', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await expect(page.locator('h1')).toContainText('Schemas', { timeout: 15000 });

		const collectionsPanel = page
			.locator('section.panel')
			.filter({ hasText: 'Collections' })
			.first();
		await expect(collectionsPanel).toBeVisible({ timeout: 15000 });

		await expect(
			collectionsPanel.locator('.panel-body button').filter({ hasText: COLLECTION }),
		).toBeVisible({ timeout: 15000 });
	});

	test('clicking collection shows schema detail', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		const schemaMeta = page.locator('.schema-meta');
		await expect(schemaMeta).toBeVisible({ timeout: 15000 });
		await expect(schemaMeta).toContainText(COLLECTION, { timeout: 15000 });
		await expect(schemaMeta).toContainText('v1', { timeout: 15000 });
	});

	test('schema detail shows entity fields table', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		const entityFieldsSection = page
			.locator('.schema-section')
			.filter({ hasText: 'Entity Fields' });
		await expect(entityFieldsSection).toBeVisible({ timeout: 15000 });

		const table = entityFieldsSection.locator('table');
		await expect(table.locator('th').filter({ hasText: 'Field' })).toBeVisible({ timeout: 15000 });
		await expect(table.locator('th').filter({ hasText: 'Type' })).toBeVisible({ timeout: 15000 });
		await expect(table.locator('th').filter({ hasText: 'Required' })).toBeVisible({
			timeout: 15000,
		});
		await expect(table.locator('th').filter({ hasText: 'Constraints' })).toBeVisible({
			timeout: 15000,
		});

		await expect(table.locator('code').filter({ hasText: 'title' }).first()).toBeVisible({
			timeout: 15000,
		});
		await expect(table.locator('code').filter({ hasText: 'status' }).first()).toBeVisible({
			timeout: 15000,
		});
		await expect(table.locator('code').filter({ hasText: 'priority' }).first()).toBeVisible({
			timeout: 15000,
		});
	});

	test('schema detail has Raw JSON toggle', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Raw JSON' }).click();

		const pre = page.locator('pre');
		await expect(pre).toBeVisible({ timeout: 15000 });
		await expect(pre).toContainText('entity_schema', { timeout: 15000 });

		await page.getByRole('button', { name: 'Structured' }).click();

		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });
	});
});

// ── Schema editing ────────────────────────────────────────────────────────────

test.describe('Schema editing', () => {
	const COLLECTION = 'schema-e2e-edit';

	test.beforeAll(async ({ request }) => {
		await createCollectionViaApi(request, COLLECTION, { name: { type: 'string' } });
	});

	test('edit mode opens textarea with current schema JSON', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();

		const textarea = page.locator('section.stack section.panel').nth(1).locator('textarea').last();
		await expect(textarea).toBeVisible({ timeout: 15000 });

		const content = await textarea.inputValue();
		expect(() => JSON.parse(content)).not.toThrow();

		await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible({ timeout: 15000 });
		await expect(page.getByRole('button', { name: 'Preview Changes' })).toBeVisible({
			timeout: 15000,
		});
	});

	test('cancel edit restores read-only view', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();
		await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Cancel' }).click();

		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });
		await expect(page.getByRole('button', { name: 'Edit', exact: true })).toBeVisible({ timeout: 15000 });
	});

	test('invalid JSON shows inline error', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();

		const schemaDetailPanel = page.locator('section.stack section.panel').nth(1);
		const textarea = schemaDetailPanel.locator('textarea').last();
		await expect(textarea).toBeVisible({ timeout: 15000 });

		await textarea.fill('not json {');
		await textarea.dispatchEvent('input');

		const errorMsg = page.locator('.message.error');
		await expect(errorMsg).toBeVisible({ timeout: 15000 });
		const errorText = await errorMsg.innerText();
		const mentionsJson =
			errorText.toLowerCase().includes('json') ||
			errorText.toLowerCase().includes('unexpected') ||
			errorText.toLowerCase().includes('token');
		expect(mentionsJson).toBe(true);
	});

	test('valid edit shows preview before save', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();

		const schemaDetailPanel = page.locator('section.stack section.panel').nth(1);
		const textarea = schemaDetailPanel.locator('textarea').last();
		await expect(textarea).toBeVisible({ timeout: 15000 });

		const currentJson = await textarea.inputValue();
		const current = JSON.parse(currentJson) as Record<string, unknown>;

		const updated = {
			...current,
			version: (typeof current.version === 'number' ? current.version : 1) + 1,
			entity_schema: {
				type: 'object',
				properties: {
					name: { type: 'string' },
					score: { type: 'integer' },
				},
			},
		};

		await textarea.fill(JSON.stringify(updated, null, 2));
		await textarea.dispatchEvent('input');

		const previewBtn = page.getByRole('button', { name: 'Preview Changes' });
		await expect(previewBtn).toBeEnabled({ timeout: 15000 });
		await previewBtn.click();

		const previewPanel = page.locator('.preview-panel');
		await expect(previewPanel).toBeVisible({ timeout: 15000 });
		await expect(previewPanel).toContainText('Schema Change Preview', { timeout: 15000 });

		const saveSchema = page.getByRole('button', { name: 'Save Schema' });
		const forceSave = page.getByRole('button', { name: 'Force Save' });
		const hasSave = (await saveSchema.count()) > 0 || (await forceSave.count()) > 0;
		expect(hasSave).toBe(true);
	});
});

// ── Schema create collection form ─────────────────────────────────────────────

test.describe('Schema create collection form', () => {
	test('create collection form has entity schema textarea', async ({ page }) => {
		await page.goto('/ui/schemas');
		await page.waitForLoadState('networkidle');

		const nameInput = page.locator('input[placeholder="tasks"]');
		await expect(nameInput).toBeVisible({ timeout: 15000 });

		const createPanel = page
			.locator('section.panel')
			.filter({ hasText: 'Create Collection' });
		const schemaTextarea = createPanel.locator('textarea');
		await expect(schemaTextarea).toBeVisible({ timeout: 15000 });

		const createButton = createPanel.getByRole('button', { name: 'Create Collection' });
		await nameInput.fill('');
		await expect(createButton).toBeDisabled({ timeout: 15000 });

		await nameInput.fill('schema-e2e-temp');
		await expect(createButton).toBeEnabled({ timeout: 15000 });
	});
});
