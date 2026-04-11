/**
 * Shared Playwright test fixtures that mock the Axon API.
 *
 * Every API endpoint that the UI calls via the Vite proxy is intercepted
 * with `page.route()` so the tests can run without a live Axon server.
 *
 * IMPORTANT: The SvelteKit app uses `base: '/ui'`, so page navigations go to
 * `/ui/collections`, `/ui/schemas`, etc. The client-side API `fetch()` calls
 * go to bare paths like `/collections`, `/entities/...`, `/health`, `/audit/query`.
 * Route patterns must NOT match `/ui/...` paths to avoid intercepting page loads.
 */
import { test as base, type Page } from '@playwright/test';

// ── Mock data ────────────────────────────────────────────────────────

export const MOCK_HEALTH = {
	status: 'healthy',
	version: '0.1.0-test',
	uptime_seconds: 42,
	backing_store: { backend: 'sqlite', status: 'ok' },
	databases: ['default'],
	default_namespace: 'default',
};

export const MOCK_COLLECTIONS: {
	name: string;
	entity_count: number;
	schema_version: number | null;
	created_at_ns: number | null;
	updated_at_ns: number | null;
}[] = [
	{
		name: 'tasks',
		entity_count: 3,
		schema_version: 1,
		created_at_ns: 1_700_000_000_000_000_000,
		updated_at_ns: 1_700_100_000_000_000_000,
	},
	{
		name: 'notes',
		entity_count: 0,
		schema_version: null,
		created_at_ns: 1_700_050_000_000_000_000,
		updated_at_ns: 1_700_050_000_000_000_000,
	},
];

export const MOCK_TASKS_DETAIL = {
	name: 'tasks',
	entity_count: 3,
	schema: {
		collection: 'tasks',
		description: 'Task tracking collection',
		version: 1,
		entity_schema: {
			type: 'object',
			properties: {
				title: { type: 'string', minLength: 1 },
				status: { type: 'string', enum: ['open', 'in_progress', 'done'] },
				priority: { type: 'integer', minimum: 1, maximum: 5 },
			},
			required: ['title', 'status'],
		},
		link_types: {},
	},
	created_at_ns: 1_700_000_000_000_000_000,
	updated_at_ns: 1_700_100_000_000_000_000,
};

export const MOCK_TASKS_SCHEMA = {
	schema: MOCK_TASKS_DETAIL.schema,
};

export const MOCK_ENTITIES = [
	{
		collection: 'tasks',
		id: 'task-001',
		version: 1,
		data: { title: 'Write tests', status: 'open', priority: 1 },
		schema_version: 1,
	},
	{
		collection: 'tasks',
		id: 'task-002',
		version: 2,
		data: { title: 'Fix bug', status: 'in_progress', priority: 3 },
		schema_version: 1,
	},
	{
		collection: 'tasks',
		id: 'task-003',
		version: 1,
		data: { title: 'Deploy', status: 'done', priority: 2 },
		schema_version: 1,
	},
];

export const MOCK_QUERY_RESULT = {
	entities: MOCK_ENTITIES,
	total_count: 3,
	next_cursor: null,
};

export const MOCK_AUDIT_ENTRIES = {
	entries: [
		{
			id: 1,
			timestamp_ns: 1_700_000_000_000_000_000,
			collection: 'tasks',
			entity_id: 'task-001',
			version: 1,
			mutation: 'create',
			data_before: null,
			data_after: { title: 'Write tests', status: 'open', priority: 1 },
			actor: 'ui',
			transaction_id: null,
		},
		{
			id: 2,
			timestamp_ns: 1_700_050_000_000_000_000,
			collection: 'tasks',
			entity_id: 'task-002',
			version: 1,
			mutation: 'create',
			data_before: null,
			data_after: { title: 'Fix bug', status: 'open', priority: 3 },
			actor: 'system',
			transaction_id: null,
		},
	],
	next_cursor: null,
};

// ── Route-mocking helper ─────────────────────────────────────────────

/**
 * Returns true if the URL path starts with /ui/ — meaning it is a
 * SvelteKit page navigation, not an API call.
 */
