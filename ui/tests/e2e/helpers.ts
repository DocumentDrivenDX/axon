/**
 * Shared helpers for Axon e2e tests running against the live HTTPS server.
 *
 * All helpers accept a Playwright `request` fixture so HTTPS errors and
 * auth headers are inherited from the project config. Tests should use
 * these to provision tenants/databases/collections/entities instead of
 * going through the UI — keeps the UI tests focused on UI behavior.
 */
import type { APIRequestContext, Page } from '@playwright/test';
import { expect } from '@playwright/test';

export type TestTenant = {
	id: string;
	name: string;
	db_name: string;
};

export type TestDatabase = {
	tenant: TestTenant;
	name: string;
};

/** Create a tenant with a unique name. */
export async function createTestTenant(
	request: APIRequestContext,
	prefix: string,
): Promise<TestTenant> {
	const name = `${prefix}${Date.now().toString(36)}`;
	const response = await request.post('/control/tenants', {
		data: { name },
	});
	expect(response.ok(), `create tenant ${name}`).toBe(true);
	return (await response.json()) as TestTenant;
}

/** Create a database under a tenant. */
export async function createTestDatabase(
	request: APIRequestContext,
	tenant: TestTenant,
	dbName = 'default',
): Promise<TestDatabase> {
	const response = await request.post(
		`/control/tenants/${encodeURIComponent(tenant.id)}/databases`,
		{ data: { name: dbName } },
	);
	expect(response.ok(), `create database ${dbName}`).toBe(true);
	return { tenant, name: dbName };
}

/** Create a collection on a database. Pass an optional schema. */
export async function createTestCollection(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	schema: {
		entity_schema?: Record<string, unknown> | null;
		lifecycles?: Record<string, unknown>;
		link_types?: Record<string, unknown>;
	} = {},
): Promise<void> {
	const url = `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/collections/${encodeURIComponent(collection)}`;
	const response = await request.post(url, {
		data: {
			schema: {
				description: null,
				version: 1,
				entity_schema: schema.entity_schema ?? null,
				link_types: schema.link_types ?? {},
				lifecycles: schema.lifecycles ?? undefined,
			},
			actor: 'e2e',
		},
	});
	expect(response.ok(), `create collection ${collection}`).toBe(true);
}

/** Create an entity in a collection. */
export async function createTestEntity(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	id: string,
	data: Record<string, unknown>,
): Promise<void> {
	const url = `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`;
	const response = await request.post(url, {
		data: { data, actor: 'e2e' },
	});
	expect(response.ok(), `create entity ${id}`).toBe(true);
}

/** Update an entity in a collection (PATCH/PUT). */
export async function updateTestEntity(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	id: string,
	data: Record<string, unknown>,
	expectedVersion: number,
): Promise<void> {
	const url = `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`;
	const response = await request.put(url, {
		data: { data, expected_version: expectedVersion, actor: 'e2e' },
	});
	expect(response.ok(), `update entity ${id}`).toBe(true);
}

/** Build a UI URL for the database collections page. */
export function dbCollectionsUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/collections`;
}

export function dbCollectionUrl(db: TestDatabase, collection: string): string {
	return `${dbCollectionsUrl(db)}/${encodeURIComponent(collection)}`;
}

export function dbGraphqlUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/graphql`;
}

export function tenantUrl(tenant: TestTenant, section: 'members' | 'credentials' | '' = ''): string {
	const base = `/ui/tenants/${encodeURIComponent(tenant.db_name)}`;
	return section ? `${base}/${section}` : base;
}

/** Click the first row in the entity list to select it. */
export async function selectFirstEntity(page: Page): Promise<void> {
	const firstRow = page.locator('table tr').nth(1);
	await firstRow.click();
}

export type TestUser = {
	id: string;
	display_name: string;
	email: string | null;
	created_at_ms: number;
	suspended_at_ms: number | null;
};

/** Provision a user row via POST /control/users/provision. */
export async function createTestUser(
	request: APIRequestContext,
	displayName?: string,
	email?: string | null,
): Promise<TestUser> {
	const name = displayName ?? `test-${Date.now().toString(36)}`;
	const response = await request.post('/control/users/provision', {
		data: { display_name: name, email: email ?? null },
	});
	expect(response.ok(), `create user ${name}`).toBe(true);
	return (await response.json()) as TestUser;
}
