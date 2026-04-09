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

const GRAPHQL_NAMED_TYPE_QUERY = `query AxonUiGraphQLNamedType($name: String!) {
	namedType: __type(name: $name) {
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

type GraphQLNamedTypeData = {
	namedType?: GraphQLNamedTypeMetadata | null;
};

type GraphQLHelperContractCache = {
	data: GraphQLHelperContractData;
	namedTypes: Map<string, Promise<GraphQLNamedTypeMetadata | null>>;
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

let helperContractMetadata: Promise<GraphQLHelperContractCache> | null = null;

function hasFields(
	typeMetadata: GraphQLNamedTypeMetadata | null | undefined,
	requiredFields: string[],
): boolean {
	const fieldNames = new Set((typeMetadata?.fields ?? []).map((field) => field.name));

	return requiredFields.every((fieldName) => fieldNames.has(fieldName));
}

function unwrapNamedType(typeMetadata: GraphQLTypeRefMetadata | null | undefined): string | null {
	return unwrapNamedTypeRef(typeMetadata)?.name ?? null;
}

function unwrapNamedTypeRef(
	typeMetadata: GraphQLTypeRefMetadata | null | undefined,
): GraphQLTypeRefMetadata | null {
	let currentType = typeMetadata ?? null;

	while (currentType) {
		if (currentType.name) {
			return currentType;
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

function seedNamedTypes(
	helperContractData: GraphQLHelperContractData,
): Map<string, Promise<GraphQLNamedTypeMetadata | null>> {
	const namedTypes = new Map<string, Promise<GraphQLNamedTypeMetadata | null>>();

	for (const typeMetadata of [
		helperContractData.collectionMeta,
		helperContractData.entityRecord,
		helperContractData.entityConnection,
		helperContractData.entityEdge,
		helperContractData.pageInfo,
	]) {
		if (typeMetadata?.name) {
			namedTypes.set(typeMetadata.name, Promise.resolve(typeMetadata));
		}
	}

	return namedTypes;
}

async function getNamedType(
	helperContractMetadata: GraphQLHelperContractCache,
	typeName: string | null,
): Promise<GraphQLNamedTypeMetadata | null> {
	if (!typeName) {
		return null;
	}

	const cachedType = helperContractMetadata.namedTypes.get(typeName);

	if (cachedType) {
		return cachedType;
	}

	const probe = gqlQuery<GraphQLNamedTypeData>(GRAPHQL_NAMED_TYPE_QUERY, { name: typeName }).then(
		(result) => result.namedType ?? null,
	);
	const cachedProbe = probe.catch((error) => {
		if (helperContractMetadata.namedTypes.get(typeName) === cachedProbe) {
			helperContractMetadata.namedTypes.delete(typeName);
		}

		throw error;
	});

	helperContractMetadata.namedTypes.set(typeName, cachedProbe);

	return cachedProbe;
}

async function getFieldNamedType(
	helperContractMetadata: GraphQLHelperContractCache,
	fieldMetadata: GraphQLFieldMetadata | null | undefined,
): Promise<GraphQLNamedTypeMetadata | null> {
	const namedTypeRef = unwrapNamedTypeRef(fieldMetadata?.type);

	if (namedTypeRef?.kind !== 'OBJECT') {
		return null;
	}

	return getNamedType(helperContractMetadata, namedTypeRef.name ?? null);
}

async function fieldReferencesTypeWithFields(
	helperContractMetadata: GraphQLHelperContractCache,
	fieldMetadata: GraphQLFieldMetadata | null | undefined,
	requiredFields: string[],
): Promise<boolean> {
	const typeMetadata = await getFieldNamedType(helperContractMetadata, fieldMetadata);

	return hasFields(typeMetadata, requiredFields);
}

async function loadGraphQLHelperContractMetadata(): Promise<GraphQLHelperContractCache> {
	if (!helperContractMetadata) {
		const probe = gqlQuery<GraphQLHelperContractData>(GRAPHQL_HELPER_CONTRACT_QUERY);
		const cachedProbe = probe
			.then((data) => ({
				data,
				namedTypes: seedNamedTypes(data),
			}))
			.catch((error) => {
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
	const metadata = await loadGraphQLHelperContractMetadata();
	const collectionsField = getField(metadata.data.__schema.queryType, 'collections');
	const supportsCollectionsHelperContract =
		(await fieldReferencesTypeWithFields(metadata, collectionsField, ['name', 'entityCount'])) &&
		(collectionsField?.args ?? []).length === 0;

	if (!supportsCollectionsHelperContract) {
		throw new Error(GRAPHQL_COLLECTIONS_HELPER_CONTRACT_ERROR);
	}
}

async function assertEntityHelperContract(): Promise<void> {
	const metadata = await loadGraphQLHelperContractMetadata();
	const entityField = getField(metadata.data.__schema.queryType, 'entity');
	const entitiesField = getField(metadata.data.__schema.queryType, 'entities');
	const entityConnectionType = await getFieldNamedType(metadata, entitiesField);
	const entityEdgeField = getField(entityConnectionType, 'edges');
	const entityEdgeType = await getFieldNamedType(metadata, entityEdgeField);
	const entityNodeField = getField(entityEdgeType, 'node');
	const pageInfoField = getField(entityConnectionType, 'pageInfo');
	const supportsEntityHelperContract =
		hasArgumentType(entityField, 'collection', 'String') &&
		hasArgumentType(entityField, 'id', 'ID') &&
		(await fieldReferencesTypeWithFields(metadata, entityField, [
			'id',
			'version',
			'data',
			'createdAt',
			'updatedAt',
		])) &&
		hasArgumentType(entitiesField, 'collection', 'String') &&
		hasArgumentType(entitiesField, 'limit', 'Int') &&
		hasArgumentType(entitiesField, 'after', 'String') &&
		hasFields(entityConnectionType, ['edges', 'pageInfo']) &&
		(await fieldReferencesTypeWithFields(metadata, entityNodeField, ['id', 'version', 'data'])) &&
		(await fieldReferencesTypeWithFields(metadata, pageInfoField, ['hasNextPage', 'endCursor']));

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
