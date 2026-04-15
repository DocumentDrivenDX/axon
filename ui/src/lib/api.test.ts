import { afterEach, beforeEach, expect, test } from 'bun:test';

import { fetchAudit, fetchCollections, issueCredential, listCredentials, revokeCredential } from './api';

const originalFetch = globalThis.fetch;

type CapturedRequest = {
	url: string;
	init: RequestInit | undefined;
};

let lastRequest: CapturedRequest | null = null;

function mockFetch(body: unknown, status = 200) {
	const handler = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
		lastRequest = { url: String(input), init };
		return new Response(JSON.stringify(body), {
			status,
			headers: { 'Content-Type': 'application/json' },
		});
	};
	globalThis.fetch = handler as unknown as typeof globalThis.fetch;
}

beforeEach(() => {
	lastRequest = null;
});

afterEach(() => {
	globalThis.fetch = originalFetch;
});

test('request() prefixes URL with tenant/database path when scope is provided', async () => {
	mockFetch({ collections: [] });

	await fetchCollections({ tenant: 'acme', database: 'orders' });

	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/collections');
});

test('request() URL-encodes tenant and database in the path', async () => {
	mockFetch({ collections: [] });

	await fetchCollections({ tenant: 'my tenant', database: 'my/db' });

	expect(lastRequest?.url).toBe('/tenants/my%20tenant/databases/my%2Fdb/collections');
});

test('request() does NOT prefix control-plane routes even when scope is provided', async () => {
	mockFetch({ entries: [], next_cursor: null });

	// fetchAudit uses /audit/query which is NOT a /control/ route, so it gets prefixed.
	// Use a raw fetch mock to test /control/ directly.
	const controlHandler = async (
		input: RequestInfo | URL,
		init?: RequestInit,
	): Promise<Response> => {
		lastRequest = { url: String(input), init };
		return new Response(JSON.stringify({ tenants: [] }), {
			status: 200,
			headers: { 'Content-Type': 'application/json' },
		});
	};
	globalThis.fetch = controlHandler as unknown as typeof globalThis.fetch;

	// Import fetchTenants which calls /control/tenants — no scope param.
	const { fetchTenants } = await import('./api');
	await fetchTenants();

	expect(lastRequest?.url).toBe('/control/tenants');
	// Confirm no scope prefix was added despite the /control/ path.
	expect(lastRequest?.url).not.toContain('/tenants/');
});

test('request() does not set X-Axon-Database header', async () => {
	mockFetch({ collections: [] });

	await fetchCollections({ tenant: 'acme', database: 'orders' });

	const headers = new Headers(lastRequest?.init?.headers);
	expect(headers.has('X-Axon-Database')).toBe(false);
});

test('request() uses plain path when no scope is provided', async () => {
	mockFetch({ collections: [] });

	await fetchCollections();

	expect(lastRequest?.url).toBe('/collections');
});

test('request() prefixes audit route with scope', async () => {
	mockFetch({ entries: [], next_cursor: null });

	await fetchAudit({}, { tenant: 'acme', database: 'orders' });

	expect(lastRequest?.url).toBe('/tenants/acme/databases/orders/audit/query');
});

// ── Credential API helpers ────────────────────────────────────────────────────

test('listCredentials() calls GET /control/tenants/:id/credentials', async () => {
	mockFetch({ credentials: [] });

	await listCredentials('tenant-123');

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials');
	// GET is the default — no explicit method set
	expect(lastRequest?.init?.method).toBeUndefined();
});

test('listCredentials() URL-encodes tenant ID', async () => {
	mockFetch({ credentials: [] });

	await listCredentials('my tenant');

	expect(lastRequest?.url).toBe('/control/tenants/my%20tenant/credentials');
});

test('issueCredential() calls POST /control/tenants/:id/credentials', async () => {
	mockFetch({ jwt: 'eyJ...', jti: 'abc-jti', expires_at_ms: 9999999 });

	await issueCredential('tenant-123', {
		target_user: 'user-uuid',
		ttl_seconds: 3600,
		grants: { databases: [] },
	});

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials');
	expect(lastRequest?.init?.method).toBe('POST');
});

test('issueCredential() URL-encodes tenant ID', async () => {
	mockFetch({ jwt: 'eyJ...', jti: 'abc-jti', expires_at_ms: 9999999 });

	await issueCredential('my tenant', {
		target_user: 'user-uuid',
		ttl_seconds: 3600,
		grants: { databases: [] },
	});

	expect(lastRequest?.url).toBe('/control/tenants/my%20tenant/credentials');
});

test('revokeCredential() calls DELETE /control/tenants/:id/credentials/:jti', async () => {
	mockFetch({});

	await revokeCredential('tenant-123', 'jti-abc');

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials/jti-abc');
	expect(lastRequest?.init?.method).toBe('DELETE');
});

test('revokeCredential() URL-encodes jti', async () => {
	mockFetch({});

	await revokeCredential('tenant-123', 'jti/with/slash');

	expect(lastRequest?.url).toBe('/control/tenants/tenant-123/credentials/jti%2Fwith%2Fslash');
});
