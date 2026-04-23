export type ApiError = {
	code?: string;
	detail?: unknown;
};

export type CollectionSchema = {
	collection: string;
	description?: string | null;
	version: number;
	entity_schema?: unknown;
	link_types?: Record<string, unknown>;
};

export type FieldChange = {
	path: string;
	kind:
		| 'added'
		| 'removed'
		| 'modified'
		| 'made_required'
		| 'made_optional'
		| 'enum_widened'
		| 'enum_narrowed'
		| 'constraint_tightened'
		| 'constraint_relaxed';
	description: string;
};

export type SchemaDiff = {
	compatibility: 'compatible' | 'breaking' | 'metadata_only';
	changes: FieldChange[];
};

export type SchemaPreviewResult = {
	schema: CollectionSchema;
	compatibility: 'compatible' | 'breaking' | 'metadata_only' | null;
	diff: SchemaDiff | null;
	dry_run: boolean;
};

export type CollectionSummary = {
	name: string;
	entity_count: number;
	schema_version: number | null;
	created_at_ns?: number | null;
	updated_at_ns?: number | null;
};

export type CollectionDetail = {
	name: string;
	entity_count: number;
	schema: CollectionSchema | null;
	created_at_ns?: number | null;
	updated_at_ns?: number | null;
};

export type EntityRecord = {
	collection: string;
	id: string;
	version: number;
	data: Record<string, unknown>;
	schema_version?: number | null;
};

export type QueryEntitiesResult = {
	entities: EntityRecord[];
	total_count: number;
	next_cursor: string | null;
};

export type AuditEntry = {
	id: number;
	timestamp_ns: number;
	collection: string;
	entity_id: string;
	version: number;
	mutation: string;
	data_before: unknown;
	data_after: unknown;
	actor: string | null;
	transaction_id?: number | null;
	metadata?: Record<string, string> | null;
	diff?: Record<string, unknown> | null;
};

export type AuditQueryResult = {
	entries: AuditEntry[];
	next_cursor: number | null;
};

export type HealthStatus = {
	status: string;
	version: string;
	uptime_seconds: number;
	backing_store: {
		backend: string;
		status: string;
	};
	databases: string[];
	default_namespace: string;
};

export type TenantDatabase = {
	tenant_id: string;
	name: string;
	created_at_ms: number;
	entity_count?: number;
};

/** Tenant + database routing scope for ADR-018 path-based URLs. */
export type Scope = { tenant: string; database: string } | null;

type QueryEntitiesInput = {
	limit?: number;
	afterId?: string | null;
};

type ScopedTenantDatabase = NonNullable<Scope>;

type GraphQLError = {
	message: string;
	path?: Array<string | number>;
	extensions?: {
		code?: string;
		stale?: MutationIntentStaleDimension[];
		[key: string]: unknown;
	};
};

type GraphQLResult<T> = {
	data?: T;
	errors?: GraphQLError[];
};

type GraphQLCollectionMeta = {
	name: string;
	entityCount: number;
	schemaVersion: number | null;
	schema?: CollectionSchema | null;
};

type GraphQLEntity = {
	collection: string;
	id: string;
	version: number;
	data: Record<string, unknown> | null;
};

type GraphQLEntityConnection = {
	totalCount: number;
	edges: Array<{
		cursor: string;
		node: GraphQLEntity;
	}>;
	pageInfo: {
		hasNextPage: boolean;
		endCursor: string | null;
	};
};

type GraphQLPutSchemaPayload = {
	schema: CollectionSchema;
	compatibility: SchemaPreviewResult['compatibility'];
	diff: SchemaDiff | null;
	dryRun: boolean;
};

type GraphQLTransactionPayload = {
	results: Array<{
		index: number;
		success: boolean;
		collection: string;
		id: string;
		entity: GraphQLEntity | null;
	}>;
};

type GraphQLAuditEntry = {
	id: string;
	timestampNs: string;
	collection: string;
	entityId: string;
	version: number;
	mutation: string;
	dataBefore: unknown;
	dataAfter: unknown;
	actor: string | null;
	transactionId?: number | null;
	metadata?: Record<string, string> | null;
};

type GraphQLAuditConnection = {
	edges: Array<{
		cursor: string;
		node: GraphQLAuditEntry;
	}>;
	pageInfo: {
		hasNextPage: boolean;
		endCursor: string | null;
	};
};

type GraphQLNeighborConnection = {
	groups: Array<{
		edges: Array<{
			node: GraphQLEntity;
			linkType: string;
			sourceCollection: string;
			sourceId: string;
			targetCollection: string;
			targetId: string;
		}>;
	}>;
};

type GraphQLRollbackEntityPayload = {
	dryRun: boolean;
	current: GraphQLEntity | null;
	target: GraphQLEntity;
	diff: Record<string, FieldDiff>;
	entity: GraphQLEntity | null;
	auditEntry: GraphQLAuditEntry | null;
};

type GraphQLCollectionTemplate = {
	collection: string;
	template: string;
	version: number;
	updatedAtNs?: string | null;
	updatedBy?: string | null;
	warnings?: string[];
};

type GraphQLRenderedEntity = {
	entity: GraphQLEntity;
	markdown: string;
};

type GraphQLRevertAuditEntryPayload = {
	entity: GraphQLEntity;
	auditEntry: GraphQLAuditEntry;
};

export type MutationIntentDecision = 'allow' | 'needs_approval' | 'deny';

export type MutationIntentApprovalState =
	| 'none'
	| 'pending'
	| 'approved'
	| 'rejected'
	| 'expired'
	| 'committed';

export type MutationApprovalRoute = {
	role: string;
	reasonRequired?: boolean;
	deadlineSeconds?: number | null;
	separationOfDuties?: boolean;
};

export type MutationIntentPreImage = {
	kind: 'entity' | 'link';
	collection: string;
	id?: string | null;
	version?: number | null;
};

export type MutationIntentOperationInput = {
	operationKind: string;
	operationHash?: string;
	operation: Record<string, unknown>;
};

export type MutationIntentCanonicalOperation = {
	operationKind: string;
	operationHash: string;
	operation: unknown;
};

export type MutationReviewSummary = {
	title?: string;
	summary?: string;
	risk?: string;
	affected_records?: MutationIntentPreImage[];
	affected_fields?: string[];
	diff?: unknown;
	policy_explanation?: string[];
};

export type MutationIntent = {
	id: string;
	tenantId: string;
	databaseId: string;
	subject: unknown;
	schemaVersion: number;
	policyVersion: number;
	operation: MutationIntentCanonicalOperation;
	operationHash: string;
	preImages: MutationIntentPreImage[];
	decision: MutationIntentDecision;
	approvalState: MutationIntentApprovalState;
	approvalRoute?: MutationApprovalRoute | null;
	expiresAtNs: string;
	reviewSummary: MutationReviewSummary;
};

export type MutationPreviewInput = {
	operation: MutationIntentOperationInput;
	subject?: unknown;
	expiresInSeconds?: number;
	reason?: string;
};

export type MutationPreviewResult = {
	decision: MutationIntentDecision;
	intent: MutationIntent | null;
	intentToken: string | null;
	canonicalOperation: MutationIntentCanonicalOperation;
	diff: unknown;
	affectedRecords: MutationIntentPreImage[];
	affectedFields: string[];
	approvalRoute?: MutationApprovalRoute | null;
	policyExplanation: string[];
};

