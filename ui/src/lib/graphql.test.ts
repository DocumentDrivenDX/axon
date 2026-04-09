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

function supportedHelperContract() {
	return {
		__schema: {
			queryType: {
				fields: [{ name: 'collections' }, { name: 'entities' }, { name: 'entity' }],
			},
		},
		collectionMeta: {
			fields: [{ name: 'name' }, { name: 'entityCount' }],
		},
		entityConnection: {
			fields: [{ name: 'edges' }, { name: 'pageInfo' }],
		},
	};
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
				},
				collectionMeta: null,
				entityConnection: null,
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchCollections()).rejects.toThrow(
		/does not expose the collection\/entity helper contract/i,
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
			return createJsonResponse({ data: supportedHelperContract() });
		}

		return createJsonResponse({
			data: {
				collections: [{ name: 'tasks', entityCount: 3 }],
			},
		});
	}) as unknown as typeof fetch;

	await expect(fetchCollections()).resolves.toEqual([{ name: 'tasks', entityCount: 3 }]);
	expect(requests).toHaveLength(2);
	expect(requests[1]?.query).toContain('collections { name entityCount }');
});

test('fetchEntities uses the entities query once the backend advertises support', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		if (requests.length === 1) {
			return createJsonResponse({ data: supportedHelperContract() });
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
	expect(requests[1]?.query).toContain('entities(collection: $collection');
	expect(requests[1]?.variables).toEqual({ collection: 'tasks', limit: 10, after: null });
});

test('fetchEntity uses the entity query once the backend advertises support', async () => {
	const requests: GraphQLRequest[] = [];

	globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
		const request = parseRequest(init);
		requests.push(request);

		if (requests.length === 1) {
			return createJsonResponse({ data: supportedHelperContract() });
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
