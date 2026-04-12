import { expect, test } from '@playwright/test';

/**
 * E2E tests for the schema management workflow.
 *
 * Tests run against a real axon-server with in-memory storage (no auth).
 * Collections are prefixed with "schema-e2e-" to avoid conflicts with other test files.
 *
 * Describe groups:
 *   - "Schema detail view"  uses collection "schema-e2e-typed"  (tests 1–4)
 *   - "Schema editing"      uses collection "schema-e2e-edit"   (tests 5–8)
 *   - "Schema create form"  no beforeAll needed                  (test 9)
 */

const BASE = 'http://localhost:4170';

// ── Helpers ──────────────────────────────────────────────────────────────────

async function createCollectionViaApi(
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

	const response = await fetch(`${BASE}/collections/${encodeURIComponent(name)}`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ schema, actor: 'e2e-test' }),
	});

	if (!response.ok) {
		const text = await response.text();
		// 409 Conflict means collection already exists — that is fine for our purposes.
		if (response.status !== 409) {
			throw new Error(`Failed to create collection "${name}": ${response.status} ${text}`);
		}
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

	test.beforeAll(async () => {
		await createCollectionViaApi(
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
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		// Page heading
		await expect(page.locator('h1')).toContainText('Schemas', { timeout: 15000 });

		// Left-hand Collections panel
		const collectionsPanel = page
			.locator('section.panel')
			.filter({ hasText: 'Collections' })
			.first();
		await expect(collectionsPanel).toBeVisible({ timeout: 15000 });

		// Our collection must appear as a button in the panel
		await expect(
			collectionsPanel.locator('.panel-body button').filter({ hasText: COLLECTION }),
		).toBeVisible({ timeout: 15000 });
	});

	test('clicking collection shows schema detail', async ({ page }) => {
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		// schema-meta block should appear
		const schemaMeta = page.locator('.schema-meta');
		await expect(schemaMeta).toBeVisible({ timeout: 15000 });

		// Must contain the collection name and version pill "v1"
		await expect(schemaMeta).toContainText(COLLECTION, { timeout: 15000 });
		await expect(schemaMeta).toContainText('v1', { timeout: 15000 });
	});

	test('schema detail shows entity fields table', async ({ page }) => {
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		// Wait for the Entity Fields section
		const entityFieldsSection = page
			.locator('.schema-section')
			.filter({ hasText: 'Entity Fields' });
		await expect(entityFieldsSection).toBeVisible({ timeout: 15000 });

		// Verify table column headers
		const table = entityFieldsSection.locator('table');
		await expect(table.locator('th').filter({ hasText: 'Field' })).toBeVisible({ timeout: 15000 });
		await expect(table.locator('th').filter({ hasText: 'Type' })).toBeVisible({ timeout: 15000 });
		await expect(table.locator('th').filter({ hasText: 'Required' })).toBeVisible({
			timeout: 15000,
		});
		await expect(table.locator('th').filter({ hasText: 'Constraints' })).toBeVisible({
			timeout: 15000,
		});

		// Verify rows contain each property name
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
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		// Wait for read-only schema view to load
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		// Click "Raw JSON"
		await page.getByRole('button', { name: 'Raw JSON' }).click();

		// pre element should appear and contain "entity_schema"
		const pre = page.locator('pre');
		await expect(pre).toBeVisible({ timeout: 15000 });
		await expect(pre).toContainText('entity_schema', { timeout: 15000 });

		// Click "Structured" to go back
		await page.getByRole('button', { name: 'Structured' }).click();

		// schema-meta should be visible again
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });
	});
});

// ── Schema editing ────────────────────────────────────────────────────────────