export type MutationIntentStaleDimension = {
	dimension: string;
	expected?: string | null;
	actual?: string | null;
	path?: string | null;
};

export type CommitMutationIntentResult = {
	committed: boolean;
	intent: MutationIntent | null;
	transactionId?: string | null;
	stale: MutationIntentStaleDimension[];
	errorCode?: string | null;
};

export type MutationIntentError = {
	message: string;
	code?: string;
	stale: MutationIntentStaleDimension[];
};

export type CommitMutationIntentOutcome =
	| { ok: true; result: CommitMutationIntentResult }
	| { ok: false; error: MutationIntentError };

export type MutationIntentFilter = {
	status?: MutationIntentApprovalState;
	statuses?: MutationIntentApprovalState[];
	decision?: MutationIntentDecision;
	includeExpired?: boolean;
};

export type MutationIntentPageInfo = {
	hasNextPage: boolean;
	hasPreviousPage: boolean;
	startCursor: string | null;
	endCursor: string | null;
};

export type MutationIntentEdge = {
	cursor: string;
	node: MutationIntent;
};

export type MutationIntentConnection = {
	totalCount: number;
	edges: MutationIntentEdge[];
	pageInfo: MutationIntentPageInfo;
};

export type MutationIntentListInput = {
	filter?: MutationIntentFilter;
	limit?: number;
	after?: string | null;
};

type AuditFilters = {
	collection?: string;
	actor?: string;
	sinceNs?: string;
	untilNs?: string;
};

type ControlGraphQLTenant = {
	id: string;
	name: string;
	dbName: string;
	createdAt: string;
};

type ControlGraphQLTenantDatabase = {
	tenantId: string;
	name: string;
	createdAtMs: number;
};

type ControlGraphQLUser = {
	id: string;
	displayName: string;
	email: string | null;
	createdAtMs: number;
	suspendedAtMs: number | null;
};

type ControlGraphQLUserAclEntry = {
	login: string;
	role: UserRole;
};

type ControlGraphQLTenantMember = {
	tenantId: string;
	userId: string;
	role: TenantMemberRole;
};

type ControlGraphQLCredential = {
	jti: string;
	userId: string;
	tenantId: string;
	issuedAtMs: number;
	expiresAtMs: number;
	revoked: boolean;
	grants: Grants;
};

function formatError(error: ApiError, status: number): string {
	const detail =
		typeof error.detail === 'string'
			? error.detail
			: error.detail
				? JSON.stringify(error.detail)
				: `Request failed with status ${status}`;

	return error.code ? `${error.code}: ${detail}` : detail;
}

function scopedPath(path: string, scope: ScopedTenantDatabase): string {
	return `/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}${path}`;
}

function pascalCase(value: string): string {
	const words = value.split(/[^A-Za-z0-9]+/).filter(Boolean);
	const name = words
		.map((word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase())
		.join('');
	return name || 'Collection';
}

async function request<T>(path: string, init?: RequestInit, scope?: Scope): Promise<T> {
	const headers = new Headers(init?.headers);
	if (init?.body && !headers.has('Content-Type')) {
		headers.set('Content-Type', 'application/json');
	}

	// Control-plane routes (/control/*) are NOT tenant-scoped.
	const url = scope && !path.startsWith('/control/') ? scopedPath(path, scope) : path;

	const response = await fetch(url, {
		...init,
		headers,
	});
	const text = await response.text();
	const payload = text ? (JSON.parse(text) as T | ApiError) : null;

	if (!response.ok) {
		throw new Error(formatError((payload as ApiError | null) ?? {}, response.status));
	}

	return payload as T;
}

async function graphqlRequest<T>(
	scope: ScopedTenantDatabase,
	query: string,
	variables: Record<string, unknown> = {},
): Promise<T> {
	const response = await fetch(scopedPath('/graphql', scope), {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ query, variables }),
	});
	const text = await response.text();
	const payload = text ? (JSON.parse(text) as GraphQLResult<T> | ApiError) : null;

	if (!response.ok) {
		throw new Error(formatError((payload as ApiError | null) ?? {}, response.status));
	}

	const result = payload as GraphQLResult<T> | null;
	if (result?.errors?.length) {
		throw new Error(
			result.errors
				.map((error) => {
					const code = error.extensions?.code;
					return code ? `${code}: ${error.message}` : error.message;
				})
				.join(', '),
		);
	}
	if (result?.data === undefined) {
		throw new Error('GraphQL response missing data');
	}

	return result.data;
}

async function graphqlRawRequest<T>(
	scope: ScopedTenantDatabase,
	query: string,
	variables: Record<string, unknown> = {},
): Promise<GraphQLResult<T>> {
	const response = await fetch(scopedPath('/graphql', scope), {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ query, variables }),
	});
	const text = await response.text();
	const payload = text ? (JSON.parse(text) as GraphQLResult<T> | ApiError) : null;

	if (!response.ok) {
		throw new Error(formatError((payload as ApiError | null) ?? {}, response.status));
	}

	return (payload as GraphQLResult<T> | null) ?? {};
}

async function controlGraphqlRequest<T>(
	query: string,
	variables: Record<string, unknown> = {},
): Promise<T> {
	const response = await fetch('/control/graphql', {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ query, variables }),
	});
	const text = await response.text();
	const payload = text ? (JSON.parse(text) as GraphQLResult<T> | ApiError) : null;

	if (!response.ok) {
		throw new Error(formatError((payload as ApiError | null) ?? {}, response.status));
	}

	const result = payload as GraphQLResult<T> | null;
	if (result?.errors?.length) {
		throw new Error(
			result.errors
				.map((error) => {
					const code = error.extensions?.code;
					return code ? `${code}: ${error.message}` : error.message;
				})
				.join(', '),
		);
	}
	if (result?.data === undefined) {
		throw new Error('GraphQL response missing data');
	}

	return result.data;
}

function collectionSummaryFromGraphql(collection: GraphQLCollectionMeta): CollectionSummary {
	return {
		name: collection.name,
		entity_count: collection.entityCount,
		schema_version: collection.schemaVersion,
	};
}

function collectionDetailFromGraphql(collection: GraphQLCollectionMeta): CollectionDetail {
	return {
		name: collection.name,
		entity_count: collection.entityCount,
		schema: collection.schema ?? null,
	};
}

function entityFromGraphql(entity: GraphQLEntity): EntityRecord {
	return {
		collection: entity.collection,
		id: entity.id,
		version: entity.version,
		data: entity.data ?? {},
	};
}

async function commitSingleEntityOperation(
	scope: ScopedTenantDatabase,
	operation: Record<string, unknown>,
): Promise<GraphQLEntity | null> {
	const data = await graphqlRequest<{ commitTransaction: GraphQLTransactionPayload }>(
		scope,
		`mutation AxonUiCommitEntityOperation($operations: [TransactionOperationInput!]!) {
			commitTransaction(input: { operations: $operations }) {
				results {
					index
					success
					collection
					id
					entity {
						collection
						id
						version
						data
					}
				}
			}
		}`,
		{ operations: [operation] },
	);

	return data.commitTransaction.results[0]?.entity ?? null;
}

function auditEntryFromGraphql(entry: GraphQLAuditEntry): AuditEntry {
	return {
		id: Number(entry.id),
		timestamp_ns: Number(entry.timestampNs),
		collection: entry.collection,
		entity_id: entry.entityId,
		version: entry.version,
		mutation: entry.mutation,
		data_before: entry.dataBefore,
		data_after: entry.dataAfter,
		actor: entry.actor,
		transaction_id: entry.transactionId ?? null,
		metadata: entry.metadata ?? null,
	};
}

