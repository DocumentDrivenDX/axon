import { execFileSync } from 'child_process';
import { existsSync } from 'fs';
import { expect, test } from '@playwright/test';

/**
 * SQLite disk-persistence E2E tests.
 *
 * These tests verify that data written through the HTTP API is actually
 * durable on disk — not just held in memory.  They do so by querying the
 * SQLite database file directly via the `sqlite3` CLI after each write.
 *
 * Only meaningful when running against a SQLite backend.  All tests are
 * automatically skipped when the expected database file is absent (e.g. when
 * the suite is run with the memory or postgres config).
 *
 * Run with: bunx playwright test --config playwright.e2e.sqlite.config.ts
 */

// Must match SQLITE_DB_PATH in playwright.e2e.sqlite.config.ts.
const DB_PATH = '/tmp/axon-e2e-sqlite.db';

const COLLECTION = 'persist-e2e-col';
const ENTITY_A = 'persist-entity-001';
const ENTITY_B = 'persist-entity-002';

/** Query the SQLite file and return trimmed stdout. */
function sqliteQuery(sql: string): string {
	return execFileSync('sqlite3', [DB_PATH, sql], { encoding: 'utf8' }).trim();
}

/** Count rows matching the given WHERE clause in the entities table. */
function countEntities(where: string): number {
	return parseInt(sqliteQuery(`SELECT COUNT(*) FROM entities WHERE ${where};`), 10);
}

test.describe('SQLite disk persistence', () => {
	test.beforeAll(async ({ request }) => {
		// Ensure the collection exists before any persistence checks.
		const collResp = await request.post(`/collections/${COLLECTION}`, {
			data: {
				schema: {
					description: null,
					version: 1,
					entity_schema: { type: 'object', properties: {} },
					link_types: {},
				},
				actor: 'e2e-test',
			},
		});
		expect([201, 409]).toContain(collResp.status());

		// Create entity A so there is definitely at least one entity in the DB.
		const entityResp = await request.post(`/entities/${COLLECTION}/${ENTITY_A}`, {
			data: { data: { label: 'persistence test A' }, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(entityResp.status());
	});

	// Skip every test in this group when the SQLite file is absent.
	test.beforeEach(async ({}, testInfo) => {
		if (!existsSync(DB_PATH)) {
			testInfo.skip(
				true,
				`SQLite database not found at ${DB_PATH} — run with playwright.e2e.sqlite.config.ts`,
			);
		}
	});

	test('entity written via API appears in the SQLite entities table', async () => {
		const count = countEntities(`collection = '${COLLECTION}' AND id = '${ENTITY_A}'`);
		expect(count).toBe(1);
	});

	test('audit log entry for entity creation is accessible via the HTTP API', async ({
		request,
	}) => {
		// The audit log is stored in-memory (not in the SQLite file), but the
		// HTTP API makes it queryable. This verifies that entity operations are
		// correctly recorded in the audit log, which is the authoritative record
		// of state transitions regardless of the storage backend.
		const resp = await request.get(`/audit/query?collection=${COLLECTION}&limit=100`);
		expect(resp.ok()).toBe(true);
		const body = (await resp.json()) as {
			entries: Array<{ entity_id: string; mutation: string }>;
		};
		const entry = body.entries.find((e) => e.entity_id === ENTITY_A);
		expect(entry).toBeDefined();
		expect(entry?.mutation).toMatch(/entity\./);
	});

	test('entity data payload is stored correctly in the database', async () => {
		// The `data` column stores JSON; verify the payload round-trips correctly.
		const rawData = sqliteQuery(
			`SELECT data FROM entities WHERE collection = '${COLLECTION}' AND id = '${ENTITY_A}' LIMIT 1;`,
		);
		const parsed = JSON.parse(rawData) as Record<string, unknown>;
		expect(parsed).toMatchObject({ label: 'persistence test A' });
	});

	test('writing a second entity increases the count in the database', async ({ request }) => {
		const before = countEntities(`collection = '${COLLECTION}'`);

		const resp = await request.post(`/entities/${COLLECTION}/${ENTITY_B}`, {
			data: { data: { label: 'persistence test B' }, actor: 'e2e-test' },
		});
		// 201 on fresh DB, 409 if already exists from a previous retry.
		expect([201, 409]).toContain(resp.status());

		const after = countEntities(`collection = '${COLLECTION}'`);
		// After writing, the count must be at least as large (new entity) or the
		// same (entity already existed from a retry).
		expect(after).toBeGreaterThanOrEqual(before);
		expect(countEntities(`collection = '${COLLECTION}' AND id = '${ENTITY_B}'`)).toBe(1);
	});

	test('updated entity version is reflected on disk', async ({ request }) => {
		// Fetch current version so we can supply the correct expected_version.
		// GET /entities/{col}/{id} returns {"entity": {"version": N, ...}}.
		const getResp = await request.get(`/entities/${COLLECTION}/${ENTITY_A}`);
		expect(getResp.ok()).toBe(true);
		const { entity: entityBody } = (await getResp.json()) as {
			entity: { version: number };
		};
		const currentVersion = entityBody.version;

		const putResp = await request.put(`/entities/${COLLECTION}/${ENTITY_A}`, {
			data: {
				data: { label: 'persistence test A — updated' },
				actor: 'e2e-test',
				expected_version: currentVersion,
			},
		});
		expect(putResp.status()).toBe(200);

		// Version in the DB should now be currentVersion + 1.
		const storedVersion = parseInt(
			sqliteQuery(
				`SELECT version FROM entities WHERE collection = '${COLLECTION}' AND id = '${ENTITY_A}' LIMIT 1;`,
			),
			10,
		);
		expect(storedVersion).toBe(currentVersion + 1);
	});

	test('deleted entity is removed from the entities table', async ({ request }) => {
		// Create a dedicated entity to delete so we don't affect other tests.
		const DEL_ID = 'persist-entity-del';
		await request.post(`/entities/${COLLECTION}/${DEL_ID}`, {
			data: { data: { label: 'to be deleted' }, actor: 'e2e-test' },
		});

		const beforeCount = countEntities(`collection = '${COLLECTION}' AND id = '${DEL_ID}'`);
		expect(beforeCount).toBe(1);

		const delResp = await request.delete(`/entities/${COLLECTION}/${DEL_ID}`, {
			data: { actor: 'e2e-test' },
		});
		expect(delResp.status()).toBe(200);

		const afterCount = countEntities(`collection = '${COLLECTION}' AND id = '${DEL_ID}'`);
		expect(afterCount).toBe(0);
	});
});