test.describe('Schema editing', () => {
	const COLLECTION = 'schema-e2e-edit';

	test.beforeAll(async () => {
		await createCollectionViaApi(COLLECTION, { name: { type: 'string' } });
	});

	test('edit mode opens textarea with current schema JSON', async ({ page }) => {
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);

		// Wait for schema detail to load before clicking Edit
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();

		// Textarea should be visible and contain valid JSON
		// Use structural selector: Schema Detail is the 2nd panel in the .stack.
		// The panel heading changes from "Schema Detail" to the collection name after selection,
		// so filter({ hasText: 'Schema Detail' }) stops matching once a collection is selected.
		const textarea = page.locator('section.stack section.panel').nth(1).locator('textarea').last();
		await expect(textarea).toBeVisible({ timeout: 15000 });

		const content = await textarea.inputValue();
		// Verify the content is parseable JSON
		expect(() => JSON.parse(content)).not.toThrow();

		// Cancel and Preview Changes buttons must appear
		await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible({ timeout: 15000 });
		await expect(page.getByRole('button', { name: 'Preview Changes' })).toBeVisible({
			timeout: 15000,
		});
	});

	test('cancel edit restores read-only view', async ({ page }) => {
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();
		await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Cancel' }).click();

		// Read-only view is restored
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		// Edit button is back
		await expect(page.getByRole('button', { name: 'Edit', exact: true })).toBeVisible({ timeout: 15000 });
	});

	test('invalid JSON shows inline error', async ({ page }) => {
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();

		// Target the edit textarea (last textarea on the page, inside the Schema Detail panel).
		// Use structural selector: Schema Detail is the 2nd panel in the .stack.
		const schemaDetailPanel = page.locator('section.stack section.panel').nth(1);
		const textarea = schemaDetailPanel.locator('textarea').last();
		await expect(textarea).toBeVisible({ timeout: 15000 });

		// Replace content with invalid JSON and fire an input event so validation runs
		await textarea.fill('not json {');
		await textarea.dispatchEvent('input');

		// Inline error message should appear
		const errorMsg = page.locator('.message.error');
		await expect(errorMsg).toBeVisible({ timeout: 15000 });
		// Message should mention JSON parsing failure
		const errorText = await errorMsg.innerText();
		const mentionsJson =
			errorText.toLowerCase().includes('json') ||
			errorText.toLowerCase().includes('unexpected') ||
			errorText.toLowerCase().includes('token');
		expect(mentionsJson).toBe(true);
	});

	test('valid edit shows preview before save', async ({ page }) => {
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		await clickCollection(page, COLLECTION);
		await expect(page.locator('.schema-meta')).toBeVisible({ timeout: 15000 });

		await page.getByRole('button', { name: 'Edit', exact: true }).click();

		// Use structural selector: Schema Detail is the 2nd panel in the .stack.
		const schemaDetailPanel = page.locator('section.stack section.panel').nth(1);
		const textarea = schemaDetailPanel.locator('textarea').last();
		await expect(textarea).toBeVisible({ timeout: 15000 });

		// Fetch the current schema JSON and construct an updated version
		const currentJson = await textarea.inputValue();
		const current = JSON.parse(currentJson) as Record<string, unknown>;

		// Bump the version and add an extra property to produce a real change
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

		// Preview Changes button should be enabled (no validation error)
		const previewBtn = page.getByRole('button', { name: 'Preview Changes' });
		await expect(previewBtn).toBeEnabled({ timeout: 15000 });
		await previewBtn.click();

		// Preview panel should appear
		const previewPanel = page.locator('.preview-panel');
		await expect(previewPanel).toBeVisible({ timeout: 15000 });

		// "Schema Change Preview" heading inside the panel
		await expect(previewPanel).toContainText('Schema Change Preview', { timeout: 15000 });

		// Either "Save Schema" (compatible) or "Force Save" (breaking) must appear
		const saveSchema = page.getByRole('button', { name: 'Save Schema' });
		const forceSave = page.getByRole('button', { name: 'Force Save' });
		const hasSave = (await saveSchema.count()) > 0 || (await forceSave.count()) > 0;
		expect(hasSave).toBe(true);
	});
});

// ── Schema create collection form ─────────────────────────────────────────────

test.describe('Schema create collection form', () => {
	test('create collection form has entity schema textarea', async ({ page }) => {
		await page.goto(`${BASE}/ui/schemas`);
		await page.waitForLoadState('networkidle');

		// Name input is visible
		const nameInput = page.locator('input[placeholder="tasks"]');
		await expect(nameInput).toBeVisible({ timeout: 15000 });

		// Entity Schema JSON textarea is visible (inside the Create Collection panel)
		const createPanel = page
			.locator('section.panel')
			.filter({ hasText: 'Create Collection' });
		const schemaTextarea = createPanel.locator('textarea');
		await expect(schemaTextarea).toBeVisible({ timeout: 15000 });

		// "Create Collection" button is disabled when name is empty
		const createButton = createPanel.getByRole('button', { name: 'Create Collection' });
		await nameInput.fill('');
		await expect(createButton).toBeDisabled({ timeout: 15000 });

		// Filling in a name enables the button
		await nameInput.fill('schema-e2e-temp');
		await expect(createButton).toBeEnabled({ timeout: 15000 });
	});
});
