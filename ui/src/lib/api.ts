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

async function request<T>(path: string, init?: RequestInit): Promise<T> {
	const headers = new Headers(init?.headers);
	if (init?.body && !headers.has('Content-Type')) {
		headers.set('Content-Type', 'application/json');
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

export async function fetchCollections(): Promise<CollectionSummary[]> {
	const response = await request<{ collections: CollectionSummary[] }>('/collections');
	return response.collections;
}

export async function fetchCollection(name: string): Promise<CollectionDetail> {
	return request<CollectionDetail>(`/collections/${encodeURIComponent(name)}`);
}

export async function fetchEntities(
	collection: string,
	options: QueryEntitiesInput = {},
): Promise<QueryEntitiesResult> {
	return request<QueryEntitiesResult>(`/collections/${encodeURIComponent(collection)}/query`, {
		method: 'POST',
		body: JSON.stringify({
			limit: options.limit ?? 50,
			after_id: options.afterId ?? null,
		}),
	});
}

export async function fetchEntity(collection: string, id: string): Promise<EntityRecord> {
	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
	);
	return response.entity;
}

export async function createEntity(
	collection: string,
	id: string,
	data: Record<string, unknown>,
): Promise<EntityRecord> {
	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		{
			method: 'POST',
			body: JSON.stringify({ data, actor: 'ui' }),
		},
	);
	return response.entity;
}

export async function updateEntity(
	collection: string,
	id: string,
	data: Record<string, unknown>,
	expectedVersion: number,
): Promise<EntityRecord> {
	const response = await request<{ entity: EntityRecord }>(
		`/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`,
		{
			method: 'PUT',
			body: JSON.stringify({ data, expected_version: expectedVersion, actor: 'ui' }),
		},
	);
	return response.entity;
}

export async function fetchSchema(collection: string): Promise<CollectionSchema> {
	const response = await request<{ schema: CollectionSchema }>(
		`/collections/${encodeURIComponent(collection)}/schema`,
	);
	return response.schema;
}

export async function updateSchema(
	collection: string,
	schema: CollectionSchema,
	options?: { force?: boolean },
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
	);

	return response.schema;
}

export async function previewSchemaChange(
	collection: string,
	schema: CollectionSchema,
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
	});
}

export async function createCollection(
	name: string,
	schema: Omit<CollectionSchema, 'collection'>,
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
	});
}

export async function fetchAudit(filters: AuditFilters = {}): Promise<AuditQueryResult> {
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
	return request<AuditQueryResult>(`/audit/query${query ? `?${query}` : ''}`);
}

export async function fetchHealth(): Promise<HealthStatus> {
	return request<HealthStatus>('/health');
}
