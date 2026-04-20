import { afterEach, beforeEach, expect, test } from 'bun:test';

import {
	createCollection,
	createEntity,
	createLink,
	deleteEntity,
	deleteLink,
	dropCollection,
	fetchAudit,
	fetchCollection,
	fetchCollections,
	fetchEntities,
	fetchEntity,
	fetchEntityAudit,
	fetchSchema,
	issueCredential,
	listCredentials,
	previewSchemaChange,
	revokeCredential,
	updateEntity,
	updateSchema,
} from './api';

const originalFetch = globalThis.fetch;

type CapturedRequest = {
	url: string;
	init: RequestInit | undefined;
};

let lastRequest: CapturedRequest | null = null;
let requests: CapturedRequest[] = [];

function mockFetch(body: unknown, status = 200) {
	const handler = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
		lastRequest = { url: String(input), init };
		requests.push(lastRequest);
		return new Response(JSON.stringify(body), {
			status,
			headers: { 'Content-Type': 'application/json' },
		});
	};
	globalThis.fetch = handler as unknown as typeof globalThis.fetch;
}

function mockFetchSequence(bodies: unknown[]) {
	let index = 0;
	const handler = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
		lastRequest = { url: String(input), init };
		requests.push(lastRequest);
		const body = bodies[index] ?? bodies[bodies.length - 1];
		index += 1;
		return new Response(JSON.stringify(body), {
			status: 200,
			headers: { 'Content-Type': 'application/json' },
		});
	};
	globalThis.fetch = handler as unknown as typeof globalThis.fetch;
}