function auditResultFromGraphql(connection: GraphQLAuditConnection): AuditQueryResult {
	return {
		entries: connection.edges.map((edge) => auditEntryFromGraphql(edge.node)),
		next_cursor: connection.pageInfo.hasNextPage ? Number(connection.pageInfo.endCursor) : null,
	};
}

function collectionTemplateFromGraphql(template: GraphQLCollectionTemplate): CollectionView {
	return {
		collection: template.collection,
		template: template.template,
		version: template.version,
		updated_at_ns: template.updatedAtNs ? Number(template.updatedAtNs) : null,
		updated_by: template.updatedBy ?? null,
	};
}

function tenantFromControlGraphql(tenant: ControlGraphQLTenant): Tenant {
	return {
		id: tenant.id,
		name: tenant.name,
		db_name: tenant.dbName,
		created_at: tenant.createdAt,
	};
}

function tenantDatabaseFromControlGraphql(database: ControlGraphQLTenantDatabase): TenantDatabase {
	return {
		tenant_id: database.tenantId,
		name: database.name,
		created_at_ms: database.createdAtMs,
	};
}

function userFromControlGraphql(user: ControlGraphQLUser): User {
	return {
		id: user.id,
		display_name: user.displayName,
		email: user.email,
		created_at_ms: user.createdAtMs,
		suspended_at_ms: user.suspendedAtMs,
	};
}

function tenantMemberFromControlGraphql(member: ControlGraphQLTenantMember): TenantMember {
	return {
		tenant_id: member.tenantId,
		user_id: member.userId,
		role: member.role,
	};
}

function credentialFromControlGraphql(credential: ControlGraphQLCredential): Credential {
	return {
		jti: credential.jti,
		user_id: credential.userId,
		tenant_id: credential.tenantId,
		issued_at_ms: credential.issuedAtMs,
		expires_at_ms: credential.expiresAtMs,
		revoked: credential.revoked,
		grants: credential.grants,
	};
}

export async function fetchCollections(scope?: Scope): Promise<CollectionSummary[]> {
	if (scope) {
		const data = await graphqlRequest<{ collections: GraphQLCollectionMeta[] }>(
			scope,
			`query AxonUiCollections {
				collections {
					name
					entityCount
					schemaVersion
				}
			}`,
		);
		return data.collections.map(collectionSummaryFromGraphql);
	}

	const response = await request<{ collections: CollectionSummary[] }>(
		'/collections',
		undefined,
		scope,
	);
	return response.collections;
}

export async function fetchCollection(name: string, scope?: Scope): Promise<CollectionDetail> {
	if (scope) {
		const data = await graphqlRequest<{ collection: GraphQLCollectionMeta | null }>(
			scope,
			`query AxonUiCollection($name: String!) {
				collection(name: $name) {
					name
					entityCount
					schemaVersion
					schema
				}
			}`,
			{ name },
		);
		if (!data.collection) {
			throw new Error(`Collection not found: ${name}`);
		}
		return collectionDetailFromGraphql(data.collection);
	}

	return request<CollectionDetail>(`/collections/${encodeURIComponent(name)}`, undefined, scope);
}

export async function fetchEntities(
	collection: string,
	options: QueryEntitiesInput = {},
	scope?: Scope,
): Promise<QueryEntitiesResult> {
	if (scope) {
		const data = await graphqlRequest<{ entities: GraphQLEntityConnection }>(
			scope,
			`query AxonUiEntities($collection: String!, $limit: Int, $after: ID) {
				entities(collection: $collection, limit: $limit, after: $after) {
					totalCount
					edges {
						cursor
						node {
							collection
							id
							version
							data
						}
					}
					pageInfo {
						hasNextPage
						endCursor
					}
				}
			}`,
			{
				collection,
				limit: options.limit ?? 50,
				after: options.afterId ?? null,
			},
		);

		return {
			entities: data.entities.edges.map((edge) => entityFromGraphql(edge.node)),
			total_count: data.entities.totalCount,
			next_cursor: data.entities.pageInfo.hasNextPage ? data.entities.pageInfo.endCursor : null,
		};
	}

	return request<QueryEntitiesResult>(
		`/collections/${encodeURIComponent(collection)}/query`,
		{
			method: 'POST',
			body: JSON.stringify({
				limit: options.limit ?? 50,
				after_id: options.afterId ?? null,
			}),
		},
		scope,
	);
}

export async function fetchEntity(
	collection: string,
	id: string,
	scope?: Scope,
): Promise<EntityRecord> {
	if (scope) {
		const data = await graphqlRequest<{ entity: GraphQLEntity | null }>(
			scope,
			`query AxonUiEntity($collection: String!, $id: ID!) {
				entity(collection: $collection, id: $id) {
					collection
					id
					version
					data
				}
			}`,
			{ collection, id },
		);
		if (!data.entity) {
			throw new Error(`Entity not found: ${collection}/${id}`);
		}
		return entityFromGraphql(data.entity);
	}

	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		undefined,
		scope,
	);
	return response.entity;
}

export async function createEntity(
	collection: string,
	id: string,
	data: Record<string, unknown>,
	scope?: Scope,
): Promise<EntityRecord> {
	if (scope) {
		const entity = await commitSingleEntityOperation(scope, {
			createEntity: { collection, id, data },
		});
		if (!entity) {
			throw new Error(`GraphQL create did not return entity: ${collection}/${id}`);
		}
		return entityFromGraphql(entity);
	}

	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		{
			method: 'POST',
			body: JSON.stringify({ data, actor: 'ui' }),
		},
		scope,
	);
	return response.entity;
}

export async function updateEntity(
	collection: string,
	id: string,
	data: Record<string, unknown>,
	expectedVersion: number,
	scope?: Scope,
): Promise<EntityRecord> {
	if (scope) {
		const entity = await commitSingleEntityOperation(scope, {
			updateEntity: {
				collection,
				id,
				expectedVersion,
				data,
			},
		});
		if (!entity) {
			throw new Error(`GraphQL update did not return entity: ${collection}/${id}`);
		}
		return entityFromGraphql(entity);
	}

	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		{
			method: 'PUT',
			body: JSON.stringify({ data, expected_version: expectedVersion, actor: 'ui' }),
		},
		scope,
	);
	return response.entity;
}

export async function fetchSchema(collection: string, scope?: Scope): Promise<CollectionSchema> {
	if (scope) {
		const detail = await fetchCollection(collection, scope);
		if (!detail.schema) {
			throw new Error(`Schema not found: ${collection}`);
		}
		return detail.schema;
	}

	const response = await request<{ schema: CollectionSchema }>(
		`/collections/${encodeURIComponent(collection)}/schema`,
		undefined,
		scope,
	);
	return response.schema;
}

export async function updateSchema(
	collection: string,
	schema: CollectionSchema,
	options?: { force?: boolean },
	scope?: Scope,
): Promise<CollectionSchema> {
	if (scope) {
		const data = await graphqlRequest<{ putSchema: GraphQLPutSchemaPayload }>(
			scope,
			`mutation AxonUiPutSchema(
				$collection: String!
				$schema: JSON!
				$force: Boolean
			) {
				putSchema(input: {
					collection: $collection
					schema: $schema
					force: $force
				}) {
					schema
					compatibility
					diff
					dryRun
				}
			}`,
			{
				collection,
				schema,
				force: options?.force ?? false,
			},
		);
		return data.putSchema.schema;
	}

	const response = await request<{ schema: CollectionSchema }>(
		`/collections/${encodeURIComponent(collection)}/schema`,
		{
			method: 'PUT',
			body: JSON.stringify({
				description: schema.description ?? null,
				version: schema.version,
				entity_schema: schema.entity_schema ?? null,
				link_types: schema.link_types ?? {},
				actor: 'ui',
				force: options?.force ?? false,
			}),
		},
		scope,
	);

	return response.schema;
}

