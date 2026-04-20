/**
 * Shared helpers for Axon e2e tests running against the live HTTPS server.
 *
 * All helpers accept a Playwright `request` fixture so HTTPS errors and
 * auth headers are inherited from the project config. Tests should use
 * these to provision tenants/databases/collections/entities instead of
 * going through the UI — keeps the UI tests focused on UI behavior.
 */
import type { APIRequestContext, APIResponse, Page } from '@playwright/test';
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

export const E2E_FIXTURE_PREFIX = 'e2e-';

function withE2eFixturePrefix(value: string): string {
	return value.startsWith(E2E_FIXTURE_PREFIX) ? value : `${E2E_FIXTURE_PREFIX}${value}`;
}

async function expectOkResponse(response: APIResponse, label: string) {
	if (response.ok()) return;
	let body = '';
	try {
		body = await response.text();
	} catch {
		body = '<unreadable response body>';
	}
	expect(response.ok(), `${label}: ${response.status()} ${body}`).toBe(true);
}

/** Create a tenant with a unique name. */
export async function createTestTenant(
	request: APIRequestContext,
	prefix: string,
): Promise<TestTenant> {
	const suffix = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
	const name = `${withE2eFixturePrefix(prefix)}-${suffix}`;
	const response = await request.post('/control/tenants', {
		data: { name },
	});
	await expectOkResponse(response, `create tenant ${name}`);
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
	await expectOkResponse(response, `create database ${dbName}`);
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
	await expectOkResponse(response, `create collection ${collection}`);
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
	await expectOkResponse(response, `create entity ${id}`);
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
	await expectOkResponse(response, `update entity ${id}`);
}

/** Build a UI URL for the database collections page. */
export function dbCollectionsUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/collections`;
}

export function dbCollectionUrl(db: TestDatabase, collection: string): string {
	return `${dbCollectionsUrl(db)}/${encodeURIComponent(collection)}`;
}

export function dbOverviewUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}`;
}

export function dbGraphqlUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/graphql`;
}

export function dbAuditUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/audit`;
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
	const name = withE2eFixturePrefix(displayName ?? `test-${Date.now().toString(36)}`);
	const response = await request.post('/control/users/provision', {
		data: { display_name: name, email: email ?? null },
	});
	await expectOkResponse(response, `create user ${name}`);
	return (await response.json()) as TestUser;
}

export async function addTestTenantMember(
	request: APIRequestContext,
	tenant: TestTenant,
	user: TestUser,
	role: 'admin' | 'write' | 'read' = 'read',
): Promise<void> {
	const response = await request.put(
		`/control/tenants/${encodeURIComponent(tenant.id)}/members/${encodeURIComponent(user.id)}`,
		{ data: { role } },
	);
	await expectOkResponse(response, `add member ${user.id} to ${tenant.id}`);
}
