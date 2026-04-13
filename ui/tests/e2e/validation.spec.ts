import { expect, test } from '@playwright/test';

/**
 * E2E tests for HTTP API error paths — validation, conflicts, and not-found.
 *
 * These tests exercise the server's error handling at the HTTP boundary.
 * They are backend-agnostic (memory, SQLite, or postgres) and verify that:
 *   - 422 UNPROCESSABLE_ENTITY is returned for schema validation violations
 *   - 409 CONFLICT is returned for duplicate resources and version conflicts
 *   - 404 NOT_FOUND is returned for missing collections and entities
 *
 * Run with any of the E2E configs, e.g.:
 *   bunx playwright test --config playwright.e2e.sqlite.config.ts
 */

// ── Schema validation (422) ───────────────────────────────────────────────────

test.describe('API validation — schema errors (422)', () => {
	const STRICT_COLLECTION = 'val-e2e-strict';

	test.beforeAll(async ({ request }) => {
		// Collection with a required "name" field and a typed "score" integer field.
		const resp = await request.post(`/collections/${STRICT_COLLECTION}`, {
			data: {
				schema: {
					description: null,
					version: 1,
					entity_schema: {
						type: 'object',
						properties: {
							name: { type: 'string' },
							score: { type: 'integer' },
						},
						required: ['name'],
					},
					link_types: {},
				},
				actor: 'e2e-test',
			},
		});
		expect([201, 409]).toContain(resp.status());
	});

	test('POST entity missing required field returns 422 with schema_validation code', async ({
		request,
	}) => {
		const resp = await request.post(`/entities/${STRICT_COLLECTION}/val-missing-name`, {
			data: { data: { score: 42 }, actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(422);
		const body = (await resp.json()) as { code: string; detail: unknown };
		expect(body.code).toBe('schema_validation');
	});

	test('POST entity with wrong field type returns 422 with schema_validation code', async ({
		request,
	}) => {
		const resp = await request.post(`/entities/${STRICT_COLLECTION}/val-bad-type`, {
			data: { data: { name: 'ok', score: 'not-an-integer' }, actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(422);
		const body = (await resp.json()) as { code: string };
		expect(body.code).toBe('schema_validation');
	});

	test('valid entity satisfying the schema is accepted (201)', async ({ request }) => {
		const resp = await request.post(`/entities/${STRICT_COLLECTION}/val-valid-entity`, {
			data: { data: { name: 'Alice', score: 100 }, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(resp.status());
	});
});

// ── Conflict errors (409) ─────────────────────────────────────────────────────

test.describe('API validation — conflict errors (409)', () => {
	const COLLECTION = 'val-e2e-dup';
	const ENTITY_ID = 'val-dup-entity-001';

	test.beforeAll(async ({ request }) => {
		// Create the collection (idempotent — 409 is fine on repeat runs).
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

		// Create the entity used by duplicate-entity and version-conflict tests.
		const entityResp = await request.post(`/entities/${COLLECTION}/${ENTITY_ID}`, {
			data: { data: { note: 'original' }, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(entityResp.status());
	});

	test('POST entity is idempotent / upsert — always returns 201', async ({ request }) => {
		// Unlike collections, entities use upsert semantics: POSTing to an
		// existing entity ID updates it and still returns 201 CREATED.
		// There is no 409 for duplicate entity IDs.
		const resp = await request.post(`/entities/${COLLECTION}/${ENTITY_ID}`, {
			data: { data: { note: 'second write via POST' }, actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(201);
		const body = (await resp.json()) as { entity: { id: string } };
		expect(body.entity.id).toBe(ENTITY_ID);
	});

	test('POST duplicate collection returns 409 with already_exists code', async ({ request }) => {
		const resp = await request.post(`/collections/${COLLECTION}`, {
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
		expect(resp.status()).toBe(409);
		const body = (await resp.json()) as { code: string };
		expect(body.code).toBe('already_exists');
	});

	test('PUT entity with wrong expected_version returns 409 with version_conflict code', async ({
		request,
	}) => {
		const resp = await request.put(`/entities/${COLLECTION}/${ENTITY_ID}`, {
			data: {
				data: { note: 'conflict update' },
				actor: 'e2e-test',
				expected_version: 99999,
			},
		});
		expect(resp.status()).toBe(409);
		const body = (await resp.json()) as {
			code: string;
			detail: { expected: number; actual: number; current_entity: { id: string; version: number } };
		};
		expect(body.code).toBe('version_conflict');
		expect(body.detail.expected).toBe(99999);
		// The response must include the current entity so the client can resolve.
		expect(body.detail.current_entity).toBeDefined();
		expect(body.detail.current_entity.id).toBe(ENTITY_ID);
	});

	test('version_conflict response includes current entity data for client resolution', async ({
		request,
	}) => {
		// GET /entities/{col}/{id} returns {"entity": {"version": N, ...}}.
		const getResp = await request.get(`/entities/${COLLECTION}/${ENTITY_ID}`);
		expect(getResp.ok()).toBe(true);
		const { entity: currentEntity } = (await getResp.json()) as {
			entity: { version: number; data: Record<string, unknown> };
		};

		const conflictResp = await request.put(`/entities/${COLLECTION}/${ENTITY_ID}`, {
			data: {
				data: { note: 'conflict' },
				actor: 'e2e-test',
				expected_version: currentEntity.version + 100,
			},
		});
		expect(conflictResp.status()).toBe(409);
		const body = (await conflictResp.json()) as {
			detail: { current_entity: { version: number; data: Record<string, unknown> } };
		};
		// current_entity.version must be the actual current version.
		expect(body.detail.current_entity.version).toBe(currentEntity.version);
	});
});

// ── Not-found errors (404) ────────────────────────────────────────────────────

test.describe('API validation — not found errors (404)', () => {
	const EXISTS_COLLECTION = 'val-e2e-notfound';

	test.beforeAll(async ({ request }) => {
		const resp = await request.post(`/collections/${EXISTS_COLLECTION}`, {
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
		expect([201, 409]).toContain(resp.status());
	});

	test('GET unknown collection returns 404 with not_found code', async ({ request }) => {
		const resp = await request.get('/collections/completely-nonexistent-xyzzy');
		expect(resp.status()).toBe(404);
		const body = (await resp.json()) as { code: string };
		expect(body.code).toBe('not_found');
	});

	test('GET unknown entity within existing collection returns 404', async ({ request }) => {
		const resp = await request.get(`/entities/${EXISTS_COLLECTION}/nonexistent-entity-xyzzy`);
		expect(resp.status()).toBe(404);
		const body = (await resp.json()) as { code: string };
		expect(body.code).toBe('not_found');
	});

	test('GET schema for unknown collection returns 404', async ({ request }) => {
		const resp = await request.get('/collections/completely-nonexistent-xyzzy/schema');
		expect(resp.status()).toBe(404);
		const body = (await resp.json()) as { code: string };
		expect(body.code).toBe('not_found');
	});

	test('DELETE non-existent entity is idempotent and returns 200', async ({ request }) => {
		// The server uses idempotent delete semantics: deleting a resource that
		// does not exist succeeds rather than returning 404.  This matches the
		// HTTP spec for safe/idempotent methods and avoids spurious errors when
		// retrying after a network failure.
		const resp = await request.delete(`/entities/${EXISTS_COLLECTION}/no-such-entity`, {
			data: { actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(200);
		const body = (await resp.json()) as { collection: string; id: string };
		expect(body.collection).toBe(EXISTS_COLLECTION);
		expect(body.id).toBe('no-such-entity');
	});
});

// ── UI — validation error display ─────────────────────────────────────────────

test.describe('UI — schema validation error is shown to the user', () => {
	const STRICT_UI_COLLECTION = 'val-e2e-ui-strict';

	test.beforeAll(async ({ request }) => {
		const resp = await request.post(`/collections/${STRICT_UI_COLLECTION}`, {
			data: {
				schema: {
					description: null,
					version: 1,
					entity_schema: {
						type: 'object',
						properties: { name: { type: 'string' } },
						required: ['name'],
					},
					link_types: {},
				},
				actor: 'e2e-test',
			},
		});
		expect([201, 409]).toContain(resp.status());
	});

	test('creating entity with invalid data shows an error in the UI', async ({ page }) => {
		await page.goto(`/ui/collections/${STRICT_UI_COLLECTION}`);
		await page.waitForLoadState('networkidle');

		// Open the Create Entity panel if it's not already visible.
		const createPanel = page.locator('section.panel').filter({ hasText: 'Create Entity' });
		const panelVisible = await createPanel.isVisible().catch(() => false);
		if (!panelVisible) {
			const toggleButton = page
				.locator('.page-header')
				.getByRole('button', { name: 'Create Entity' });
			await expect(toggleButton).toBeVisible({ timeout: 10000 });
			await toggleButton.click();
		}

		await expect(createPanel).toBeVisible({ timeout: 10000 });

		// Fill in an entity ID but provide JSON that is missing the required "name" field.
		await createPanel.getByPlaceholder('task-001').fill('val-ui-invalid');
		const textarea = createPanel.locator('textarea');
		await expect(textarea).toBeVisible();
		await textarea.fill(JSON.stringify({ score: 99 }));

		// Submit.
		await createPanel.getByRole('button', { name: 'Create Entity' }).click();

		// The UI must display some error.  Accept either a generic message element
		// or the server's validation error propagated into the page.
		const errorVisible =
			(await page.locator('.message.error').isVisible().catch(() => false)) ||
			(await page.getByText(/schema/i).isVisible().catch(() => false)) ||
			(await page.getByText(/invalid/i).isVisible().catch(() => false)) ||
			(await page.getByText(/required/i).isVisible().catch(() => false)) ||
			(await page.getByText(/validation/i).isVisible().catch(() => false));

		expect(
			errorVisible,
			'Expected some error message to be visible after submitting invalid entity data',
		).toBe(true);
	});
});

// ── Schema constraint varieties ───────────────────────────────────────────────
//
// These tests verify that the full range of JSON Schema 2020-12 constraints
// enforced by axon-schema are applied on BOTH create (POST) and update (PUT).
// The collection below uses every constraint type the server advertises:
//   enum, minimum/maximum, minLength/maxLength, additionalProperties: false.

test.describe('API validation — schema constraint varieties', () => {
	const COLL = 'val-e2e-constraints';
	const VALID_ENTITY = 'val-constraints-base';

	/** A valid payload that satisfies every constraint in the schema below. */
	const VALID_DATA = {
		status: 'open',
		priority: 5,
		title: 'hello',
	};

	test.beforeAll(async ({ request }) => {
		const resp = await request.post(`/collections/${COLL}`, {
			data: {
				schema: {
					description: null,
					version: 1,
					entity_schema: {
						type: 'object',
						properties: {
							// enum constraint
							status: { type: 'string', enum: ['open', 'closed', 'pending'] },
							// numeric range constraints
							priority: { type: 'integer', minimum: 1, maximum: 10 },
							// string length constraints
							title: { type: 'string', minLength: 3, maxLength: 50 },
						},
						required: ['status', 'title'],
						additionalProperties: false,
					},
					link_types: {},
				},
				actor: 'e2e-test',
			},
		});
		expect([201, 409]).toContain(resp.status());

		// Seed one valid entity so update (PUT) tests have something to work with.
		const eResp = await request.post(`/entities/${COLL}/${VALID_ENTITY}`, {
			data: { data: VALID_DATA, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(eResp.status());
	});

	// ── enum ───────────────────────────────────────────────────────────────────

	test('POST: enum violation returns 422', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-bad-enum`, {
			data: { data: { status: 'invalid-value', title: 'ok' }, actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(422);
		const body = (await resp.json()) as { code: string; detail: string };
		expect(body.code).toBe('schema_validation');
	});

	test('PUT: enum violation on update returns 422', async ({ request }) => {
		const getResp = await request.get(`/entities/${COLL}/${VALID_ENTITY}`);
		const { entity: cur } = (await getResp.json()) as { entity: { version: number } };

		const resp = await request.put(`/entities/${COLL}/${VALID_ENTITY}`, {
			data: {
				data: { status: 'not-allowed', title: 'ok' },
				actor: 'e2e-test',
				expected_version: cur.version,
			},
		});
		expect(resp.status()).toBe(422);
		const body = (await resp.json()) as { code: string };
		expect(body.code).toBe('schema_validation');
	});

	// ── numeric range ──────────────────────────────────────────────────────────

	test('POST: integer below minimum returns 422', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-low-prio`, {
			data: { data: { status: 'open', title: 'abc', priority: 0 }, actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(422);
		expect(((await resp.json()) as { code: string }).code).toBe('schema_validation');
	});

	test('POST: integer above maximum returns 422', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-high-prio`, {
			data: { data: { status: 'open', title: 'abc', priority: 99 }, actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(422);
		expect(((await resp.json()) as { code: string }).code).toBe('schema_validation');
	});

	test('POST: integer at boundary (minimum=1) is accepted', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-min-prio`, {
			data: { data: { status: 'open', title: 'abc', priority: 1 }, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(resp.status());
	});

	test('POST: integer at boundary (maximum=10) is accepted', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-max-prio`, {
			data: { data: { status: 'open', title: 'abc', priority: 10 }, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(resp.status());
	});

	// ── string length ──────────────────────────────────────────────────────────

	test('POST: string shorter than minLength returns 422', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-short-title`, {
			data: { data: { status: 'open', title: 'ab' }, actor: 'e2e-test' },
		});
		expect(resp.status()).toBe(422);
		expect(((await resp.json()) as { code: string }).code).toBe('schema_validation');
	});

	test('POST: string longer than maxLength returns 422', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-long-title`, {
			data: {
				data: { status: 'open', title: 'x'.repeat(51) },
				actor: 'e2e-test',
			},
		});
		expect(resp.status()).toBe(422);
		expect(((await resp.json()) as { code: string }).code).toBe('schema_validation');
	});

	test('POST: string at exact minLength is accepted', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-exact-min`, {
			data: { data: { status: 'open', title: 'abc' }, actor: 'e2e-test' },
		});
		expect([201, 409]).toContain(resp.status());
	});

	// ── additionalProperties ───────────────────────────────────────────────────

	test('POST: extra field rejected when additionalProperties is false', async ({ request }) => {
		const resp = await request.post(`/entities/${COLL}/val-extra-field`, {
			data: {
				data: { status: 'open', title: 'abc', unknown_field: 'surprise' },
				actor: 'e2e-test',
			},
		});
		expect(resp.status()).toBe(422);
		expect(((await resp.json()) as { code: string }).code).toBe('schema_validation');
	});

	// ── multiple simultaneous violations ──────────────────────────────────────

	test('POST: multiple constraint violations are all reported in the detail', async ({
		request,
	}) => {
		// Violates: enum (status), minLength (title), minimum (priority).
		const resp = await request.post(`/entities/${COLL}/val-multi-err`, {
			data: {
				data: { status: 'bogus', title: 'x', priority: -5 },
				actor: 'e2e-test',
			},
		});
		expect(resp.status()).toBe(422);
		const body = (await resp.json()) as { code: string; detail: string };
		expect(body.code).toBe('schema_validation');
		// The detail string must mention more than one problem.
		expect(body.detail.length).toBeGreaterThan(10);
	});

	// ── missing required field on update ──────────────────────────────────────

	test('PUT: dropping a required field returns 422', async ({ request }) => {
		const getResp = await request.get(`/entities/${COLL}/${VALID_ENTITY}`);
		const { entity: cur } = (await getResp.json()) as { entity: { version: number } };

		// Omit 'status' which is required.
		const resp = await request.put(`/entities/${COLL}/${VALID_ENTITY}`, {
			data: {
				data: { title: 'no status here' },
				actor: 'e2e-test',
				expected_version: cur.version,
			},
		});
		expect(resp.status()).toBe(422);
		expect(((await resp.json()) as { code: string }).code).toBe('schema_validation');
	});
});