export async function previewSchemaChange(
	collection: string,
	schema: CollectionSchema,
	scope?: Scope,
): Promise<SchemaPreviewResult> {
	if (scope) {
		const data = await graphqlRequest<{ putSchema: GraphQLPutSchemaPayload }>(
			scope,
			`mutation AxonUiPreviewSchema($collection: String!, $schema: JSON!) {
				putSchema(input: {
					collection: $collection
					schema: $schema
					dryRun: true
				}) {
					schema
					compatibility
					diff
					dryRun
				}
			}`,
			{ collection, schema },
		);
		return {
			schema: data.putSchema.schema,
			compatibility: data.putSchema.compatibility,
			diff: data.putSchema.diff,
			dry_run: data.putSchema.dryRun,
		};
	}

	return request<SchemaPreviewResult>(
		`/collections/${encodeURIComponent(collection)}/schema`,
		{
			method: 'PUT',
			body: JSON.stringify({
				description: schema.description ?? null,
				version: schema.version,
				entity_schema: schema.entity_schema ?? null,
				link_types: schema.link_types ?? {},
				actor: 'ui',
				dry_run: true,
			}),
		},
		scope,
	);
}

export async function createCollection(
	name: string,
	schema: Omit<CollectionSchema, 'collection'>,
	scope?: Scope,
): Promise<void> {
	if (scope) {
		await graphqlRequest<{ createCollection: GraphQLCollectionMeta }>(
			scope,
			`mutation AxonUiCreateCollection($name: String!, $schema: JSON!) {
				createCollection(input: { name: $name, schema: $schema }) {
					name
					entityCount
					schemaVersion
					schema
				}
			}`,
			{ name, schema },
		);
		return;
	}

	await request<{ name: string }>(
		`/collections/${encodeURIComponent(name)}`,
		{
			method: 'POST',
			body: JSON.stringify({
				schema: {
					description: schema.description ?? null,
					version: schema.version,
					entity_schema: schema.entity_schema ?? null,
					link_types: schema.link_types ?? {},
				},
				actor: 'ui',
			}),
		},
		scope,
	);
}

export async function fetchAudit(
	filters: AuditFilters = {},
	scope?: Scope,
): Promise<AuditQueryResult> {
	if (scope) {
		const variables: Record<string, string> = {};
		if (filters.collection) variables.collection = filters.collection;
		if (filters.actor) variables.actor = filters.actor;
		if (filters.sinceNs) variables.sinceNs = filters.sinceNs;
		if (filters.untilNs) variables.untilNs = filters.untilNs;
		const data = await graphqlRequest<{ auditLog: GraphQLAuditConnection }>(
			scope,
			`query AxonUiAuditLog(
				$collection: String
				$actor: String
				$sinceNs: String
				$untilNs: String
			) {
				auditLog(
					collection: $collection
					actor: $actor
					sinceNs: $sinceNs
					untilNs: $untilNs
				) {
					edges {
						cursor
						node {
							id
							timestampNs
							collection
							entityId
							version
							mutation
							dataBefore
							dataAfter
							actor
							transactionId
							metadata
						}
					}
					pageInfo {
						hasNextPage
						endCursor
					}
					}
				}`,
			variables,
		);
		return auditResultFromGraphql(data.auditLog);
	}

	const params = new URLSearchParams();
	if (filters.collection) {
		params.set('collection', filters.collection);
	}
	if (filters.actor) {
		params.set('actor', filters.actor);
	}
	if (filters.sinceNs) {
		params.set('since_ns', filters.sinceNs);
	}
	if (filters.untilNs) {
		params.set('until_ns', filters.untilNs);
	}

	const query = params.toString();
	return request<AuditQueryResult>(`/audit/query${query ? `?${query}` : ''}`, undefined, scope);
}

export async function fetchHealth(): Promise<HealthStatus> {
	return request<HealthStatus>('/health');
}

export type AuthIdentity = {
	actor: string;
	role: 'admin' | 'write' | 'read';
};

export type AuthState =
	| { status: 'authenticated'; identity: AuthIdentity }
	| { status: 'unauthenticated' }
	| { status: 'loading' };

export async function fetchAuthMe(): Promise<AuthIdentity> {
	return request<AuthIdentity>('/auth/me');
}

export async function dropCollection(name: string, scope?: Scope): Promise<void> {
	if (scope) {
		await graphqlRequest<{ dropCollection: { name: string; entitiesRemoved: number } }>(
			scope,
			`mutation AxonUiDropCollection($name: String!) {
				dropCollection(input: { name: $name, confirm: true }) {
					name
					entitiesRemoved
				}
			}`,
			{ name },
		);
		return;
	}

	await request<void>(
		`/collections/${encodeURIComponent(name)}`,
		{
			method: 'DELETE',
			body: JSON.stringify({ actor: 'ui' }),
		},
		scope,
	);
}

export async function deleteEntity(collection: string, id: string, scope?: Scope): Promise<void> {
	if (scope) {
		const existing = await fetchEntity(collection, id, scope);
		await commitSingleEntityOperation(scope, {
			deleteEntity: {
				collection,
				id,
				expectedVersion: existing.version,
			},
		});
		return;
	}

	await request<void>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		{
			method: 'DELETE',
			body: JSON.stringify({ actor: 'ui' }),
		},
		scope,
	);
}

// ── Tenant / control-plane API ───────────────────────────────────────────────

export type Tenant = {
	id: string;
	name: string;
	db_name: string;
	created_at: string;
};

export async function fetchTenants(): Promise<Tenant[]> {
	const data = await controlGraphqlRequest<{ tenants: ControlGraphQLTenant[] }>(
		`query AxonUiControlTenants {
			tenants {
				id
				name
				dbName
				createdAt
			}
		}`,
	);
	return data.tenants.map(tenantFromControlGraphql);
}

export async function createTenant(name: string): Promise<Tenant> {
	const data = await controlGraphqlRequest<{ createTenant: ControlGraphQLTenant }>(
		`mutation AxonUiCreateTenant($name: String!) {
			createTenant(name: $name) {
				id
				name
				dbName
				createdAt
			}
		}`,
		{ name },
	);
	return tenantFromControlGraphql(data.createTenant);
}

export async function deleteTenant(tenantId: string): Promise<void> {
	await controlGraphqlRequest<{ deleteTenant: { deleted: boolean } }>(
		`mutation AxonUiDeleteTenant($id: String!) {
			deleteTenant(id: $id) {
				deleted
			}
		}`,
		{ id: tenantId },
	);
}

export async function fetchTenant(tenantId: string): Promise<Tenant> {
	const data = await controlGraphqlRequest<{ tenant: ControlGraphQLTenant | null }>(
		`query AxonUiControlTenant($id: String!) {
			tenant(id: $id) {
				id
				name
				dbName
				createdAt
			}
		}`,
		{ id: tenantId },
	);
	if (!data.tenant) {
		throw new Error(`Tenant not found: ${tenantId}`);
	}
	return tenantFromControlGraphql(data.tenant);
}

