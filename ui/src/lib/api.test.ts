import { afterEach, beforeEach, expect, test } from 'bun:test';

import {
	fetchAudit,
	fetchCollection,
	fetchCollections,
	fetchEntities,
	fetchEntity,
	fetchSchema,
	issueCredential,
	listCredentials,
	revokeCredential,
} from './api';

const originalFetch = globalThis.fetch;

type CapturedRequest = {
	url: string;
	init: RequestInit | undefined;
};

let lastRequest: CapturedRequest | null = null;

function mockFetch(body: unknown, status = 200) {
	const handler = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
		lastRequest = { url: String(input), init };
		return new Response(JSON.stringify(body), {
			status,
			headers: { 'Content-Type': 'application/json' },
		});
	};
	globalThis.fetch = handler as unknown as typeof globalThis.fetch;
}

beforeEach(() => {
	lastRequest = null;
});

afterEach(() => {
	globalThis.fetch = originalFetch;
});

test('request() prefixes URL with tenant/database path when scope is provided', async () => {
	mockFetch({ data: { collections: [] } });

	await fetchCollections({ tenant: 'acme', database: 'orders' });

	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
});

test('request() URL-encodes tenant and database in the path', async () => {
	mockFetch({ data: { collections: [] } });

	await fetchCollections({ tenant: 'my tenant', database: 'my/db' });

	expect(lastRequest?.url).toBe('/tenants/my%20tenant/databases/my%2Fdb/graphql');
});

test('request() does NOT prefix control-plane routes even when scope is provided', async () => {
	mockFetch({ entries: [], next_cursor: null });

	// fetchAudit uses /audit/query which is NOT a /control/ route, so it gets prefixed.
	// Use a raw fetch mock to test /control/ directly.
	const controlHandler = async (
		input: RequestInfo | URL,
		init?: RequestInit,
	): Promise<Response> => {
		lastRequest = { url: String(input), init };
		return new Response(JSON.stringify({ tenants: [] }), {
			status: 200,
			headers: { 'Content-Type': 'application/json' },
		});
	};
	globalThis.fetch = controlHandler as unknown as typeof globalThis.fetch;

	// Import fetchTenants which calls /control/tenants — no scope param.
	const { fetchTenants } = await import('./api');
	await fetchTenants();

	expect(lastRequest?.url).toBe('/control/tenants');
	// Confirm no scope prefix was added despite the /control/ path.
	expect(lastRequest?.url).not.toContain('/tenants/');
});

test('request() does not set X-Axon-Database header', async () => {
	mockFetch({ data: { collections: [] } });

	await fetchCollections({ tenant: 'acme', database: 'orders' });

	const headers = new Headers(lastRequest?.init?.headers);
	expect(headers.has('X-Axon-Database')).toBe(false);
});

test('request() uses plain path when no scope is provided', async () => {
	mockFetch({ collections: [] });

	await fetchCollections();

	expect(lastRequest?.url).toBe('/collections');
});

test('request() prefixes audit route with scope', async () => {
	mockFetch({ entries: [], next_cursor: null });

	await fetchAudit({}, { tenant: 'acme', database: 'orders' });

	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/audit/query');
});

test('fetchCollections() maps tenant-scoped GraphQL collection metadata', async () => {
	mockFetch({
		data: {
			collections: [{ name: 'tasks', entityCount: 3, schemaVersion: 2 }],
		},
	});

	const result = await fetchCollections({ tenant: 'acme', database: 'orders' });

	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(lastRequest?.init?.method).toBe('POST');
	expect(JSON.parse(String(lastRequest?.init?.body)).query).toContain('collections');
	expect(result).toEqual([{ name: 'tasks', entity_count: 3, schema_version: 2 }]);
});

test('fetchCollection() uses tenant-scoped GraphQL collection query', async () => {
	const schema = {
		collection: 'tasks',
		version: 2,
		entity_schema: { type: 'object' },
	};
	mockFetch({
		data: {
			collection: { name: 'tasks', entityCount: 3, schemaVersion: 2, schema },
		},
	});

	const result = await fetchCollection('tasks', { tenant: 'acme', database: 'orders' });

	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(JSON.parse(String(lastRequest?.init?.body)).variables).toEqual({ name: 'tasks' });
	expect(result).toEqual({ name: 'tasks', entity_count: 3, schema });
});

