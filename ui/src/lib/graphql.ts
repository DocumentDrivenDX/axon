/**
 * GraphQL client for the Axon Admin UI.
 *
 * US-051: Admin UI uses GraphQL. All data fetches go through this client.
 * Target: <200ms p99 for entity detail queries.
 */

const GRAPHQL_ENDPOINT = '/graphql';
const GRAPHQL_COLLECTIONS_HELPER_CONTRACT_ERROR =
	'Axon /graphql does not expose the collections helper contract yet.';
const GRAPHQL_ENTITY_HELPER_CONTRACT_ERROR =
	'Axon /graphql does not expose the entity helper contract yet.';
const GRAPHQL_HELPER_CONTRACT_QUERY = `query AxonUiGraphQLHelperContract {
	__schema {
		queryType {
			fields {
				name
				args {
					name
					type {
						kind
						name
						ofType {
							kind
							name
							ofType {
								kind
								name
								ofType {
									kind
									name
									ofType {
										kind
										name
									}
								}
							}
						}
					}
				}
				type {
					kind
					name
					ofType {
						kind
						name
						ofType {
							kind
							name
							ofType {
								kind
								name
								ofType {
									kind
									name
								}
							}
						}
					}
				}
			}
		}
	}
	collectionMeta: __type(name: "CollectionMeta") {
		name
		fields {
			name
			type {
				kind
				name
				ofType {
					kind
					name
					ofType {
						kind
						name
						ofType {
							kind
							name
							ofType {
								kind
								name
							}
						}
					}
				}
			}
		}
	}
	entityRecord: __type(name: "EntityRecord") {
		name
		fields {
			name
			type {
				kind
				name
				ofType {
					kind
					name
					ofType {
						kind
						name
						ofType {
							kind
							name
							ofType {
								kind
								name
							}
						}
					}
				}
			}
		}
	}
	entityConnection: __type(name: "EntityConnection") {
		name
		fields {
			name
			type {
				kind
				name
				ofType {
					kind
					name
					ofType {
						kind
						name
						ofType {
							kind
							name
							ofType {
								kind
								name
							}
						}
					}
				}
			}
		}
	}
	entityEdge: __type(name: "EntityEdge") {
		name
		fields {
			name
			type {
				kind
				name
				ofType {
					kind
					name
					ofType {
						kind
						name
						ofType {
							kind
							name
							ofType {
								kind
								name
							}
						}
					}
				}
			}
		}
	}
	pageInfo: __type(name: "PageInfo") {
		name
		fields {
			name
			type {
				kind
				name
				ofType {
					kind
					name
					ofType {
						kind
						name
						ofType {
							kind
							name
							ofType {
								kind
								name
							}
						}
					}
				}
			}
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
	args?: GraphQLArgumentMetadata[] | null;
	type?: GraphQLTypeRefMetadata | null;
};

type GraphQLArgumentMetadata = {
	name: string;
	type?: GraphQLTypeRefMetadata | null;
};

type GraphQLTypeRefMetadata = {
	kind: string;
	name?: string | null;
	ofType?: GraphQLTypeRefMetadata | null;
};

type GraphQLNamedTypeMetadata = {
	name: string;
	fields?: GraphQLFieldMetadata[] | null;
};

type GraphQLHelperContractData = {
	__schema: {
		queryType?: {
			fields?: GraphQLFieldMetadata[] | null;
		} | null;
	};
	collectionMeta?: GraphQLNamedTypeMetadata | null;
	entityRecord?: GraphQLNamedTypeMetadata | null;
	entityConnection?: GraphQLNamedTypeMetadata | null;
	entityEdge?: GraphQLNamedTypeMetadata | null;
	pageInfo?: GraphQLNamedTypeMetadata | null;
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

let helperContractMetadata: Promise<GraphQLHelperContractData> | null = null;

function hasFields(
	typeMetadata: GraphQLNamedTypeMetadata | null | undefined,
	requiredFields: string[],
): boolean {
	const fieldNames = new Set((typeMetadata?.fields ?? []).map((field) => field.name));

	return requiredFields.every((fieldName) => fieldNames.has(fieldName));
}

function unwrapNamedType(typeMetadata: GraphQLTypeRefMetadata | null | undefined): string | null {
	let currentType = typeMetadata ?? null;

	while (currentType) {
		if (currentType.name) {
			return currentType.name;
		}

		currentType = currentType.ofType ?? null;
	}

	return null;
}

function getField(
	typeMetadata: { fields?: GraphQLFieldMetadata[] | null } | null | undefined,
	fieldName: string,
): GraphQLFieldMetadata | null {
	return (typeMetadata?.fields ?? []).find((field) => field.name === fieldName) ?? null;
}

function getArgument(
	fieldMetadata: GraphQLFieldMetadata | null | undefined,
	argumentName: string,
): GraphQLArgumentMetadata | null {
	return (fieldMetadata?.args ?? []).find((argument) => argument.name === argumentName) ?? null;
}

function hasArgumentType(
	fieldMetadata: GraphQLFieldMetadata | null | undefined,
	argumentName: string,
	expectedTypeName: string,
): boolean {
	return unwrapNamedType(getArgument(fieldMetadata, argumentName)?.type) === expectedTypeName;
}

function getNamedType(
	helperContractMetadata: GraphQLHelperContractData,
	typeName: string | null,
): GraphQLNamedTypeMetadata | null {
	switch (typeName) {
		case 'CollectionMeta':
			return helperContractMetadata.collectionMeta ?? null;
		case 'EntityRecord':
			return helperContractMetadata.entityRecord ?? null;
		case 'EntityConnection':
			return helperContractMetadata.entityConnection ?? null;
		case 'EntityEdge':
			return helperContractMetadata.entityEdge ?? null;
		case 'PageInfo':
			return helperContractMetadata.pageInfo ?? null;
		default:
			return null;
	}
}

function fieldReferencesTypeWithFields(
	helperContractMetadata: GraphQLHelperContractData,
	fieldMetadata: GraphQLFieldMetadata | null | undefined,
	requiredFields: string[],
): boolean {
	const typeMetadata = getNamedType(helperContractMetadata, unwrapNamedType(fieldMetadata?.type));

	return hasFields(typeMetadata, requiredFields);
}

async function loadGraphQLHelperContractMetadata(): Promise<GraphQLHelperContractData> {
	if (!helperContractMetadata) {
		const probe = gqlQuery<GraphQLHelperContractData>(GRAPHQL_HELPER_CONTRACT_QUERY);
		const cachedProbe = probe.catch((error) => {
			if (helperContractMetadata === cachedProbe) {
				helperContractMetadata = null;
			}

			throw error;
		});

		helperContractMetadata = cachedProbe;
	}

	return helperContractMetadata;
}

async function assertCollectionsHelperContract(): Promise<void> {
	const data = await loadGraphQLHelperContractMetadata();
	const collectionsField = getField(data.__schema.queryType, 'collections');
	const supportsCollectionsHelperContract =
		fieldReferencesTypeWithFields(data, collectionsField, ['name', 'entityCount']) &&
		(collectionsField?.args ?? []).length === 0;

	if (!supportsCollectionsHelperContract) {
		throw new Error(GRAPHQL_COLLECTIONS_HELPER_CONTRACT_ERROR);
	}
}

async function assertEntityHelperContract(): Promise<void> {
	const data = await loadGraphQLHelperContractMetadata();
	const entityField = getField(data.__schema.queryType, 'entity');
	const entitiesField = getField(data.__schema.queryType, 'entities');
	const entityConnectionType = getNamedType(data, unwrapNamedType(entitiesField?.type));
	const entityEdgeField = getField(entityConnectionType, 'edges');
	const entityEdgeType = getNamedType(data, unwrapNamedType(entityEdgeField?.type));
	const entityNodeField = getField(entityEdgeType, 'node');
	const pageInfoField = getField(entityConnectionType, 'pageInfo');
	const supportsEntityHelperContract =
		hasArgumentType(entityField, 'collection', 'String') &&
		hasArgumentType(entityField, 'id', 'ID') &&
		fieldReferencesTypeWithFields(data, entityField, [
			'id',
			'version',
			'data',
			'createdAt',
			'updatedAt',
		]) &&
		hasArgumentType(entitiesField, 'collection', 'String') &&
		hasArgumentType(entitiesField, 'limit', 'Int') &&
		hasArgumentType(entitiesField, 'after', 'String') &&
		hasFields(entityConnectionType, ['edges', 'pageInfo']) &&
		fieldReferencesTypeWithFields(data, entityNodeField, ['id', 'version', 'data']) &&
		fieldReferencesTypeWithFields(data, pageInfoField, ['hasNextPage', 'endCursor']);

	if (!supportsEntityHelperContract) {
		throw new Error(GRAPHQL_ENTITY_HELPER_CONTRACT_ERROR);
	}
}

export function __resetGraphQLHelperContractForTests(): void {
	helperContractMetadata = null;
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
	await assertCollectionsHelperContract();

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
	await assertEntityHelperContract();

	const data = await gqlQuery<{ entities: EntityConnection }>(
		`query($collection: String!, $limit: Int, $after: String) {
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
	await assertEntityHelperContract();

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