export async function fetchTenantDatabases(tenantId: string): Promise<TenantDatabase[]> {
	const data = await controlGraphqlRequest<{
		tenantDatabases: ControlGraphQLTenantDatabase[];
	}>(
		`query AxonUiTenantDatabases($tenantId: String!) {
			tenantDatabases(tenantId: $tenantId) {
				tenantId
				name
				createdAtMs
			}
		}`,
		{ tenantId },
	);
	return data.tenantDatabases.map(tenantDatabaseFromControlGraphql);
}

export async function createTenantDatabase(
	tenantId: string,
	name: string,
): Promise<TenantDatabase> {
	const data = await controlGraphqlRequest<{
		createTenantDatabase: ControlGraphQLTenantDatabase;
	}>(
		`mutation AxonUiCreateTenantDatabase($tenantId: String!, $name: String!) {
			createTenantDatabase(tenantId: $tenantId, name: $name) {
				tenantId
				name
				createdAtMs
			}
		}`,
		{ tenantId, name },
	);
	return tenantDatabaseFromControlGraphql(data.createTenantDatabase);
}

export async function deleteTenantDatabase(tenantId: string, name: string): Promise<void> {
	await controlGraphqlRequest<{ deleteTenantDatabase: { deleted: boolean } }>(
		`mutation AxonUiDeleteTenantDatabase($tenantId: String!, $name: String!) {
			deleteTenantDatabase(tenantId: $tenantId, name: $name) {
				deleted
			}
		}`,
		{ tenantId, name },
	);
}

// ── User provisioning (deployment-wide user rows) ────────────────────────────

export type User = {
	id: string;
	display_name: string;
	email: string | null;
	created_at_ms: number;
	suspended_at_ms: number | null;
};

export async function createUser(displayName: string, email: string | null): Promise<User> {
	const data = await controlGraphqlRequest<{ provisionUser: ControlGraphQLUser }>(
		`mutation AxonUiProvisionUser($displayName: String!, $email: String) {
			provisionUser(displayName: $displayName, email: $email) {
				id
				displayName
				email
				createdAtMs
				suspendedAtMs
			}
		}`,
		{ displayName, email },
	);
	return userFromControlGraphql(data.provisionUser);
}

export async function listUsers(): Promise<User[]> {
	const data = await controlGraphqlRequest<{ users: ControlGraphQLUser[] }>(
		`query AxonUiProvisionedUsers {
			users {
				id
				displayName
				email
				createdAtMs
				suspendedAtMs
			}
		}`,
	);
	return data.users.map(userFromControlGraphql);
}

export async function suspendUser(id: string): Promise<void> {
	await controlGraphqlRequest<{ suspendUser: { suspended: boolean } }>(
		`mutation AxonUiSuspendUser($userId: String!) {
			suspendUser(userId: $userId) {
				suspended
			}
		}`,
		{ userId: id },
	);
}

// ── Global user ACL (deployment-wide role assignments) ──────────────────────

export type UserRole = 'admin' | 'write' | 'read';

export type UserAclEntry = {
	login: string;
	role: UserRole;
};

export async function fetchUsers(): Promise<UserAclEntry[]> {
	const data = await controlGraphqlRequest<{ userRoles: ControlGraphQLUserAclEntry[] }>(
		`query AxonUiUserRoles {
			userRoles {
				login
				role
			}
		}`,
	);
	return data.userRoles;
}

export async function setUserRole(login: string, role: UserRole): Promise<UserAclEntry> {
	const data = await controlGraphqlRequest<{ setUserRole: ControlGraphQLUserAclEntry }>(
		`mutation AxonUiSetUserRole($login: String!, $role: String!) {
			setUserRole(login: $login, role: $role) {
				login
				role
			}
		}`,
		{ login, role },
	);
	return data.setUserRole;
}

export async function removeUserRole(login: string): Promise<void> {
	await controlGraphqlRequest<{ removeUserRole: { deleted: boolean } }>(
		`mutation AxonUiRemoveUserRole($login: String!) {
			removeUserRole(login: $login) {
				deleted
			}
		}`,
		{ login },
	);
}

// ── Tenant membership ────────────────────────────────────────────────────────

export type TenantMemberRole = 'admin' | 'write' | 'read';

export type TenantMember = {
	tenant_id: string;
	user_id: string;
	role: TenantMemberRole;
};

export async function fetchTenantMembers(tenantId: string): Promise<TenantMember[]> {
	const data = await controlGraphqlRequest<{ tenantMembers: ControlGraphQLTenantMember[] }>(
		`query AxonUiTenantMembers($tenantId: String!) {
			tenantMembers(tenantId: $tenantId) {
				tenantId
				userId
				role
			}
		}`,
		{ tenantId },
	);
	return data.tenantMembers.map(tenantMemberFromControlGraphql);
}

export async function upsertTenantMember(
	tenantId: string,
	userId: string,
	role: TenantMemberRole,
): Promise<TenantMember> {
	const data = await controlGraphqlRequest<{
		upsertTenantMember: ControlGraphQLTenantMember;
	}>(
		`mutation AxonUiUpsertTenantMember(
			$tenantId: String!
			$userId: String!
			$role: String!
		) {
			upsertTenantMember(tenantId: $tenantId, userId: $userId, role: $role) {
				tenantId
				userId
				role
			}
		}`,
		{ tenantId, userId, role },
	);
	return tenantMemberFromControlGraphql(data.upsertTenantMember);
}

export async function removeTenantMember(tenantId: string, userId: string): Promise<void> {
	await controlGraphqlRequest<{ removeTenantMember: { deleted: boolean } }>(
		`mutation AxonUiRemoveTenantMember($tenantId: String!, $userId: String!) {
			removeTenantMember(tenantId: $tenantId, userId: $userId) {
				deleted
			}
		}`,
		{ tenantId, userId },
	);
}

// ── Credential management ────────────────────────────────────────────────────

export type Credential = {
	jti: string;
	user_id: string;
	tenant_id: string;
	issued_at_ms: number;
	expires_at_ms: number;
	revoked: boolean;
	grants: Grants;
};

export type Grants = {
	databases: GrantedDatabase[];
};

export type GrantedDatabase = {
	name: string;
	ops: Array<'read' | 'write' | 'admin'>;
};

export type IssueCredentialRequest = {
	target_user: string;
	ttl_seconds: number;
	grants: Grants;
};

export type IssueCredentialResponse = {
	jwt: string;
	jti: string;
	expires_at_ms: number;
};

export async function listCredentials(tenantId: string): Promise<Credential[]> {
	const data = await controlGraphqlRequest<{ credentials: ControlGraphQLCredential[] }>(
		`query AxonUiCredentials($tenantId: String!) {
			credentials(tenantId: $tenantId) {
				jti
				userId
				tenantId
				issuedAtMs
				expiresAtMs
				revoked
				grants
			}
		}`,
		{ tenantId },
	);
	return data.credentials.map(credentialFromControlGraphql);
}

