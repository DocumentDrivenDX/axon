import { afterEach, beforeEach, expect, test } from 'bun:test';

import {
	__resetGraphQLHelperContractForTests,
	fetchCollections,
	fetchEntities,
	fetchEntity,
	gqlQuery,
} from './graphql';

const originalFetch = globalThis.fetch;

type GraphQLRequest = {
	query: string;
	variables?: Record<string, unknown>;
};

type GraphQLTypeRef = {
	kind: string;
	name: string | null;
	ofType?: GraphQLTypeRef | null;
};

function createJsonResponse(body: unknown): Response {
	return new Response(JSON.stringify(body), {
		status: 200,
		headers: { 'Content-Type': 'application/json' },
	});
}

function parseRequest(init?: RequestInit): GraphQLRequest {
	if (typeof init?.body !== 'string') {
		throw new Error('expected GraphQL request body to be a JSON string');
	}

	return JSON.parse(init.body) as GraphQLRequest;
}

function namedType(name: string): GraphQLTypeRef {
	return { kind: 'OBJECT', name };
}

function scalarType(name: string): GraphQLTypeRef {
	return { kind: 'SCALAR', name };
}

function nonNullType(ofType: GraphQLTypeRef): GraphQLTypeRef {
	return { kind: 'NON_NULL', name: null, ofType };
}

function listType(ofType: GraphQLTypeRef): GraphQLTypeRef {
	return { kind: 'LIST', name: null, ofType };
}

function supportedCollectionContract() {
	return {
		__schema: {
			queryType: {
				fields: [
					{
						name: 'collections',
						args: [],
						type: nonNullType(listType(nonNullType(namedType('CollectionMeta')))),
					},
					{
						name: 'entity',
						args: [
							{ name: 'collection', type: nonNullType(scalarType('String')) },
							{ name: 'id', type: nonNullType(scalarType('ID')) },
						],
						type: scalarType('JSON'),
					},
					{
						name: 'entities',
						args: [
							{ name: 'collection', type: nonNullType(scalarType('String')) },
							{ name: 'limit', type: scalarType('Int') },
							{ name: 'after', type: scalarType('String') },
						],
						type: nonNullType(namedType('EntityConnection')),
					},
				],
			},
		},
		collectionMeta: {
			name: 'CollectionMeta',
			fields: [
				{ name: 'name', type: nonNullType(scalarType('String')) },
				{ name: 'entityCount', type: nonNullType(scalarType('Int')) },
			],
		},
		entityConnection: {
			name: 'EntityConnection',
			fields: [
				{
					name: 'edges',
					type: nonNullType(listType(nonNullType(namedType('EntityEdge')))),
				},
				{ name: 'pageInfo', type: nonNullType(namedType('PageInfo')) },
			],
		},
		entityEdge: {
			name: 'EntityEdge',
			fields: [
				{ name: 'node', type: scalarType('JSON') },
				{ name: 'cursor', type: nonNullType(scalarType('String')) },
			],
		},
		pageInfo: {
			name: 'PageInfo',
			fields: [
				{ name: 'hasNextPage', type: nonNullType(scalarType('Boolean')) },
				{ name: 'endCursor', type: scalarType('String') },
			],
		},
	};
}

function supportedEntityHelperContract() {
	return {
		__schema: {
			queryType: {
				fields: [
					{
						name: 'collections',
						args: [],
						type: nonNullType(listType(nonNullType(namedType('CollectionMeta')))),
					},
					{
						name: 'entity',
						args: [
							{ name: 'collection', type: nonNullType(scalarType('String')) },
							{ name: 'id', type: nonNullType(scalarType('ID')) },
						],
						type: namedType('EntityRecord'),
					},
					{
						name: 'entities',
						args: [
							{ name: 'collection', type: nonNullType(scalarType('String')) },
							{ name: 'limit', type: scalarType('Int') },
							{ name: 'after', type: scalarType('String') },
						],
						type: nonNullType(namedType('EntityConnection')),
					},
				],
			},
		},
		collectionMeta: {
			name: 'CollectionMeta',
			fields: [
				{ name: 'name', type: nonNullType(scalarType('String')) },
				{ name: 'entityCount', type: nonNullType(scalarType('Int')) },
			],
		},
		entityRecord: {
			name: 'EntityRecord',
			fields: [
				{ name: 'id', type: nonNullType(scalarType('ID')) },
				{ name: 'version', type: nonNullType(scalarType('Int')) },
				{ name: 'data', type: nonNullType(scalarType('JSON')) },
				{ name: 'createdAt', type: nonNullType(scalarType('DateTime')) },
				{ name: 'updatedAt', type: nonNullType(scalarType('DateTime')) },
			],
		},
		entityConnection: {
			name: 'EntityConnection',
			fields: [
				{
					name: 'edges',
					type: nonNullType(listType(nonNullType(namedType('EntityEdge')))),
				},
				{ name: 'pageInfo', type: nonNullType(namedType('PageInfo')) },
			],
		},
		entityEdge: {
			name: 'EntityEdge',
			fields: [
				{ name: 'node', type: nonNullType(namedType('EntityRecord')) },
				{ name: 'cursor', type: nonNullType(scalarType('String')) },
			],
		},
		pageInfo: {
			name: 'PageInfo',
			fields: [
				{ name: 'hasNextPage', type: nonNullType(scalarType('Boolean')) },
				{ name: 'endCursor', type: scalarType('String') },
			],
		},
	};
}

