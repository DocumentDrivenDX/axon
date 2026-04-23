import { afterEach, beforeEach, expect, test } from 'bun:test';

import {
	applyEntityRollback,
	commitMutationIntent,
	createCollection,
	createEntity,
	createLink,
	createTenant,
	createTenantDatabase,
	createUser,
	deleteCollectionTemplate,
	deleteEntity,
	deleteLink,
	deleteTenant,
	deleteTenantDatabase,
	dropCollection,
	fetchAudit,
	fetchCollection,
	fetchCollectionTemplate,
	fetchCollections,
	fetchEntities,
	fetchEntity,
	fetchEntityAudit,
	fetchRenderedEntity,
	fetchSchema,
	fetchTenant,
	fetchTenantDatabases,
	fetchTenantMembers,
	fetchTenants,
	fetchUsers,
	issueCredential,
	listCredentials,
	listUsers,
	previewEntityRollback,
	previewMutationIntent,
	previewSchemaChange,
	putCollectionTemplate,
	removeTenantMember,
	removeUserRole,
	revertAuditEntry,
	revokeCredential,
	setUserRole,
	suspendUser,
	transitionLifecycle,
	traverseLinks,
	updateEntity,
	updateSchema,
	upsertTenantMember,
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

test('control-plane helpers use the unscoped control GraphQL endpoint', async () => {
	mockFetch({ data: { tenants: [] } });

	await fetchTenants();

	expect(lastRequest?.url).toBe('/control/graphql');
	expect(lastRequest?.url).not.toContain('/tenants/');
	expect(lastRequest?.init?.method).toBe('POST');
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

test('previewMutationIntent() posts typed preview mutation input', async () => {
	mockFetch({
		data: {
			previewMutation: {
				decision: 'allow',
				intentToken: 'token.allowed',
				intent: {
					id: 'mint_1',
					tenantId: 'acme',
					databaseId: 'orders',
					subject: { user_id: 'ui' },
					schemaVersion: 1,
					policyVersion: 1,
					operation: {
						operationKind: 'update_entity',
						operationHash: 'sha256:abc',
						operation: { collection: 'tasks' },
					},
					operationHash: 'sha256:abc',
					preImages: [{ kind: 'entity', collection: 'tasks', id: 'task-1', version: 1 }],
					decision: 'allow',
					approvalState: 'none',
					approvalRoute: null,
					expiresAtNs: '1000000000',
					reviewSummary: { policy_explanation: ['allow: all-update'] },
				},
				canonicalOperation: {
					operationKind: 'update_entity',
					operationHash: 'sha256:abc',
					operation: { collection: 'tasks' },
				},
				diff: { title: { before: 'old', after: 'new' } },
				affectedRecords: [{ kind: 'entity', collection: 'tasks', id: 'task-1', version: 1 }],
				affectedFields: ['title'],
				approvalRoute: null,
				policyExplanation: ['allow: all-update'],
			},
		},
	});

	const result = await previewMutationIntent(
		{ tenant: 'acme', database: 'orders' },
		{
			operation: {
				operationKind: 'update_entity',
				operation: {
					collection: 'tasks',
					id: 'task-1',
					expected_version: 1,
					data: { title: 'new' },
				},
			},
			expiresInSeconds: 600,
		},
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('previewMutation');
	expect(body.variables.input.operation.operationKind).toBe('update_entity');
	expect(body.variables.input.operation.operation.expected_version).toBe(1);
	expect(result.intentToken).toBe('token.allowed');
	expect(result.affectedFields).toEqual(['title']);
});

test('commitMutationIntent() returns stale GraphQL errors without throwing', async () => {
	mockFetch({
		errors: [
			{
				message: 'mutation intent is stale',
				extensions: {
					code: 'intent_stale',
					stale: [
						{
							dimension: 'pre_image',
							expected: '1',
							actual: '2',
							path: 'entity:tasks/task-1',
						},
					],
				},
			},
		],
	});

	const outcome = await commitMutationIntent(
		{ tenant: 'acme', database: 'orders' },
		{ intentToken: 'token.allowed', intentId: 'mint_1' },
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('commitMutationIntent');
	expect(body.variables.input.intentId).toBe('mint_1');
	expect(outcome.ok).toBe(false);
	if (!outcome.ok) {
		expect(outcome.error.code).toBe('intent_stale');
		expect(outcome.error.stale).toHaveLength(1);
		expect(outcome.error.stale[0]?.dimension).toBe('pre_image');
	}
});

test('revertAuditEntry() uses tenant-scoped GraphQL mutation', async () => {
	mockFetch({
		data: {
			revertAuditEntry: {
				entity: {
					collection: 'tasks',
					id: 'task-1',
					version: 3,
					data: { title: 'old' },
				},
				auditEntry: {
					id: '9',
					timestampNs: '123',
					collection: 'tasks',
					entityId: 'task-1',
					version: 3,
					mutation: 'entity.revert',
					dataBefore: { title: 'new' },
					dataAfter: { title: 'old' },
					actor: 'ui',
					transactionId: null,
					metadata: { reverted_from_entry_id: '2' },
				},
			},
		},
	});

	const result = await revertAuditEntry(2, { tenant: 'acme', database: 'orders' });

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('revertAuditEntry');
	expect(body.variables).toEqual({ auditEntryId: '2' });
	expect(result).toEqual({
		entity: { collection: 'tasks', id: 'task-1', version: 3, data: { title: 'old' } },
		audit_entry_id: 9,
	});
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

test('traverseLinks() uses tenant-scoped GraphQL neighbors query', async () => {
	mockFetch({
		data: {
			neighbors: {
				groups: [
					{
						edges: [
							{
								node: {
									collection: 'tasks',
									id: 't1',
									version: 1,
									data: { title: 'one' },
								},
								linkType: 'owns',
								sourceCollection: 'users',
								sourceId: 'u1',
								targetCollection: 'tasks',
								targetId: 't1',
							},
						],
					},
				],
			},
		},
	});

	const result = await traverseLinks(
		'users',
		'u1',
		{ linkType: 'owns' },
		{ tenant: 'acme', database: 'orders' },
	);

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('neighbors(');
	expect(body.variables).toEqual({
		collection: 'users',
		id: 'u1',
		linkType: 'owns',
		direction: 'outbound',
	});
	expect(result).toEqual({
		entities: [{ collection: 'tasks', id: 't1', version: 1, data: { title: 'one' } }],
		paths: [
			{
				source_collection: 'users',
				source_id: 'u1',
				target_collection: 'tasks',
				target_id: 't1',
				link_type: 'owns',
			},
		],
	});
});

test('transitionLifecycle() uses typed GraphQL mutation and refreshes generic entity', async () => {
	mockFetchSequence([
		{
			data: {
				transitionTasksLifecycle: {
					id: 't1',
					version: 2,
					lifecycles: {},
				},
			},
		},
		{
			data: {
				entity: {
					collection: 'tasks',
					id: 't1',
					version: 2,
					data: { title: 'submitted', status: 'submitted' },
				},
			},
		},
	]);

	const result = await transitionLifecycle(
		'tasks',
		't1',
		{
			lifecycle_name: 'status',
			target_state: 'submitted',
			expected_version: 1,
		},
		{ tenant: 'acme', database: 'orders' },
	);

	expect(requests).toHaveLength(2);
	const transitionBody = JSON.parse(String(requests[0]?.init?.body));
	expect(requests[0]?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(transitionBody.query).toContain('transitionTasksLifecycle');
	expect(transitionBody.variables).toEqual({
		id: 't1',
		lifecycleName: 'status',
		targetState: 'submitted',
		expectedVersion: 1,
	});
	expect(result.entity).toEqual({
		collection: 'tasks',
		id: 't1',
		version: 2,
		data: { title: 'submitted', status: 'submitted' },
	});
});

test('entity rollback helpers use tenant-scoped GraphQL rollbackEntity mutation', async () => {
	mockFetchSequence([
		{
			data: {
				rollbackEntity: {
					current: {
						collection: 'tasks',
						id: 't1',
						version: 2,
						data: { title: 'v2' },
					},
					target: {
						collection: 'tasks',
						id: 't1',
						version: 1,
						data: { title: 'v1' },
					},
					diff: { title: { path: 'title', kind: 'modified', description: 'title changed' } },
				},
			},
		},
		{
			data: {
				rollbackEntity: {
					entity: {
						collection: 'tasks',
						id: 't1',
						version: 3,
						data: { title: 'v1' },
					},
					auditEntry: {
						id: '7',
						timestampNs: '123',
						collection: 'tasks',
						entityId: 't1',
						version: 3,
						mutation: 'entity.rollback',
						dataBefore: { title: 'v2' },
						dataAfter: { title: 'v1' },
						actor: 'ui',
						transactionId: null,
						metadata: null,
					},
				},
			},
		},
	]);

	await expect(
		previewEntityRollback('tasks', 't1', 1, { tenant: 'acme', database: 'orders' }),
	).resolves.toEqual({
		current: { collection: 'tasks', id: 't1', version: 2, data: { title: 'v2' } },
		target: { collection: 'tasks', id: 't1', version: 1, data: { title: 'v1' } },
		diff: { title: { path: 'title', kind: 'modified', description: 'title changed' } },
	});
	await expect(
		applyEntityRollback('tasks', 't1', 1, 2, { tenant: 'acme', database: 'orders' }),
	).resolves.toEqual({
		entity: { collection: 'tasks', id: 't1', version: 3, data: { title: 'v1' } },
		audit_entry: {
			id: 7,
			timestamp_ns: 123,
			collection: 'tasks',
			entity_id: 't1',
			version: 3,
			mutation: 'entity.rollback',
			data_before: { title: 'v2' },
			data_after: { title: 'v1' },
			actor: 'ui',
			transaction_id: null,
			metadata: null,
		},
	});

	expect(JSON.parse(String(requests[0]?.init?.body)).variables).toEqual({
		collection: 'tasks',
		id: 't1',
		toVersion: 1,
	});
	expect(JSON.parse(String(requests[1]?.init?.body)).variables).toEqual({
		collection: 'tasks',
		id: 't1',
		toVersion: 1,
		expectedVersion: 2,
	});
});

test('collection template helpers use tenant-scoped GraphQL', async () => {
	mockFetchSequence([
		{
			data: {
				collectionTemplate: {
					collection: 'tasks',
					template: '# {{title}}',
					version: 1,
					updatedAtNs: '123',
					updatedBy: 'admin',
					warnings: [],
				},
			},
		},
		{
			data: {
				putCollectionTemplate: {
					collection: 'tasks',
					template: '## {{title}}',
					version: 2,
					updatedAtNs: '456',
					updatedBy: 'admin',
					warnings: ['optional field notes may be absent'],
				},
			},
		},
		{ data: { deleteCollectionTemplate: { deleted: true } } },
	]);

	await expect(
		fetchCollectionTemplate('tasks', { tenant: 'acme', database: 'orders' }),
	).resolves.toEqual({
		collection: 'tasks',
		template: '# {{title}}',
		version: 1,
		updated_at_ns: 123,
		updated_by: 'admin',
	});
	await expect(
		putCollectionTemplate('tasks', '## {{title}}', { tenant: 'acme', database: 'orders' }),
	).resolves.toEqual({
		collection: 'tasks',
		template: '## {{title}}',
		version: 2,
		updated_at_ns: 456,
		updated_by: 'admin',
		warnings: ['optional field notes may be absent'],
	});
	await deleteCollectionTemplate('tasks', { tenant: 'acme', database: 'orders' });

	expect(requests.map((request) => request.url)).toEqual([
		'/tenants/acme/databases/orders/graphql',
		'/tenants/acme/databases/orders/graphql',
		'/tenants/acme/databases/orders/graphql',
	]);
	expect(JSON.parse(String(requests[0]?.init?.body)).query).toContain('collectionTemplate');
	expect(JSON.parse(String(requests[1]?.init?.body)).query).toContain('putCollectionTemplate');
	expect(JSON.parse(String(requests[2]?.init?.body)).query).toContain('deleteCollectionTemplate');
	expect(JSON.parse(String(requests[1]?.init?.body)).variables).toEqual({
		collection: 'tasks',
		template: '## {{title}}',
	});
});

test('fetchRenderedEntity() uses tenant-scoped GraphQL renderedEntity query', async () => {
	mockFetch({
		data: {
			renderedEntity: {
				markdown: '# Hello',
				entity: {
					collection: 'tasks',
					id: 't1',
					version: 1,
					data: { title: 'Hello' },
				},
			},
		},
	});

	await expect(
		fetchRenderedEntity('tasks', 't1', { tenant: 'acme', database: 'orders' }),
	).resolves.toBe('# Hello');

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/graphql');
	expect(body.query).toContain('renderedEntity');
	expect(body.variables).toEqual({ collection: 'tasks', id: 't1' });
});

// ── Control-plane GraphQL helpers ────────────────────────────────────────────

test('fetchTenants() maps control GraphQL tenant fields', async () => {
	mockFetch({
		data: {
			tenants: [{ id: 't1', name: 'Acme', dbName: 'acme-a1', createdAt: '2026-04-20T00:00:00Z' }],
		},
	});

	const result = await fetchTenants();

	const body = JSON.parse(String(lastRequest?.init?.body));
	expect(lastRequest?.url).toBe('/control/graphql');
	expect(body.query).toContain('tenants');
	expect(result).toEqual([
		{ id: 't1', name: 'Acme', db_name: 'acme-a1', created_at: '2026-04-20T00:00:00Z' },
	]);
});

test('createTenant(), fetchTenant(), and deleteTenant() use control GraphQL', async () => {
	mockFetchSequence([
		{
			data: {
				createTenant: {
					id: 't1',
					name: 'Acme',
					dbName: 'acme-a1',
					createdAt: '2026-04-20T00:00:00Z',
				},
			},
		},
		{
			data: {
				tenant: {
					id: 't1',
					name: 'Acme',
					dbName: 'acme-a1',
					createdAt: '2026-04-20T00:00:00Z',
				},
			},
		},
		{ data: { deleteTenant: { deleted: true } } },
	]);

	await expect(createTenant('Acme')).resolves.toEqual({
		id: 't1',
		name: 'Acme',
		db_name: 'acme-a1',
		created_at: '2026-04-20T00:00:00Z',
	});
	await expect(fetchTenant('t1')).resolves.toEqual({
		id: 't1',
		name: 'Acme',
		db_name: 'acme-a1',
		created_at: '2026-04-20T00:00:00Z',
	});
	await deleteTenant('t1');

	expect(requests.map((request) => request.url)).toEqual([
		'/control/graphql',
		'/control/graphql',
		'/control/graphql',
	]);
	expect(JSON.parse(String(requests[0]?.init?.body)).variables).toEqual({ name: 'Acme' });
	expect(JSON.parse(String(requests[1]?.init?.body)).variables).toEqual({ id: 't1' });
	expect(JSON.parse(String(requests[2]?.init?.body)).variables).toEqual({ id: 't1' });
});

test('tenant database helpers use control GraphQL variables without path encoding', async () => {
	mockFetchSequence([
		{
			data: {
				tenantDatabases: [{ tenantId: 'tenant/one', name: 'orders', createdAtMs: 1000 }],
			},
		},
		{ data: { createTenantDatabase: { tenantId: 'tenant/one', name: 'ops', createdAtMs: 2000 } } },
		{ data: { deleteTenantDatabase: { deleted: true } } },
	]);

	await expect(fetchTenantDatabases('tenant/one')).resolves.toEqual([
		{ tenant_id: 'tenant/one', name: 'orders', created_at_ms: 1000 },
	]);
	await expect(createTenantDatabase('tenant/one', 'ops')).resolves.toEqual({
		tenant_id: 'tenant/one',
		name: 'ops',
		created_at_ms: 2000,
	});
	await deleteTenantDatabase('tenant/one', 'ops');

	expect(JSON.parse(String(requests[0]?.init?.body)).variables).toEqual({ tenantId: 'tenant/one' });
	expect(JSON.parse(String(requests[1]?.init?.body)).variables).toEqual({
		tenantId: 'tenant/one',
		name: 'ops',
	});
	expect(JSON.parse(String(requests[2]?.init?.body)).variables).toEqual({
		tenantId: 'tenant/one',
		name: 'ops',
	});
});

test('provisioned user helpers use control GraphQL and map field names', async () => {
	mockFetchSequence([
		{
			data: {
				provisionUser: {
					id: 'u1',
					displayName: 'Ada',
					email: 'ada@example.com',
					createdAtMs: 123,
					suspendedAtMs: null,
				},
			},
		},
		{
			data: {
				users: [
					{
						id: 'u1',
						displayName: 'Ada',
						email: 'ada@example.com',
						createdAtMs: 123,
						suspendedAtMs: 456,
					},
				],
			},
		},
		{ data: { suspendUser: { suspended: true } } },
	]);

	await expect(createUser('Ada', 'ada@example.com')).resolves.toEqual({
		id: 'u1',
		display_name: 'Ada',
		email: 'ada@example.com',
		created_at_ms: 123,
		suspended_at_ms: null,
	});
	await expect(listUsers()).resolves.toEqual([
		{
			id: 'u1',
			display_name: 'Ada',
			email: 'ada@example.com',
			created_at_ms: 123,
			suspended_at_ms: 456,
		},
	]);
	await suspendUser('u1');

	expect(requests.map((request) => request.url)).toEqual([
		'/control/graphql',
		'/control/graphql',
		'/control/graphql',
	]);
});

test('deployment role helpers use control GraphQL', async () => {
	mockFetchSequence([
		{ data: { userRoles: [{ login: 'admin@example.com', role: 'admin' }] } },
		{ data: { setUserRole: { login: 'writer@example.com', role: 'write' } } },
		{ data: { removeUserRole: { deleted: true } } },
	]);

	await expect(fetchUsers()).resolves.toEqual([{ login: 'admin@example.com', role: 'admin' }]);
	await expect(setUserRole('writer@example.com', 'write')).resolves.toEqual({
		login: 'writer@example.com',
		role: 'write',
	});
	await removeUserRole('writer@example.com');

	expect(JSON.parse(String(requests[1]?.init?.body)).variables).toEqual({
		login: 'writer@example.com',
		role: 'write',
	});
	expect(JSON.parse(String(requests[2]?.init?.body)).variables).toEqual({
		login: 'writer@example.com',
	});
});

test('tenant member helpers use control GraphQL and map field names', async () => {
	mockFetchSequence([
		{ data: { tenantMembers: [{ tenantId: 't1', userId: 'u1', role: 'read' }] } },
		{ data: { upsertTenantMember: { tenantId: 't1', userId: 'u2', role: 'write' } } },
		{ data: { removeTenantMember: { deleted: true } } },
	]);

	await expect(fetchTenantMembers('t1')).resolves.toEqual([
		{ tenant_id: 't1', user_id: 'u1', role: 'read' },
	]);
	await expect(upsertTenantMember('t1', 'u2', 'write')).resolves.toEqual({
		tenant_id: 't1',
		user_id: 'u2',
		role: 'write',
	});
	await removeTenantMember('t1', 'u2');

	expect(JSON.parse(String(requests[0]?.init?.body)).variables).toEqual({ tenantId: 't1' });
	expect(JSON.parse(String(requests[1]?.init?.body)).variables).toEqual({
		tenantId: 't1',
		userId: 'u2',
		role: 'write',
	});
	expect(JSON.parse(String(requests[2]?.init?.body)).variables).toEqual({
		tenantId: 't1',
		userId: 'u2',
	});
});

test('credential helpers use control GraphQL without exposing JWT in list', async () => {
	mockFetchSequence([
		{
			data: {
				credentials: [
					{
						jti: 'jti-1',
						userId: 'u1',
						tenantId: 't1',
						issuedAtMs: 100,
						expiresAtMs: 200,
						revoked: false,
						grants: { databases: [{ name: 'orders', ops: ['read'] }] },
					},
				],
			},
		},
		{ data: { issueCredential: { jwt: 'eyJ...', jti: 'jti-2', expiresAt: 300 } } },
		{ data: { revokeCredential: { revoked: true } } },
	]);

	await expect(listCredentials('t1')).resolves.toEqual([
		{
			jti: 'jti-1',
			user_id: 'u1',
			tenant_id: 't1',
			issued_at_ms: 100,
			expires_at_ms: 200,
			revoked: false,
			grants: { databases: [{ name: 'orders', ops: ['read'] }] },
		},
	]);
	await expect(
		issueCredential('t1', {
			target_user: 'u1',
			ttl_seconds: 3600,
			grants: { databases: [{ name: 'orders', ops: ['read'] }] },
		}),
	).resolves.toEqual({ jwt: 'eyJ...', jti: 'jti-2', expires_at_ms: 300000 });
	await revokeCredential('t1', 'jti-2');

	expect(JSON.parse(String(requests[0]?.init?.body)).query).not.toContain('jwt');
	expect(JSON.parse(String(requests[1]?.init?.body)).variables).toEqual({
		tenantId: 't1',
		targetUser: 'u1',
		ttlSeconds: 3600,
		grants: { databases: [{ name: 'orders', ops: ['read'] }] },
	});
	expect(JSON.parse(String(requests[2]?.init?.body)).variables).toEqual({
		tenantId: 't1',
		jti: 'jti-2',
	});
});