export async function issueCredential(
	tenantId: string,
	body: IssueCredentialRequest,
): Promise<IssueCredentialResponse> {
	const data = await controlGraphqlRequest<{
		issueCredential: { jwt: string; jti: string; expiresAt: number };
	}>(
		`mutation AxonUiIssueCredential(
			$tenantId: String!
			$targetUser: String!
			$ttlSeconds: Int!
			$grants: JSON!
		) {
			issueCredential(
				tenantId: $tenantId
				targetUser: $targetUser
				ttlSeconds: $ttlSeconds
				grants: $grants
			) {
				jwt
				jti
				expiresAt
			}
		}`,
		{
			tenantId,
			targetUser: body.target_user,
			ttlSeconds: body.ttl_seconds,
			grants: body.grants,
		},
	);
	return {
		jwt: data.issueCredential.jwt,
		jti: data.issueCredential.jti,
		expires_at_ms: data.issueCredential.expiresAt * 1000,
	};
}

export async function revokeCredential(tenantId: string, jti: string): Promise<void> {
	await controlGraphqlRequest<{ revokeCredential: { revoked: boolean } }>(
		`mutation AxonUiRevokeCredential($tenantId: String!, $jti: String!) {
			revokeCredential(tenantId: $tenantId, jti: $jti) {
				revoked
			}
		}`,
		{ tenantId, jti },
	);
}

// ── Transaction rollback ─────────────────────────────────────────────────────

export interface TransactionRollbackResult {
	transaction_id: string;
	entities_affected: number;
	entities_rolled_back: number;
	errors: string[];
	dry_run: boolean;
	details: unknown[];
}

export async function rollbackTransaction(
	transactionId: string,
	dryRun: boolean,
	scope: Scope,
): Promise<TransactionRollbackResult> {
	return request<TransactionRollbackResult>(
		`/transactions/${encodeURIComponent(transactionId)}/rollback`,
		{
			method: 'POST',
			body: JSON.stringify({ dry_run: dryRun }),
		},
		scope,
	);
}

// ── Audit revert ─────────────────────────────────────────────────────────────

export type RevertResult = {
	entity: EntityRecord;
	audit_entry_id: number;
};

export async function revertAuditEntry(auditEntryId: number, scope: Scope): Promise<RevertResult> {
	if (scope) {
		const data = await graphqlRequest<{ revertAuditEntry: GraphQLRevertAuditEntryPayload }>(
			scope,
			`mutation AxonUiRevertAuditEntry($auditEntryId: ID!) {
				revertAuditEntry(auditEntryId: $auditEntryId) {
					entity {
						collection
						id
						version
						data
					}
					auditEntry {
						id
						timestampNs
						collection
						entityId
						version
						mutation
						dataBefore
						dataAfter
						actor
						transactionId
						metadata
					}
				}
			}`,
			{ auditEntryId: String(auditEntryId) },
		);
		return {
			entity: entityFromGraphql(data.revertAuditEntry.entity),
			audit_entry_id: Number(data.revertAuditEntry.auditEntry.id),
		};
	}

	return request<RevertResult>(
		'/audit/revert',
		{
			method: 'POST',
			body: JSON.stringify({ audit_entry_id: auditEntryId, actor: 'ui' }),
		},
		scope,
	);
}

// ── Per-entity audit history ────────────────────────────────────────────────