function supportedCustomEntityHelperContract() {
	return {
		__schema: {
			queryType: {
				fields: [
					{
						name: 'collections',
						args: [],
						type: nonNullType(listType(nonNullType(namedType('CollectionMeta')))),
					},
					{
						name: 'entity',
						args: [
							{ name: 'collection', type: nonNullType(scalarType('String')) },
							{ name: 'id', type: nonNullType(scalarType('ID')) },
						],
						type: namedType('TaskRecord'),
					},
					{
						name: 'entities',
						args: [
							{ name: 'collection', type: nonNullType(scalarType('String')) },
							{ name: 'limit', type: scalarType('Int') },
							{ name: 'after', type: scalarType('String') },
						],
						type: nonNullType(namedType('TaskConnection')),
					},
				],
			},
		},
		collectionMeta: {
			name: 'CollectionMeta',
			fields: [
				{ name: 'name', type: nonNullType(scalarType('String')) },
				{ name: 'entityCount', type: nonNullType(scalarType('Int')) },
			],
		},
		entityRecord: null,
		entityConnection: null,
		entityEdge: null,
		pageInfo: {
			name: 'PageInfo',
			fields: [
				{ name: 'hasNextPage', type: nonNullType(scalarType('Boolean')) },
				{ name: 'endCursor', type: scalarType('String') },
			],
		},
	};
}

function namedTypeResponse(name: string) {
	switch (name) {
		case 'TaskRecord':
			return {
				name: 'TaskRecord',
				fields: [
					{ name: 'id', type: nonNullType(scalarType('ID')) },
					{ name: 'version', type: nonNullType(scalarType('Int')) },
					{ name: 'data', type: nonNullType(scalarType('JSON')) },
					{ name: 'createdAt', type: nonNullType(scalarType('DateTime')) },
					{ name: 'updatedAt', type: nonNullType(scalarType('DateTime')) },
				],
			};
		case 'TaskConnection':
			return {
				name: 'TaskConnection',
				fields: [
					{
						name: 'edges',
						type: nonNullType(listType(nonNullType(namedType('TaskEdge')))),
					},
					{ name: 'pageInfo', type: nonNullType(namedType('PageInfo')) },
				],
			};
		case 'TaskEdge':
			return {
				name: 'TaskEdge',
				fields: [
					{ name: 'node', type: nonNullType(namedType('TaskRecord')) },
					{ name: 'cursor', type: nonNullType(scalarType('String')) },
				],
			};
		default:
			return null;
	}
}

beforeEach(() => {
	__resetGraphQLHelperContractForTests();
});

afterEach(() => {
	globalThis.fetch = originalFetch;
	__resetGraphQLHelperContractForTests();
});

test('gqlQuery returns parsed GraphQL data', async () => {
	globalThis.fetch = (async () =>
		createJsonResponse({
			data: {
				ok: true,
			},
		})) as unknown as typeof fetch;

	await expect(gqlQuery<{ ok: boolean }>('{ ok }')).resolves.toEqual({ ok: true });
});

test('fetchCollections fails fast when the backend schema lacks the helper contract', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		return createJsonResponse({
			data: {
				__schema: {
					queryType: {
						fields: [{ name: 'tasks' }],
					},
					types: [],
				},
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchCollections()).rejects.toThrow(
		/does not expose the collections helper contract/i,
	);
	expect(requests).toHaveLength(1);
	expect(requests[0]?.query).toContain('__schema');
});

test('fetchCollections uses the collections query once the backend advertises support', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		if (requests.length === 1) {
			return createJsonResponse({ data: supportedCollectionContract() });
		}

		return createJsonResponse({
			data: {
				collections: [{ name: 'tasks', entityCount: 3 }],
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchCollections()).resolves.toEqual([{ name: 'tasks', entityCount: 3 }]);
	expect(requests).toHaveLength(2);
	expect(requests[0]?.query).not.toContain('types {');
	expect(requests[0]?.query).toContain('collectionMeta: __type(name: "CollectionMeta")');
	expect(requests[0]?.query).toContain('entityRecord: __type(name: "EntityRecord")');
	expect(requests[0]?.query).toContain('entityConnection: __type(name: "EntityConnection")');
	expect(requests[0]?.query).toContain('entityEdge: __type(name: "EntityEdge")');
	expect(requests[0]?.query).toContain('pageInfo: __type(name: "PageInfo")');
	expect(requests[1]?.query).toContain('collections { name entityCount }');
});

test('fetchCollections retries the helper contract probe after a transient failure', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		if (requests.length === 1) {
			throw new Error('transient network error');
		}

		if (requests.length === 2) {
			return createJsonResponse({ data: supportedCollectionContract() });
		}

		return createJsonResponse({
			data: {
				collections: [{ name: 'tasks', entityCount: 3 }],
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchCollections()).rejects.toThrow(/transient network error/i);
	await expect(fetchCollections()).resolves.toEqual([{ name: 'tasks', entityCount: 3 }]);
	expect(requests).toHaveLength(3);
	expect(requests[0]?.query).toContain('__schema');
	expect(requests[1]?.query).toContain('__schema');
	expect(requests[2]?.query).toContain('collections { name entityCount }');
});

test('fetchEntities fails fast when the backend only exposes the FEAT-015 generic contract', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		return createJsonResponse({ data: supportedCollectionContract() });
	}) as unknown as typeof fetch;

	await expect(fetchEntities('tasks', { limit: 10 })).rejects.toThrow(
		/does not expose the entity helper contract/i,
	);
	expect(requests).toHaveLength(1);
	expect(requests[0]?.query).toContain('__schema');
});