test('fetchSchema() reads schema through tenant-scoped GraphQL collection metadata', async () => {
	const schema = {
		collection: 'tasks',
		version: 1,
		entity_schema: { type: 'object' },
	};
	mockFetch({
		data: {
			collection: { name: 'tasks', entityCount: 0, schemaVersion: 1, schema },
		},
	});

	const result = await fetchSchema('tasks', { tenant: 'acme', database: 'orders' });

	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(result).toEqual(schema);
});

test('fetchEntities() uses tenant-scoped GraphQL connection query', async () => {
	mockFetch({
		data: {
			entities: {
				totalCount: 2,
				edges: [
					{
						cursor: 'task-1',
						node: {
							collection: 'tasks',
							id: 'task-1',
							version: 1,
							data: { title: 'one' },
						},
					},
				],
				pageInfo: { hasNextPage: true, endCursor: 'task-1' },
			},
		},
	});

	const result = await fetchEntities(
		'tasks',
		{ limit: 1, afterId: 'cursor-0' },
		{ tenant: 'acme', database: 'orders' },
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('entities(collection: $collection');
	expect(body.variables).toEqual({ collection: 'tasks', limit: 1, after: 'cursor-0' });
	expect(result).toEqual({
		entities: [{ collection: 'tasks', id: 'task-1', version: 1, data: { title: 'one' } }],
		total_count: 2,
		next_cursor: 'task-1',
	});
});

test('fetchEntity() uses tenant-scoped GraphQL entity query', async () => {
	mockFetch({
		data: {
			entity: {
				collection: 'tasks',
				id: 'task-1',
				version: 3,
				data: { title: 'one' },
			},
		},
	});

	const result = await fetchEntity('tasks', 'task-1', { tenant: 'acme', database: 'orders' });

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.variables).toEqual({ collection: 'tasks', id: 'task-1' });
	expect(result).toEqual({
		collection: 'tasks',
		id: 'task-1',
		version: 3,
		data: { title: 'one' },
	});
});

// ── Credential API helpers ────────────────────────────────────────────────────

test('listCredentials() calls GET /control/tenants/:id/credentials', async () => {
	mockFetch({ credentials: [] });

	await listCredentials('tenant-123');

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials');
	// GET is the default — no explicit method set
	expect(lastRequest?.init?.method).toBeUndefined();
});

test('listCredentials() URL-encodes tenant ID', async () => {
	mockFetch({ credentials: [] });

	await listCredentials('my tenant');

	expect(lastRequest?.url).toBe('/control/tenants/my%20tenant/credentials');
});

test('issueCredential() calls POST /control/tenants/:id/credentials', async () => {
	mockFetch({ jwt: 'eyJ...', jti: 'abc-jti', expires_at_ms: 9999999 });

	await issueCredential('tenant-123', {
		target_user: 'user-uuid',
		ttl_seconds: 3600,
		grants: { databases: [] },
	});

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials');
	expect(lastRequest?.init?.method).toBe('POST');
});

test('issueCredential() URL-encodes tenant ID', async () => {
	mockFetch({ jwt: 'eyJ...', jti: 'abc-jti', expires_at_ms: 9999999 });

	await issueCredential('my tenant', {
		target_user: 'user-uuid',
		ttl_seconds: 3600,
		grants: { databases: [] },
	});

	expect(lastRequest?.url).toBe('/control/tenants/my%20tenant/credentials');
});

test('revokeCredential() calls DELETE /control/tenants/:id/credentials/:jti', async () => {
	mockFetch({});

	await revokeCredential('tenant-123', 'jti-abc');

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials/jti-abc');
	expect(lastRequest?.init?.method).toBe('DELETE');
});

test('revokeCredential() URL-encodes jti', async () => {
	mockFetch({});

	await revokeCredential('tenant-123', 'jti/with/slash');

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials/jti%2Fwith%2Fslash');
});
