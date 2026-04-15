import { expect, test } from '@playwright/test';
import {
	createTestCollection,
	createTestDatabase,
	createTestEntity,
	createTestTenant,
	dbCollectionUrl,
	dbCollectionsUrl,
	dbGraphqlUrl,
	type TestDatabase,
	type TestTenant,
} from './helpers';

/**
 * Wave 1 capability coverage: GraphQL console, entity audit history,
 * link management, lifecycle transitions, markdown template editor.
 */
test.describe('Wave 1 — GraphQL console', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'gql');
		db = await createTestDatabase(request, tenant);
		// GraphQL schema is built from collections; empty DB = empty schema
		// and the server returns GRAPHQL_SCHEMA_ERROR. Create a collection
		// so introspection has something to report.
		await createTestCollection(request, db, 'items', {
			entity_schema: {
				type: 'object',
				properties: { title: { type: 'string' } },
				required: ['title'],
			},
		});
	});

	test('page loads with query editor and response pane', async ({ page }) => {
		await page.goto(dbGraphqlUrl(db));
		await expect(page.getByRole('heading', { name: 'GraphQL', level: 1 })).toBeVisible();
		await expect(page.getByTestId('graphql-query')).toBeVisible();
		await expect(page.getByText(/Run a query/)).toBeVisible();
	});

	test('introspection query returns a schema', async ({ page }) => {
		await page.goto(dbGraphqlUrl(db));
		// Use the default query (an introspection on __schema).
		await page.getByRole('button', { name: /Run/ }).click();
		const response = page.getByTestId('graphql-response');
		await expect(response).toBeVisible({ timeout: 10_000 });
		const text = (await response.textContent()) ?? '';
		expect(text).toContain('"__schema"');
		expect(text).toContain('"queryType"');
	});

	test('invalid query returns GraphQL errors block', async ({ page }) => {
		await page.goto(dbGraphqlUrl(db));
		await page.getByTestId('graphql-query').fill('{ definitelyNotAField }');
		await page.getByRole('button', { name: /Run/ }).click();
		const response = page.getByTestId('graphql-response');
		await expect(response).toBeVisible({ timeout: 10_000 });
		const text = (await response.textContent()) ?? '';
		expect(text.toLowerCase()).toMatch(/error|unknown|field/);
	});
});

test.describe('Wave 1 — Entity audit history tab', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'audit');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'tasks');
		await createTestEntity(request, db, 'tasks', 't-001', {
			title: 'First task',
			status: 'open',
		});
	});

	test('history tab shows version entries for an entity', async ({ page }) => {
		await page.goto(dbCollectionUrl(db, 'tasks'));
		// Entity list loads and first entity auto-selects.
		await expect(page.getByText('First task').first()).toBeVisible();
		await page.getByTestId('entity-tab-audit').click();
		const timeline = page.getByTestId('entity-audit-timeline');
		await expect(timeline).toBeVisible({ timeout: 5_000 });
		await expect(timeline.getByText(/entity.create/)).toBeVisible();
		await expect(timeline.getByText('v1')).toBeVisible();
	});
});

test.describe('Wave 1 — Link management', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'links');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'tasks');
		await createTestCollection(request, db, 'projects');
		await createTestEntity(request, db, 'tasks', 't-001', { title: 'Task A' });
		await createTestEntity(request, db, 'projects', 'p-001', { name: 'Project X' });
	});

	test('create outbound link from task to project, then remove it', async ({ page }) => {
		await page.goto(dbCollectionUrl(db, 'tasks'));
		await expect(page.getByText('Task A').first()).toBeVisible();
		await page.getByTestId('entity-tab-links').click();

		// Create link via the form.
		await page.getByRole('button', { name: 'Add Link' }).click();
		await page.getByTestId('link-type-input').fill('belongs-to');
		await page.getByTestId('link-target-collection-input').fill('projects');
		await page.getByTestId('link-target-id-input').fill('p-001');
		await page.getByTestId('link-submit').click();

		// Link appears in the table.
		const linksTable = page.getByTestId('entity-links-table');
		await expect(linksTable).toBeVisible({ timeout: 5_000 });
		await expect(linksTable.getByText('belongs-to')).toBeVisible();
		await expect(linksTable.getByText('projects/p-001')).toBeVisible();

		// Remove the link.
		await linksTable.getByRole('button', { name: 'Remove' }).click();
		await expect(linksTable).toHaveCount(0);
		await expect(page.getByText('No outbound links.')).toBeVisible();
	});
});

test.describe('Wave 1 — Lifecycle transitions', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'lc');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'tickets', {
			entity_schema: {
				type: 'object',
				properties: {
					title: { type: 'string' },
					status: { type: 'string' },
				},
				required: ['title', 'status'],
			},
			lifecycles: {
				status: {
					field: 'status',
					initial: 'open',
					transitions: {
						open: ['in_progress', 'closed'],
						in_progress: ['closed'],
					},
				},
			},
		});
		await createTestEntity(request, db, 'tickets', 'tix-001', {
			title: 'Broken thing',
			status: 'open',
		});
	});

	test('lifecycle tab shows current state and transitions entity', async ({ page }) => {
		await page.goto(dbCollectionUrl(db, 'tickets'));
		await expect(page.getByText('Broken thing').first()).toBeVisible();
		await page.getByTestId('entity-tab-lifecycle').click();

		const currentState = page.getByTestId('lifecycle-current-state');
		await expect(currentState).toHaveText('open');

		await page.getByTestId('lifecycle-transition-in_progress').click();

		// After the transition the pill should update.
		await expect(currentState).toHaveText('in_progress', { timeout: 5_000 });
		// And a new transition target (closed) should be shown.
		await expect(page.getByTestId('lifecycle-transition-closed')).toBeVisible();
	});
});

test.describe('Wave 1 — Markdown template editor', () => {
	let tenant: TestTenant;
	let db: TestDatabase;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'tpl');
		db = await createTestDatabase(request, tenant);
		await createTestCollection(request, db, 'notes');
		await createTestEntity(request, db, 'notes', 'n-001', {
			title: 'Hello',
			body: 'World',
		});
	});

	test('create, preview, and delete a markdown template', async ({ page }) => {
		await page.goto(dbCollectionUrl(db, 'notes'));

		// Template section should show "no template" and a Create button.
		const section = page.getByTestId('collection-template-section');
		await expect(section).toBeVisible();
		await expect(section.getByText(/No markdown template set/)).toBeVisible();

		// Open the editor and save a template.
		await page.getByTestId('template-edit-button').click();
		await page.getByTestId('template-editor-textarea').fill('# {{title}}\n\n{{body}}');
		await page.getByRole('button', { name: 'Save' }).click();

		// After save, the display (non-edit mode) shows the template content.
		const display = page.getByTestId('template-display');
		await expect(display).toBeVisible({ timeout: 5_000 });
		await expect(display).toContainText('# {{title}}');

		// Now switch to the entity's Markdown tab and verify the render.
		await expect(page.getByText('Hello').first()).toBeVisible();
		await page.getByTestId('entity-tab-markdown').click();
		const rendered = page.getByTestId('entity-markdown-output');
		await expect(rendered).toBeVisible({ timeout: 5_000 });
		await expect(rendered).toContainText('# Hello');
		await expect(rendered).toContainText('World');

		// Delete the template.
		await section.getByRole('button', { name: 'Delete' }).click();
		await expect(section.getByText(/No markdown template set/)).toBeVisible({ timeout: 5_000 });
	});
});