beforeEach(() => {
	lastRequest = null;
	requests = [];
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

test('fetchAudit() uses tenant-scoped GraphQL auditLog query', async () => {
	mockFetch({
		data: {
			auditLog: {
				edges: [
					{
						cursor: '42',
						node: {
							id: '42',
							timestampNs: '123456',
							collection: 'tasks',
							entityId: 'task-1',
							version: 2,
							mutation: 'entity.update',
							dataBefore: { title: 'old' },
							dataAfter: { title: 'new' },
							actor: 'alice',
							transactionId: 7,
							metadata: { source: 'test' },
						},
					},
				],
				pageInfo: { hasNextPage: true, endCursor: '42' },
			},
		},
	});

	const result = await fetchAudit(
		{ collection: 'tasks', actor: 'alice', sinceNs: '1', untilNs: '999' },
		{ tenant: 'acme', database: 'orders' },
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('auditLog');
	expect(body.variables).toEqual({
		collection: 'tasks',
		actor: 'alice',
		sinceNs: '1',
		untilNs: '999',
	});
	expect(result).toEqual({
		entries: [
			{
				id: 42,
				timestamp_ns: 123456,
				collection: 'tasks',
				entity_id: 'task-1',
				version: 2,
				mutation: 'entity.update',
				data_before: { title: 'old' },
				data_after: { title: 'new' },
				actor: 'alice',
				transaction_id: 7,
				metadata: { source: 'test' },
			},
		],
		next_cursor: 42,
	});
});

test('fetchEntityAudit() uses tenant-scoped GraphQL auditLog entity filter', async () => {
	mockFetch({
		data: {
			auditLog: {
				edges: [],
				pageInfo: { hasNextPage: false, endCursor: null },
			},
		},
	});

	const result = await fetchEntityAudit('tasks', 'task-1', {
		tenant: 'acme',
		database: 'orders',
	});

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('entityId');
	expect(body.variables).toEqual({ collection: 'tasks', entityId: 'task-1' });
	expect(result).toEqual({ entries: [], next_cursor: null });
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

test('createCollection() uses tenant-scoped GraphQL createCollection mutation', async () => {
	mockFetch({
		data: {
			createCollection: {
				name: 'tasks',
				entityCount: 0,
				schemaVersion: 1,
				schema: { collection: 'tasks', version: 1 },
			},
		},
	});

	await createCollection(
		'tasks',
		{
			version: 1,
			entity_schema: { type: 'object' },
			link_types: {},
		},
		{ tenant: 'acme', database: 'orders' },
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('createCollection');
	expect(body.variables).toEqual({
		name: 'tasks',
		schema: { version: 1, entity_schema: { type: 'object' }, link_types: {} },
	});
});

test('updateSchema() uses tenant-scoped GraphQL putSchema mutation', async () => {
	const schema = { collection: 'tasks', version: 2, entity_schema: { type: 'object' } };
	mockFetch({
		data: {
			putSchema: {
				schema,
				compatibility: 'compatible',
				diff: null,
				dryRun: false,
			},
		},
	});

	const result = await updateSchema(
		'tasks',
		schema,
		{ force: true },
		{
			tenant: 'acme',
			database: 'orders',
		},
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('putSchema');
	expect(body.variables).toEqual({ collection: 'tasks', schema, force: true });
	expect(result).toEqual(schema);
});

test('previewSchemaChange() uses tenant-scoped GraphQL dry-run putSchema mutation', async () => {
	const schema = { collection: 'tasks', version: 2, entity_schema: { type: 'object' } };
	mockFetch({
		data: {
			putSchema: {
				schema,
				compatibility: 'metadata_only',
				diff: { compatibility: 'metadata_only', changes: [] },
				dryRun: true,
			},
		},
	});

	const result = await previewSchemaChange('tasks', schema, { tenant: 'acme', database: 'orders' });

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('dryRun: true');
	expect(body.variables).toEqual({ collection: 'tasks', schema });
	expect(result).toEqual({
		schema,
		compatibility: 'metadata_only',
		diff: { compatibility: 'metadata_only', changes: [] },
		dry_run: true,
	});
});

test('dropCollection() uses tenant-scoped GraphQL dropCollection mutation', async () => {
	mockFetch({
		data: {
			dropCollection: { name: 'tasks', entitiesRemoved: 2 },
		},
	});

	await dropCollection('tasks', { tenant: 'acme', database: 'orders' });

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('dropCollection');
	expect(body.variables).toEqual({ name: 'tasks' });
});

test('createEntity() uses tenant-scoped GraphQL commitTransaction mutation', async () => {
	mockFetch({
		data: {
			commitTransaction: {
				results: [
					{
						index: 0,
						success: true,
						collection: 'tasks',
						id: 'task-1',
						entity: {
							collection: 'tasks',
							id: 'task-1',
							version: 1,
							data: { title: 'one' },
						},
					},
				],
			},
		},
	});

	const result = await createEntity(
		'tasks',
		'task-1',
		{ title: 'one' },
		{ tenant: 'acme', database: 'orders' },
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('commitTransaction');
	expect(body.variables).toEqual({
		operations: [{ createEntity: { collection: 'tasks', id: 'task-1', data: { title: 'one' } } }],
	});
	expect(result).toEqual({
		collection: 'tasks',
		id: 'task-1',
		version: 1,
		data: { title: 'one' },
	});
});

test('updateEntity() uses tenant-scoped GraphQL commitTransaction mutation', async () => {
	mockFetch({
		data: {
			commitTransaction: {
				results: [
					{
						index: 0,
						success: true,
						collection: 'tasks',
						id: 'task-1',
						entity: {
							collection: 'tasks',
							id: 'task-1',
							version: 2,
							data: { title: 'two' },
						},
					},
				],
			},
		},
	});

	const result = await updateEntity('tasks', 'task-1', { title: 'two' }, 1, {
		tenant: 'acme',
		database: 'orders',
	});

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.variables).toEqual({
		operations: [
			{
				updateEntity: {
					collection: 'tasks',
					id: 'task-1',
					expectedVersion: 1,
					data: { title: 'two' },
				},
			},
		],
	});
	expect(result.version).toBe(2);
});

test('deleteEntity() reads version then deletes through tenant-scoped GraphQL commitTransaction', async () => {
	mockFetchSequence([
		{
			data: {
				entity: {
					collection: 'tasks',
					id: 'task-1',
					version: 3,
					data: { title: 'old' },
				},
			},
		},
		{
			data: {
				commitTransaction: {
					results: [
						{
							index: 0,
							success: true,
							collection: 'tasks',
							id: 'task-1',
							entity: null,
						},
					],
				},
			},
		},
	]);

	await deleteEntity('tasks', 'task-1', { tenant: 'acme', database: 'orders' });

	expect(requests).toHaveLength(2);
	const getBody = JSON.parse(String(requests[0]?.init?.body));
	const deleteBody = JSON.parse(String(requests[1]?.init?.body));
	expect(requests[0]?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(getBody.query).toContain('entity(collection: $collection');
	expect(requests[1]?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(deleteBody.variables).toEqual({
		operations: [
			{
				deleteEntity: {
					collection: 'tasks',
					id: 'task-1',
					expectedVersion: 3,
				},
			},
		],
	});
});

test('createLink() uses tenant-scoped GraphQL createLink mutation', async () => {
	mockFetch({ data: { createLink: true } });

	const link = {
		source_collection: 'users',
		source_id: 'u1',
		target_collection: 'tasks',
		target_id: 't1',
		link_type: 'assigned-to',
		metadata: { role: 'owner' },
	};
	const result = await createLink(link, { tenant: 'acme', database: 'orders' });

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('createLink');
	expect(body.variables).toEqual({
		sourceCollection: 'users',
		sourceId: 'u1',
		targetCollection: 'tasks',
		targetId: 't1',
		linkType: 'assigned-to',
		metadata: JSON.stringify({ role: 'owner' }),
	});
	expect(result).toEqual(link);
});

test('deleteLink() uses tenant-scoped GraphQL deleteLink mutation', async () => {
	mockFetch({ data: { deleteLink: true } });

	await deleteLink(
		{
			source_collection: 'users',
			source_id: 'u1',
			target_collection: 'tasks',
			target_id: 't1',
			link_type: 'assigned-to',
		},
		{ tenant: 'acme', database: 'orders' },
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('deleteLink');
	expect(body.variables).toEqual({
		sourceCollection: 'users',
		sourceId: 'u1',
		targetCollection: 'tasks',
		targetId: 't1',
		linkType: 'assigned-to',
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
