/**
 * GraphQL client for the Axon Admin UI.
 *
 * US-051: Admin UI uses GraphQL. All data fetches go through this client.
 * Target: <200ms p99 for entity detail queries.
 */

const GRAPHQL_ENDPOINT = '/graphql';

/**
 * Execute a GraphQL query against the Axon server.
 * @param {string} query - GraphQL query string
 * @param {object} variables - Query variables
 * @returns {Promise<object>} - Query result
 */
export async function gqlQuery(query, variables = {}) {
	const response = await fetch(GRAPHQL_ENDPOINT, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ query, variables }),
	});

	if (!response.ok) {
		throw new Error(`GraphQL request failed: ${response.status}`);
	}

	const result = await response.json();
	if (result.errors) {
		throw new Error(result.errors.map((e) => e.message).join(', '));
	}

	return result.data;
}

/**
 * Fetch all collections.
 */
export async function fetchCollections() {
	return gqlQuery('{ collections { name entityCount } }');
}

/**
 * Fetch entities in a collection with pagination.
 */
export async function fetchEntities(collection, { limit = 50, after = null } = {}) {
	return gqlQuery(
		`query($collection: String!, $limit: Int, $after: ID) {
			entities(collection: $collection, limit: $limit, after: $after) {
				edges { node { id version data } }
				pageInfo { hasNextPage endCursor }
			}
		}`,
		{ collection, limit, after },
	);
}

/**
 * Fetch a single entity by ID (entity detail = single query).
 */
export async function fetchEntity(collection, id) {
	return gqlQuery(
		`query($collection: String!, $id: ID!) {
			entity(collection: $collection, id: $id) {
				id version data createdAt updatedAt
			}
		}`,
		{ collection, id },
	);
}
