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

type AuditFilters = {
	collection?: string;
	actor?: string;
	sinceNs?: string;
	untilNs?: string;
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
		throw new Error(result.errors.map((error) => error.message).join(', '));
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
	const response = await request<{ tenants: Tenant[] }>('/control/tenants');
	return response.tenants;
}

export async function createTenant(name: string): Promise<Tenant> {
	return request<Tenant>('/control/tenants', {
		method: 'POST',
		body: JSON.stringify({ name }),
	});
}

export async function deleteTenant(tenantId: string): Promise<void> {
	await request<void>(`/control/tenants/${encodeURIComponent(tenantId)}`, {
		method: 'DELETE',
	});
}

export async function fetchTenant(tenantId: string): Promise<Tenant> {
	return request<Tenant>(`/control/tenants/${encodeURIComponent(tenantId)}`);
}

export async function fetchTenantDatabases(tenantId: string): Promise<TenantDatabase[]> {
	const response = await request<{ databases: TenantDatabase[] }>(
		`/control/tenants/${encodeURIComponent(tenantId)}/databases`,
	);
	return response.databases;
}

export async function createTenantDatabase(
	tenantId: string,
	name: string,
): Promise<TenantDatabase> {
	return request<TenantDatabase>(`/control/tenants/${encodeURIComponent(tenantId)}/databases`, {
		method: 'POST',
		body: JSON.stringify({ name }),
	});
}

export async function deleteTenantDatabase(tenantId: string, name: string): Promise<void> {
	await request<void>(
		`/control/tenants/${encodeURIComponent(tenantId)}/databases/${encodeURIComponent(name)}`,
		{ method: 'DELETE' },
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
	return request<User>('/control/users/provision', {
		method: 'POST',
		body: JSON.stringify({ display_name: displayName, email }),
	});
}

export async function listUsers(): Promise<User[]> {
	const response = await request<{ users: User[] }>('/control/users/list');
	return response.users;
}

export async function suspendUser(id: string): Promise<void> {
	await request<void>(`/control/users/suspend/${encodeURIComponent(id)}`, {
		method: 'DELETE',
	});
}

// ── Global user ACL (deployment-wide role assignments) ──────────────────────

export type UserRole = 'admin' | 'write' | 'read';

export type UserAclEntry = {
	login: string;
	role: UserRole;
};

export async function fetchUsers(): Promise<UserAclEntry[]> {
	const response = await request<{ users: UserAclEntry[] }>('/control/users');
	return response.users;
}

export async function setUserRole(login: string, role: UserRole): Promise<UserAclEntry> {
	return request<UserAclEntry>(`/control/users/${encodeURIComponent(login)}`, {
		method: 'PUT',
		body: JSON.stringify({ role }),
	});
}

export async function removeUserRole(login: string): Promise<void> {
	await request<void>(`/control/users/${encodeURIComponent(login)}`, {
		method: 'DELETE',
	});
}

// ── Tenant membership ────────────────────────────────────────────────────────

export type TenantMemberRole = 'admin' | 'write' | 'read';

export type TenantMember = {
	tenant_id: string;
	user_id: string;
	role: TenantMemberRole;
};

export async function fetchTenantMembers(tenantId: string): Promise<TenantMember[]> {
	const response = await request<{ members: TenantMember[] }>(
		`/control/tenants/${encodeURIComponent(tenantId)}/members`,
	);
	return response.members;
}

export async function upsertTenantMember(
	tenantId: string,
	userId: string,
	role: TenantMemberRole,
): Promise<TenantMember> {
	return request<TenantMember>(
		`/control/tenants/${encodeURIComponent(tenantId)}/members/${encodeURIComponent(userId)}`,
		{
			method: 'PUT',
			body: JSON.stringify({ role }),
		},
	);
}

export async function removeTenantMember(tenantId: string, userId: string): Promise<void> {
	await request<void>(
		`/control/tenants/${encodeURIComponent(tenantId)}/members/${encodeURIComponent(userId)}`,
		{ method: 'DELETE' },
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
	const response = await request<{ credentials: Credential[] }>(
		`/control/tenants/${encodeURIComponent(tenantId)}/credentials`,
	);
	return response.credentials;
}

export async function issueCredential(
	tenantId: string,
	body: IssueCredentialRequest,
): Promise<IssueCredentialResponse> {
	return request<IssueCredentialResponse>(
		`/control/tenants/${encodeURIComponent(tenantId)}/credentials`,
		{ method: 'POST', body: JSON.stringify(body) },
	);
}

export async function revokeCredential(tenantId: string, jti: string): Promise<void> {
	await request<void>(
		`/control/tenants/${encodeURIComponent(tenantId)}/credentials/${encodeURIComponent(jti)}`,
		{ method: 'DELETE' },
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
	const qs = options.linkType ? `?link_type=${encodeURIComponent(options.linkType)}` : '';
	return request<TraverseResult>(
		`/traverse/${encodeURIComponent(collection)}/${encodeURIComponent(id)}${qs}`,
		undefined,
		scope,
	);
}

export async function createLink(body: Link, scope?: Scope): Promise<Link> {
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
	const base = scope && { tenant: scope.tenant, database: scope.database }
		? `/tenants/${encodeURIComponent(scope.tenant)}/databases/${encodeURIComponent(scope.database)}`
		: '';
	const url = `${base}/collections/${encodeURIComponent(collection)}/entities/${encodeURIComponent(id)}?format=markdown`;
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

// ── Raw GraphQL passthrough for the playground page ─────────────────────────

export type GraphQLResponse<T = unknown> = {
	data?: T;
	errors?: Array<{ message: string; path?: (string | number)[] }>;
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
