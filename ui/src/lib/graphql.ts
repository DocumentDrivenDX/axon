/**
 * GraphQL client for the Axon Admin UI.
 *
 * US-051: Admin UI uses GraphQL. All data fetches go through this client.
 * Target: <200ms p99 for entity detail queries.
 */

const GRAPHQL_ENDPOINT = '/graphql';
const GRAPHQL_HELPER_CONTRACT_ERROR =
	'Axon /graphql does not expose the collection/entity helper contract yet.';
const GRAPHQL_HELPER_CONTRACT_QUERY = `query AxonUiGraphQLHelperContract {
	__schema {
		queryType {
			fields {
				name
			}
		}
	}
	collectionMeta: __type(name: "CollectionMeta") {
		fields {
			name
		}
	}
	entityConnection: __type(name: "EntityConnection") {
		fields {
			name
		}
	}
}`;

type GraphQLError = {
	message: string;
};

type GraphQLResponse<T> = {
	data?: T;
	errors?: GraphQLError[];
};

type GraphQLFieldMetadata = {
	name: string;
};

type GraphQLTypeMetadata = {
	fields?: GraphQLFieldMetadata[] | null;
};

type GraphQLHelperContractData = {
	__schema: {
		queryType?: {
			fields?: GraphQLFieldMetadata[] | null;
		} | null;
	};
	collectionMeta?: GraphQLTypeMetadata | null;
	entityConnection?: GraphQLTypeMetadata | null;
};

export type CollectionSummary = {
	name: string;
	entityCount: number;
};

export type EntityRecord = {
	id: string;
	version: number;
	data: unknown;
	createdAt?: string;
	updatedAt?: string;
};

export type EntityConnection = {
	edges: Array<{ node: EntityRecord }>;
	pageInfo: {
		hasNextPage: boolean;
		endCursor: string | null;
	};
};

let helperContractCheck: Promise<void> | null = null;

function hasFields(
	typeMetadata: GraphQLTypeMetadata | null | undefined,
	requiredFields: string[],
): boolean {
	const fieldNames = new Set((typeMetadata?.fields ?? []).map((field) => field.name));

	return requiredFields.every((fieldName) => fieldNames.has(fieldName));
}

async function assertGraphQLHelperContract(): Promise<void> {
	helperContractCheck ??= (async () => {
		const data = await gqlQuery<GraphQLHelperContractData>(GRAPHQL_HELPER_CONTRACT_QUERY);
		const queryFieldNames = new Set(
			(data.__schema.queryType?.fields ?? []).map((field) => field.name),
		);
		const supportsHelperContract =
			queryFieldNames.has('collections') &&
			queryFieldNames.has('entities') &&
			queryFieldNames.has('entity') &&
			hasFields(data.collectionMeta, ['name', 'entityCount']) &&
			hasFields(data.entityConnection, ['edges', 'pageInfo']);

		if (!supportsHelperContract) {
			throw new Error(GRAPHQL_HELPER_CONTRACT_ERROR);
		}
	})();

	return helperContractCheck;
}

export function __resetGraphQLHelperContractForTests(): void {
	helperContractCheck = null;
}

/**
 * Execute a GraphQL query against the Axon server.
 */
export async function gqlQuery<T>(
	query: string,
	variables: Record<string, unknown> = {},
): Promise<T> {
	const response = await fetch(GRAPHQL_ENDPOINT, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ query, variables }),
	});

	if (!response.ok) {
		throw new Error(`GraphQL request failed: ${response.status}`);
	}

	const result = (await response.json()) as GraphQLResponse<T>;
	if (result.errors?.length) {
		throw new Error(result.errors.map((error) => error.message).join(', '));
	}

	if (result.data === undefined) {
		throw new Error('GraphQL response missing data');
	}

	return result.data;
}

/**
 * Fetch all collections.
 */
export async function fetchCollections(): Promise<CollectionSummary[]> {
	await assertGraphQLHelperContract();

	const data = await gqlQuery<{ collections: CollectionSummary[] }>(
		'{ collections { name entityCount } }',
	);

	return data.collections;
}

/**
 * Fetch entities in a collection with pagination.
 */
export async function fetchEntities(
	collection: string,
	{ limit = 50, after = null }: { limit?: number; after?: string | null } = {},
): Promise<EntityConnection> {
	await assertGraphQLHelperContract();

	const data = await gqlQuery<{ entities: EntityConnection }>(
		`query($collection: String!, $limit: Int, $after: ID) {
			entities(collection: $collection, limit: $limit, after: $after) {
				edges { node { id version data } }
				pageInfo { hasNextPage endCursor }
			}
		}`,
		{ collection, limit, after },
	);

	return data.entities;
}

/**
 * Fetch a single entity by ID (entity detail = single query).
 */
export async function fetchEntity(collection: string, id: string): Promise<EntityRecord> {
	await assertGraphQLHelperContract();

	const data = await gqlQuery<{ entity: EntityRecord }>(
		`query($collection: String!, $id: ID!) {
			entity(collection: $collection, id: $id) {
				id version data createdAt updatedAt
			}
		}`,
		{ collection, id },
	);

	return data.entity;
}
