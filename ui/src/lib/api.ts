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

type QueryEntitiesInput = {
	limit?: number;
	afterId?: string | null;
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

async function request<T>(path: string, init?: RequestInit, dbName?: string | null): Promise<T> {
	const headers = new Headers(init?.headers);
	if (init?.body && !headers.has('Content-Type')) {
		headers.set('Content-Type', 'application/json');
	}
	if (dbName) {
		headers.set('X-Axon-Database', dbName);
	}

	const response = await fetch(path, {
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

export async function fetchCollections(dbName?: string | null): Promise<CollectionSummary[]> {
	const response = await request<{ collections: CollectionSummary[] }>('/collections', undefined, dbName);
	return response.collections;
}

export async function fetchCollection(name: string, dbName?: string | null): Promise<CollectionDetail> {
	return request<CollectionDetail>(`/collections/${encodeURIComponent(name)}`, undefined, dbName);
}

export async function fetchEntities(
	collection: string,
	options: QueryEntitiesInput = {},
	dbName?: string | null,
): Promise<QueryEntitiesResult> {
	return request<QueryEntitiesResult>(`/collections/${encodeURIComponent(collection)}/query`, {
		method: 'POST',
		body: JSON.stringify({
			limit: options.limit ?? 50,
			after_id: options.afterId ?? null,
		}),
	}, dbName);
}

export async function fetchEntity(collection: string, id: string, dbName?: string | null): Promise<EntityRecord> {
	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		undefined,
		dbName,
	);
	return response.entity;
}

export async function createEntity(
	collection: string,
	id: string,
	data: Record<string, unknown>,
	dbName?: string | null,
): Promise<EntityRecord> {
	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		{
			method: 'POST',
			body: JSON.stringify({ data, actor: 'ui' }),
		},
		dbName,
	);
	return response.entity;
}

export async function updateEntity(
	collection: string,
	id: string,
	data: Record<string, unknown>,
	expectedVersion: number,
	dbName?: string | null,
): Promise<EntityRecord> {
	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		{
			method: 'PUT',
			body: JSON.stringify({ data, expected_version: expectedVersion, actor: 'ui' }),
		},
		dbName,
	);
	return response.entity;
}

export async function fetchSchema(collection: string, dbName?: string | null): Promise<CollectionSchema> {
	const response = await request<{ schema: CollectionSchema }>(
		`/collections/${encodeURIComponent(collection)}/schema`,
		undefined,
		dbName,
	);
	return response.schema;
}

export async function updateSchema(
	collection: string,
	schema: CollectionSchema,
	options?: { force?: boolean },
	dbName?: string | null,
): Promise<CollectionSchema> {
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
		dbName,
	);

	return response.schema;
}

export async function previewSchemaChange(
	collection: string,
	schema: CollectionSchema,
	dbName?: string | null,
): Promise<SchemaPreviewResult> {
	return request<SchemaPreviewResult>(`/collections/${encodeURIComponent(collection)}/schema`, {
		method: 'PUT',
		body: JSON.stringify({
			description: schema.description ?? null,
			version: schema.version,
			entity_schema: schema.entity_schema ?? null,
			link_types: schema.link_types ?? {},
			actor: 'ui',
			dry_run: true,
		}),
	}, dbName);
}

export async function createCollection(
	name: string,
	schema: Omit<CollectionSchema, 'collection'>,
	dbName?: string | null,
): Promise<void> {
	await request<{ name: string }>(`/collections/${encodeURIComponent(name)}`, {
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
	}, dbName);
}

export async function fetchAudit(filters: AuditFilters = {}, dbName?: string | null): Promise<AuditQueryResult> {
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
	return request<AuditQueryResult>(`/audit/query${query ? `?${query}` : ''}`, undefined, dbName);
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

export async function dropCollection(name: string, dbName?: string | null): Promise<void> {
	await request<void>(`/collections/${encodeURIComponent(name)}`, {
		method: 'DELETE',
		body: JSON.stringify({ actor: 'ui' }),
	}, dbName);
}

export async function deleteEntity(collection: string, id: string, dbName?: string | null): Promise<void> {
	await request<void>(`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`, {
		method: 'DELETE',
		body: JSON.stringify({ actor: 'ui' }),
	}, dbName);
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