test('fetchEntities uses the entities query once the backend advertises support', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		if (requests.length === 1) {
			return createJsonResponse({ data: supportedEntityHelperContract() });
		}

		return createJsonResponse({
			data: {
				entities: {
					edges: [{ node: { id: 'task-1', version: 2, data: { title: 'Ship it' } } }],
					pageInfo: { hasNextPage: false, endCursor: null },
				},
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchEntities('tasks', { limit: 10 })).resolves.toEqual({
		edges: [{ node: { id: 'task-1', version: 2, data: { title: 'Ship it' } } }],
		pageInfo: { hasNextPage: false, endCursor: null },
	});
	expect(requests).toHaveLength(2);
	expect(requests[1]?.query).toContain('query($collection: String!, $limit: Int, $after: String)');
	expect(requests[1]?.query).toContain('entities(collection: $collection');
	expect(requests[1]?.variables).toEqual({ collection: 'tasks', limit: 10, after: null });
});

test('fetchEntity fails fast when the backend only exposes the FEAT-015 generic contract', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		return createJsonResponse({ data: supportedCollectionContract() });
	}) as unknown as typeof fetch;

	await expect(fetchEntity('tasks', 'task-1')).rejects.toThrow(
		/does not expose the entity helper contract/i,
	);
	expect(requests).toHaveLength(1);
	expect(requests[0]?.query).toContain('__schema');
});

test('fetchEntity uses the entity query once the backend advertises support', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		if (requests.length === 1) {
			return createJsonResponse({ data: supportedEntityHelperContract() });
		}

		return createJsonResponse({
			data: {
				entity: {
					id: 'task-1',
					version: 2,
					data: { title: 'Ship it' },
					createdAt: '2026-04-08T00:00:00Z',
					updatedAt: '2026-04-08T00:00:00Z',
				},
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchEntity('tasks', 'task-1')).resolves.toEqual({
		id: 'task-1',
		version: 2,
		data: { title: 'Ship it' },
		createdAt: '2026-04-08T00:00:00Z',
		updatedAt: '2026-04-08T00:00:00Z',
	});
	expect(requests).toHaveLength(2);
	expect(requests[1]?.query).toContain('entity(collection: $collection, id: $id)');
	expect(requests[1]?.variables).toEqual({ collection: 'tasks', id: 'task-1' });
});

test('fetchEntity and fetchEntities accept shape-compatible helper types with custom names', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		if (requests.length === 1) {
			return createJsonResponse({ data: supportedCustomEntityHelperContract() });
		}

		if (request.query.includes('namedType: __type(name: $name)')) {
			return createJsonResponse({
				data: {
					namedType: namedTypeResponse(String(request.variables?.name)),
				},
			});
		}

		if (request.query.includes('entity(collection: $collection, id: $id)')) {
			return createJsonResponse({
				data: {
					entity: {
						id: 'task-1',
						version: 2,
						data: { title: 'Ship it' },
						createdAt: '2026-04-08T00:00:00Z',
						updatedAt: '2026-04-08T00:00:00Z',
					},
				},
			});
		}

		return createJsonResponse({
			data: {
				entities: {
					edges: [{ node: { id: 'task-1', version: 2, data: { title: 'Ship it' } } }],
					pageInfo: { hasNextPage: false, endCursor: null },
				},
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchEntity('tasks', 'task-1')).resolves.toEqual({
		id: 'task-1',
		version: 2,
		data: { title: 'Ship it' },
		createdAt: '2026-04-08T00:00:00Z',
		updatedAt: '2026-04-08T00:00:00Z',
	});
	await expect(fetchEntities('tasks', { limit: 10 })).resolves.toEqual({
		edges: [{ node: { id: 'task-1', version: 2, data: { title: 'Ship it' } } }],
		pageInfo: { hasNextPage: false, endCursor: null },
	});

	expect(requests[0]?.query).not.toContain('types {');
	expect(
		requests
			.filter((request) => request.query.includes('namedType: __type(name: $name)'))
			.map((request) => request.variables?.name)
			.sort(),
	).toEqual(['TaskConnection', 'TaskEdge', 'TaskRecord']);
	expect(requests.at(-1)?.variables).toEqual({ collection: 'tasks', limit: 10, after: null });
});