export async function fetchEntityAudit(
	collection: string,
	id: string,
	scope?: Scope,
): Promise<AuditQueryResult> {
	if (scope) {
		const data = await graphqlRequest<{ auditLog: GraphQLAuditConnection }>(
			scope,
			`query AxonUiEntityAuditLog($collection: String!, $entityId: ID!) {
				auditLog(collection: $collection, entityId: $entityId) {
					edges {
						cursor
						node {
							id
							timestampNs
							collection
							entityId
							version
							mutation
							dataBefore
							dataAfter
							actor
							transactionId
							metadata
						}
					}
					pageInfo {
						hasNextPage
						endCursor
					}
				}
			}`,
			{ collection, entityId: id },
		);
		return auditResultFromGraphql(data.auditLog);
	}

	return request<AuditQueryResult>(
		`/audit/entity/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		undefined,
		scope,
	);
}

// ── Links ────────────────────────────────────────────────────────────────────

export type Link = {
	source_collection: string;
	source_id: string;
	target_collection: string;
	target_id: string;
	link_type: string;
	metadata?: Record<string, unknown>;
};

export type TraversePath = {
	source_collection: string;
	source_id: string;
	target_collection: string;
	target_id: string;
	link_type: string;
};

export type TraverseResult = {
	entities: EntityRecord[];
	paths?: TraversePath[];
};

/** Traverse outbound links from the given entity, optionally filtered by link_type. */
export async function traverseLinks(
	collection: string,
	id: string,
	options: { linkType?: string } = {},
	scope?: Scope,
): Promise<TraverseResult> {
	if (scope) {
		const data = await graphqlRequest<{ neighbors: GraphQLNeighborConnection }>(
			scope,
			`query AxonUiEntityNeighbors(
				$collection: String!
				$id: ID!
				$linkType: String
				$direction: String
			) {
				neighbors(
					collection: $collection
					id: $id
					linkType: $linkType
					direction: $direction
				) {
					groups {
						edges {
							node {
								collection
								id
								version
								data
							}
							linkType
							sourceCollection
							sourceId
							targetCollection
							targetId
						}
					}
				}
			}`,
			{
				collection,
				id,
				linkType: options.linkType ?? null,
				direction: 'outbound',
			},
		);
		const edges = data.neighbors.groups.flatMap((group) => group.edges);
		return {
			entities: edges.map((edge) => entityFromGraphql(edge.node)),
			paths: edges.map((edge) => ({
				source_collection: edge.sourceCollection,
				source_id: edge.sourceId,
				target_collection: edge.targetCollection,
				target_id: edge.targetId,
				link_type: edge.linkType,
			})),
		};
	}

	const qs = options.linkType ? `?link_type=${encodeURIComponent(options.linkType)}` : '';
	return request<TraverseResult>(
		`/traverse/${encodeURIComponent(collection)}/${encodeURIComponent(id)}${qs}`,
		undefined,
		scope,
	);
}

export async function createLink(body: Link, scope?: Scope): Promise<Link> {
	if (scope) {
		await graphqlRequest<{ createLink: boolean }>(
			scope,
			`mutation AxonUiCreateLink(
				$sourceCollection: String!
				$sourceId: ID!
				$targetCollection: String!
				$targetId: ID!
				$linkType: String!
				$metadata: String
			) {
				createLink(
					sourceCollection: $sourceCollection
					sourceId: $sourceId
					targetCollection: $targetCollection
					targetId: $targetId
					linkType: $linkType
					metadata: $metadata
				)
			}`,
			{
				sourceCollection: body.source_collection,
				sourceId: body.source_id,
				targetCollection: body.target_collection,
				targetId: body.target_id,
				linkType: body.link_type,
				metadata: body.metadata ? JSON.stringify(body.metadata) : null,
			},
		);
		return body;
	}

	const response = await request<{ link: Link }>(
		'/links',
		{
			method: 'POST',
			body: JSON.stringify({ ...body, actor: 'ui' }),
		},
		scope,
	);
	return response.link;
}

export async function deleteLink(body: Omit<Link, 'metadata'>, scope?: Scope): Promise<void> {
	if (scope) {
		await graphqlRequest<{ deleteLink: boolean }>(
			scope,
			`mutation AxonUiDeleteLink(
				$sourceCollection: String!
				$sourceId: ID!
				$targetCollection: String!
				$targetId: ID!
				$linkType: String!
			) {
				deleteLink(
					sourceCollection: $sourceCollection
					sourceId: $sourceId
					targetCollection: $targetCollection
					targetId: $targetId
					linkType: $linkType
				)
			}`,
			{
				sourceCollection: body.source_collection,
				sourceId: body.source_id,
				targetCollection: body.target_collection,
				targetId: body.target_id,
				linkType: body.link_type,
			},
		);
		return;
	}

	await request<void>(
		'/links',
		{
			method: 'DELETE',
			body: JSON.stringify({ ...body, actor: 'ui' }),
		},
		scope,
	);
}

// ── Markdown template CRUD ───────────────────────────────────────────────────

export type CollectionView = {
	collection: string;
	template: string;
	version: number;
	updated_at_ns?: number | null;
	updated_by?: string | null;
};

export async function fetchCollectionTemplate(
	collection: string,
	scope?: Scope,
): Promise<CollectionView> {
	if (scope) {
		const data = await graphqlRequest<{ collectionTemplate: GraphQLCollectionTemplate | null }>(
			scope,
			`query AxonUiCollectionTemplate($collection: String!) {
				collectionTemplate(collection: $collection) {
					collection
					template
					version
					updatedAtNs
					updatedBy
					warnings
				}
			}`,
			{ collection },
		);
		if (!data.collectionTemplate) {
			throw new Error(`not found: collection '${collection}' has no markdown template defined`);
		}
		return collectionTemplateFromGraphql(data.collectionTemplate);
	}

	return request<CollectionView>(
		`/collections/${encodeURIComponent(collection)}/template`,
		undefined,
		scope,
	);
}

export async function putCollectionTemplate(
	collection: string,
	template: string,
	scope?: Scope,
): Promise<CollectionView & { warnings?: string[] }> {
	if (scope) {
		const data = await graphqlRequest<{ putCollectionTemplate: GraphQLCollectionTemplate }>(
			scope,
			`mutation AxonUiPutCollectionTemplate($collection: String!, $template: String!) {
				putCollectionTemplate(input: { collection: $collection, template: $template }) {
					collection
					template
					version
					updatedAtNs
					updatedBy
					warnings
				}
			}`,
			{ collection, template },
		);
		return {
			...collectionTemplateFromGraphql(data.putCollectionTemplate),
			warnings: data.putCollectionTemplate.warnings ?? [],
		};
	}

	return request<CollectionView & { warnings?: string[] }>(
		`/collections/${encodeURIComponent(collection)}/template`,
		{
			method: 'PUT',
			body: JSON.stringify({ template }),
		},
		scope,
	);
}

export async function deleteCollectionTemplate(collection: string, scope?: Scope): Promise<void> {
	if (scope) {
		await graphqlRequest<{ deleteCollectionTemplate: { deleted: boolean } }>(
			scope,
			`mutation AxonUiDeleteCollectionTemplate($collection: String!) {
				deleteCollectionTemplate(collection: $collection) {
					deleted
				}
			}`,
			{ collection },
		);
		return;
	}

	await request<void>(
		`/collections/${encodeURIComponent(collection)}/template`,
		{ method: 'DELETE', body: JSON.stringify({}) },
		scope,
	);
}

/**
 * Fetch the rendered markdown for an entity. Returns the raw markdown
 * string as text/markdown from `?format=markdown` on the entity GET.
 */
export async function fetchRenderedEntity(
	collection: string,
	id: string,
	scope?: Scope,
): Promise<string> {
	if (scope) {
		const data = await graphqlRequest<{ renderedEntity: GraphQLRenderedEntity | null }>(
			scope,
			`query AxonUiRenderedEntity($collection: String!, $id: ID!) {
				renderedEntity(collection: $collection, id: $id) {
					markdown
					entity {
						collection
						id
						version
						data
					}
				}
			}`,
			{ collection, id },
		);
		if (!data.renderedEntity) {
			throw new Error(`not found: entity '${id}'`);
		}
		return data.renderedEntity.markdown;
	}

	const url = `/collections/${encodeURIComponent(collection)}/entities/${encodeURIComponent(id)}?format=markdown`;
	const response = await fetch(url, { headers: { Accept: 'text/markdown' } });
	if (!response.ok) {
		const text = await response.text();
		throw new Error(`rendered entity fetch failed (${response.status}): ${text}`);
	}
	return response.text();
}

// ── Lifecycle transitions ────────────────────────────────────────────────────

export type LifecycleDef = {
	field: string;
	initial: string;
	transitions: Record<string, string[]>;
};

export type TransitionLifecycleResponse = {
	entity: EntityRecord;
	audit_id?: number | null;
};

export async function transitionLifecycle(
	collection: string,
	id: string,
	body: {
		lifecycle_name: string;
		target_state: string;
		expected_version: number;
	},
	scope?: Scope,
): Promise<TransitionLifecycleResponse> {
	if (scope) {
		const typeName = pascalCase(collection);
		const data = await graphqlRequest<Record<string, GraphQLEntity>>(
			scope,
			`mutation AxonUiTransitionLifecycle(
				$id: ID!
				$lifecycleName: String!
				$targetState: String!
				$expectedVersion: Int!
			) {
				transition${typeName}Lifecycle(
					id: $id
					lifecycleName: $lifecycleName
					targetState: $targetState
					expectedVersion: $expectedVersion
				) {
					id
					version
					lifecycles
				}
			}`,
			{
				id,
				lifecycleName: body.lifecycle_name,
				targetState: body.target_state,
				expectedVersion: body.expected_version,
			},
		);
		const transitioned = data[`transition${typeName}Lifecycle`];
		if (!transitioned) {
			throw new Error(`GraphQL transition did not return entity: ${collection}/${id}`);
		}
		const entity = await fetchEntity(collection, id, scope);
		return { entity };
	}

	return request<TransitionLifecycleResponse>(
		`/lifecycle/${encodeURIComponent(collection)}/${encodeURIComponent(id)}/transition`,
		{
			method: 'POST',
			body: JSON.stringify({ ...body, actor: 'ui' }),
		},
		scope,
	);
}

/** Extract lifecycle definitions from a collection schema. */
export function lifecyclesFromSchema(
	schema: CollectionSchema | null | undefined,
): Record<string, LifecycleDef> {
	if (!schema) return {};
	const raw = (schema as unknown as Record<string, unknown>).lifecycles;
	if (!raw || typeof raw !== 'object') return {};
	return raw as Record<string, LifecycleDef>;
}

// ── Entity rollback ──────────────────────────────────────────────────────────

export type FieldDiff = {
	path: string;
	kind: string;
	description: string;
};

export type RollbackPreview = {
	current: EntityRecord | null;
	target: EntityRecord;
	diff: Record<string, FieldDiff>;
};

export type RollbackApplied = {
	entity: EntityRecord;
	audit_entry: AuditEntry;
};

export async function previewEntityRollback(
	collection: string,
	id: string,
	toVersion: number,
	scope?: Scope,
): Promise<RollbackPreview> {
	if (scope) {
		const data = await graphqlRequest<{ rollbackEntity: GraphQLRollbackEntityPayload }>(
			scope,
			`mutation AxonUiPreviewEntityRollback(
				$collection: String!
				$id: ID!
				$toVersion: Int!
			) {
				rollbackEntity(input: {
					collection: $collection
					id: $id
					toVersion: $toVersion
					dryRun: true
				}) {
					current {
						collection
						id
						version
						data
					}
					target {
						collection
						id
						version
						data
					}
					diff
				}
			}`,
			{ collection, id, toVersion },
		);
		return {
			current: data.rollbackEntity.current ? entityFromGraphql(data.rollbackEntity.current) : null,
			target: entityFromGraphql(data.rollbackEntity.target),
			diff: data.rollbackEntity.diff,
		};
	}

	return request<RollbackPreview>(
		`/collections/${encodeURIComponent(collection)}/entities/${encodeURIComponent(id)}/rollback`,
		{
			method: 'POST',
			body: JSON.stringify({ to_version: toVersion, actor: 'ui', dry_run: true }),
		},
		scope,
	);
}

export async function applyEntityRollback(
	collection: string,
	id: string,
	toVersion: number,
	expectedVersion: number,
	scope?: Scope,
): Promise<RollbackApplied> {
	if (scope) {
		const data = await graphqlRequest<{ rollbackEntity: GraphQLRollbackEntityPayload }>(
			scope,
			`mutation AxonUiApplyEntityRollback(
				$collection: String!
				$id: ID!
				$toVersion: Int!
				$expectedVersion: Int!
			) {
				rollbackEntity(input: {
					collection: $collection
					id: $id
					toVersion: $toVersion
					expectedVersion: $expectedVersion
					dryRun: false
				}) {
					entity {
						collection
						id
						version
						data
					}
					auditEntry {
						id
						timestampNs
						collection
						entityId
						version
						mutation
						dataBefore
						dataAfter
						actor
						transactionId
						metadata
					}
				}
			}`,
			{ collection, id, toVersion, expectedVersion },
		);
		if (!data.rollbackEntity.entity || !data.rollbackEntity.auditEntry) {
			throw new Error(`GraphQL rollback did not return applied entity: ${collection}/${id}`);
		}
		return {
			entity: entityFromGraphql(data.rollbackEntity.entity),
			audit_entry: auditEntryFromGraphql(data.rollbackEntity.auditEntry),
		};
	}

	return request<RollbackApplied>(
		`/collections/${encodeURIComponent(collection)}/entities/${encodeURIComponent(id)}/rollback`,
		{
			method: 'POST',
			body: JSON.stringify({
				to_version: toVersion,
				expected_version: expectedVersion,
				actor: 'ui',
				dry_run: false,
			}),
		},
		scope,
	);
}

const MUTATION_INTENT_FIELDS = `
	id
	tenantId
	databaseId
	subject
	schemaVersion
	policyVersion
	operation { operationKind operationHash operation }
	operationHash
	preImages { kind collection id version }
	decision
	approvalState
	approvalRoute { role reasonRequired deadlineSeconds separationOfDuties }
	expiresAtNs
	reviewSummary
`;

function mutationIntentErrorFromGraphql(error: GraphQLError | undefined): MutationIntentError {
	const code = error?.extensions?.code;
	return {
		message: error?.message ?? 'Mutation intent operation failed',
		stale: error?.extensions?.stale ?? [],
		...(code ? { code } : {}),
	};
}

export async function previewMutationIntent(
	scope: ScopedTenantDatabase,
	input: MutationPreviewInput,
): Promise<MutationPreviewResult> {
	const data = await graphqlRequest<{ previewMutation: MutationPreviewResult }>(
		scope,
		`mutation AxonUiPreviewMutationIntent($input: MutationPreviewInput!) {
			previewMutation(input: $input) {
				decision
				intentToken
				intent { ${MUTATION_INTENT_FIELDS} }
				canonicalOperation { operationKind operationHash operation }
				diff
				affectedRecords { kind collection id version }
				affectedFields
				approvalRoute { role reasonRequired deadlineSeconds separationOfDuties }
				policyExplanation
			}
		}`,
		{ input },
	);

	return data.previewMutation;
}

export async function commitMutationIntent(
	scope: ScopedTenantDatabase,
	input: { intentToken: string; intentId?: string; operation?: MutationIntentOperationInput },
): Promise<CommitMutationIntentOutcome> {
	const result = await graphqlRawRequest<{ commitMutationIntent: CommitMutationIntentResult }>(
		scope,
		`mutation AxonUiCommitMutationIntent($input: CommitIntentInput!) {
			commitMutationIntent(input: $input) {
				committed
				errorCode
				stale { dimension expected actual path }
				transactionId
				intent { ${MUTATION_INTENT_FIELDS} }
			}
		}`,
		{ input },
	);

	if (result.errors?.length) {
		return { ok: false, error: mutationIntentErrorFromGraphql(result.errors[0]) };
	}

	const committed = result.data?.commitMutationIntent;
	if (!committed) {
		return {
			ok: false,
			error: { message: 'GraphQL response missing commitMutationIntent', stale: [] },
		};
	}

	return { ok: true, result: committed };
}

export async function fetchMutationIntents(
	scope: ScopedTenantDatabase,
	input: MutationIntentListInput = {},
): Promise<MutationIntentConnection> {
	const data = await graphqlRequest<{ pendingMutationIntents: MutationIntentConnection }>(
		scope,
		`query AxonUiMutationIntents($filter: MutationIntentFilter, $limit: Int, $after: String) {
			pendingMutationIntents(filter: $filter, limit: $limit, after: $after) {
				totalCount
				edges {
					cursor
					node { ${MUTATION_INTENT_FIELDS} }
				}
				pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
			}
		}`,
		{
			filter: input.filter ?? {},
			limit: input.limit ?? 25,
			after: input.after ?? null,
		},
	);

	return data.pendingMutationIntents;
}

export async function fetchMutationIntent(
	scope: ScopedTenantDatabase,
	id: string,
): Promise<MutationIntent | null> {
	const data = await graphqlRequest<{ mutationIntent: MutationIntent | null }>(
		scope,
		`query AxonUiMutationIntent($id: ID!) {
			mutationIntent(id: $id) { ${MUTATION_INTENT_FIELDS} }
		}`,
		{ id },
	);

	return data.mutationIntent;
}

export async function approveMutationIntent(
	scope: ScopedTenantDatabase,
	input: { intentId: string; reason?: string },
): Promise<MutationIntent> {
	const data = await graphqlRequest<{ approveMutationIntent: MutationIntent }>(
		scope,
		`mutation AxonUiApproveMutationIntent($input: ApproveIntentInput!) {
			approveMutationIntent(input: $input) { ${MUTATION_INTENT_FIELDS} }
		}`,
		{ input },
	);

	return data.approveMutationIntent;
}

export async function rejectMutationIntent(
	scope: ScopedTenantDatabase,
	input: { intentId: string; reason: string },
): Promise<MutationIntent> {
	const data = await graphqlRequest<{ rejectMutationIntent: MutationIntent }>(
		scope,
		`mutation AxonUiRejectMutationIntent($input: RejectIntentInput!) {
			rejectMutationIntent(input: $input) { ${MUTATION_INTENT_FIELDS} }
		}`,
		{ input },
	);

	return data.rejectMutationIntent;
}

// ── Raw GraphQL passthrough for the playground page ─────────────────────────

export type GraphQLResponse<T = unknown> = {
	data?: T;
	errors?: Array<{
		message: string;
		path?: (string | number)[];
		extensions?: Record<string, unknown>;
	}>;
};

export async function executeGraphql(
	query: string,
	variables: Record<string, unknown> | undefined,
	scope: { tenant: string; database: string },
): Promise<GraphQLResponse> {
	const response = await fetch(scopedPath('/graphql', scope), {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ query, variables: variables ?? {} }),
	});
	const text = await response.text();
	try {
		return JSON.parse(text) as GraphQLResponse;
	} catch {
		throw new Error(`GraphQL response was not JSON (${response.status}): ${text.slice(0, 200)}`);
	}
}