function isPageNavigation(url: string): boolean {
	const path = new URL(url).pathname;
	return path.startsWith('/ui/') || path === '/ui';
}

/**
 * Install API route mocks on the given page. Call this before any
 * `page.goto(...)` so the interceptors are already registered.
 */
export async function mockAxonApi(page: Page) {
	// GET /health
	await page.route('**/health', (route) => {
		if (route.request().method() !== 'GET') return route.fallback();
		if (isPageNavigation(route.request().url())) return route.fallback();
		return route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(MOCK_HEALTH),
		});
	});

	// GET /collections — list all collections
	// Must not match /ui/collections (page navigation).
	await page.route(/\/collections$/, (route) => {
		if (route.request().method() !== 'GET') return route.fallback();
		if (isPageNavigation(route.request().url())) return route.fallback();
		return route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify({ collections: MOCK_COLLECTIONS }),
		});
	});

	// GET /collections/:name — single collection detail
	// POST /collections/:name — create collection
	// Must not match /ui/collections/:name (page navigation).
	await page.route(/\/collections\/([^/]+)$/, (route) => {
		if (isPageNavigation(route.request().url())) return route.fallback();

		const method = route.request().method();
		const url = new URL(route.request().url());
		const name = decodeURIComponent(url.pathname.split('/').filter(Boolean).pop() ?? '');

		if (method === 'GET') {
			const detail = name === 'tasks' ? MOCK_TASKS_DETAIL : {
				name,
				entity_count: 0,
				schema: null,
				created_at_ns: null,
				updated_at_ns: null,
			};
			return route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(detail),
			});
		}

		if (method === 'POST') {
			return route.fulfill({
				status: 201,
				contentType: 'application/json',
				body: JSON.stringify({ name }),
			});
		}

		return route.fallback();
	});

	// POST /collections/:name/query — entity listing
	await page.route('**/collections/*/query', (route) => {
		if (route.request().method() !== 'POST') return route.fallback();
		return route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(MOCK_QUERY_RESULT),
		});
	});

	// GET/PUT /collections/:name/schema
	await page.route('**/collections/*/schema', (route) => {
		const method = route.request().method();
		if (method === 'GET') {
			return route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(MOCK_TASKS_SCHEMA),
			});
		}
		if (method === 'PUT') {
			return route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					schema: MOCK_TASKS_DETAIL.schema,
					compatibility: 'compatible',
					diff: { compatibility: 'compatible', changes: [] },
					dry_run: true,
				}),
			});
		}
		return route.fallback();
	});

	// GET /entities/:collection/:id — single entity
	await page.route(/\/entities\/([^/]+)\/([^/]+)$/, (route) => {
		const method = route.request().method();
		const url = new URL(route.request().url());
		const parts = url.pathname.split('/').filter(Boolean);
		const entityId = decodeURIComponent(parts[parts.length - 1] ?? '');

		if (method === 'GET') {
			const entity = MOCK_ENTITIES.find((e) => e.id === entityId) ?? MOCK_ENTITIES[0];
			return route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ entity }),
			});
		}

		if (method === 'POST') {
			// Create entity
			return route.fulfill({
				status: 201,
				contentType: 'application/json',
				body: JSON.stringify({
					entity: {
						collection: 'tasks',
						id: entityId,
						version: 1,
						data: {},
						schema_version: 1,
					},
				}),
			});
		}

		return route.fallback();
	});

	// GET /audit/query
	await page.route('**/audit/query**', (route) => {
		if (route.request().method() !== 'GET') return route.fallback();
		return route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(MOCK_AUDIT_ENTRIES),
		});
	});
}

// ── Extended test fixture ────────────────────────────────────────────

/**
 * A custom Playwright `test` that automatically mocks the Axon API on
 * every test. Import this instead of `@playwright/test`'s `test`.
 */
export const test = base.extend<{ autoMock: void }>({
	// biome-ignore lint/correctness/noEmptyPattern: Playwright fixture pattern requires destructuring.
	autoMock: [async ({ page }, use) => {
		await mockAxonApi(page);
		await use();
	}, { auto: true }],
});

export { expect } from '@playwright/test';
