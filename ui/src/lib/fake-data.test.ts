import { describe, expect, test } from 'bun:test';

import { generateFakeEntities, generateFakeEntity } from './fake-data';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function entity(schema: object): Record<string, unknown> {
	return generateFakeEntity(schema) as Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Basic type generation
// ---------------------------------------------------------------------------

describe('generateFakeEntity', () => {
	test('generates a string for type "string"', () => {
		const result = generateFakeEntity({ type: 'string' });
		expect(typeof result).toBe('string');
		expect((result as string).length).toBeGreaterThan(0);
	});

	test('generates an integer for type "integer"', () => {
		const result = generateFakeEntity({ type: 'integer' });
		expect(typeof result).toBe('number');
		expect(Number.isInteger(result)).toBe(true);
	});

	test('generates a number for type "number"', () => {
		const result = generateFakeEntity({ type: 'number' });
		expect(typeof result).toBe('number');
		expect(Number.isFinite(result as number)).toBe(true);
	});

	test('generates a boolean for type "boolean"', () => {
		const result = generateFakeEntity({ type: 'boolean' });
		expect(typeof result).toBe('boolean');
	});

	test('generates null for type "null"', () => {
		const result = generateFakeEntity({ type: 'null' });
		expect(result).toBeNull();
	});

	test('generates an array for type "array"', () => {
		const result = generateFakeEntity({ type: 'array', items: { type: 'string' } });
		expect(Array.isArray(result)).toBe(true);
		const arr = result as string[];
		expect(arr.length).toBeGreaterThanOrEqual(1);
		expect(arr.length).toBeLessThanOrEqual(3);
		for (const item of arr) {
			expect(typeof item).toBe('string');
		}
	});

	test('generates an object for type "object"', () => {
		const result = entity({
			type: 'object',
			properties: {
				name: { type: 'string' },
				age: { type: 'integer' },
			},
			required: ['name'],
		});
		expect(typeof result).toBe('object');
		expect(result).not.toBeNull();
		// Required field must be present
		expect(result).toHaveProperty('name');
		expect(typeof result.name).toBe('string');
	});
});

// ---------------------------------------------------------------------------
// Constraint handling
// ---------------------------------------------------------------------------

describe('constraints', () => {
	test('respects minLength and maxLength for strings', () => {
		for (let i = 0; i < 20; i++) {
			const result = generateFakeEntity({
				type: 'string',
				minLength: 5,
				maxLength: 10,
			}) as string;
			expect(result.length).toBeGreaterThanOrEqual(5);
			expect(result.length).toBeLessThanOrEqual(10);
		}
	});

	test('respects minimum and maximum for integers', () => {
		for (let i = 0; i < 20; i++) {
			const result = generateFakeEntity({
				type: 'integer',
				minimum: 10,
				maximum: 20,
			}) as number;
			expect(result).toBeGreaterThanOrEqual(10);
			expect(result).toBeLessThanOrEqual(20);
			expect(Number.isInteger(result)).toBe(true);
		}
	});

	test('respects minimum and maximum for numbers', () => {
		for (let i = 0; i < 20; i++) {
			const result = generateFakeEntity({
				type: 'number',
				minimum: 1.0,
				maximum: 5.0,
			}) as number;
			expect(result).toBeGreaterThanOrEqual(1.0);
			expect(result).toBeLessThanOrEqual(5.0);
		}
	});

	test('picks from enum values', () => {
		const allowed = ['red', 'green', 'blue'];
		for (let i = 0; i < 20; i++) {
			const result = generateFakeEntity({ type: 'string', enum: allowed }) as string;
			expect(allowed).toContain(result);
		}
	});

	test('respects enum with mixed types', () => {
		const allowed: (string | number | boolean | null)[] = [1, 'two', true, null];
		for (let i = 0; i < 20; i++) {
			const result = generateFakeEntity({ enum: allowed }) as string | number | boolean | null;
			expect(allowed).toContain(result);
		}
	});
});

// ---------------------------------------------------------------------------
// Format handling
// ---------------------------------------------------------------------------

describe('format', () => {
	test('generates valid email for format "email"', () => {
		for (let i = 0; i < 10; i++) {
			const result = generateFakeEntity({ type: 'string', format: 'email' }) as string;
			expect(result).toContain('@');
			expect(result).toContain('.');
		}
	});

	test('generates ISO date-time for format "date-time"', () => {
		for (let i = 0; i < 10; i++) {
			const result = generateFakeEntity({ type: 'string', format: 'date-time' }) as string;
			expect(result).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
		}
	});

	test('generates ISO date for format "date"', () => {
		for (let i = 0; i < 10; i++) {
			const result = generateFakeEntity({ type: 'string', format: 'date' }) as string;
			expect(result).toMatch(/^\d{4}-\d{2}-\d{2}$/);
		}
	});

	test('generates URL for format "uri"', () => {
		for (let i = 0; i < 10; i++) {
			const result = generateFakeEntity({ type: 'string', format: 'uri' }) as string;
			expect(result).toMatch(/^https:\/\//);
		}
	});

	test('generates UUID for format "uuid"', () => {
		for (let i = 0; i < 10; i++) {
			const result = generateFakeEntity({ type: 'string', format: 'uuid' }) as string;
			expect(result).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$/);
		}
	});
});

// ---------------------------------------------------------------------------
// Nested / recursive structures
// ---------------------------------------------------------------------------

describe('nested schemas', () => {
	test('generates nested objects', () => {
		const result = entity({
			type: 'object',
			properties: {
				address: {
					type: 'object',
					properties: {
						street: { type: 'string' },
						city: { type: 'string' },
						zip: { type: 'string', minLength: 5, maxLength: 5 },
					},
					required: ['street', 'city'],
				},
			},
			required: ['address'],
		});

		expect(result).toHaveProperty('address');
		const address = result.address as Record<string, unknown>;
		expect(typeof address.street).toBe('string');
		expect(typeof address.city).toBe('string');
	});

	test('generates arrays of objects', () => {
		const result = entity({
			type: 'object',
			properties: {
				tags: {
					type: 'array',
					items: {
						type: 'object',
						properties: {
							label: { type: 'string' },
							weight: { type: 'number', minimum: 0, maximum: 1 },
						},
						required: ['label'],
					},
				},
			},
			required: ['tags'],
		});

		expect(result).toHaveProperty('tags');
		const tags = result.tags as Array<Record<string, unknown>>;
		expect(Array.isArray(tags)).toBe(true);
		expect(tags.length).toBeGreaterThanOrEqual(1);
		for (const tag of tags) {
			expect(typeof tag.label).toBe('string');
		}
	});

	test('handles arrays of strings', () => {
		const result = entity({
			type: 'object',
			properties: {
				emails: {
					type: 'array',
					items: { type: 'string', format: 'email' },
				},
			},
			required: ['emails'],
		});

		const emails = result.emails as string[];
		expect(Array.isArray(emails)).toBe(true);
		for (const email of emails) {
			expect(email).toContain('@');
		}
	});
});

// ---------------------------------------------------------------------------
// Required vs optional fields
// ---------------------------------------------------------------------------

describe('required vs optional', () => {
	test('required fields are always present', () => {
		for (let i = 0; i < 30; i++) {
			const result = entity({
				type: 'object',
				properties: {
					id: { type: 'string' },
					name: { type: 'string' },
					optional_field: { type: 'string' },
				},
				required: ['id', 'name'],
			});
			expect(result).toHaveProperty('id');
			expect(result).toHaveProperty('name');
		}
	});

	test('optional fields are sometimes omitted (probabilistic, check over many runs)', () => {
		let missingCount = 0;
		const runs = 100;

		for (let i = 0; i < runs; i++) {
			const result = entity({
				type: 'object',
				properties: {
					required_field: { type: 'string' },
					optional_field: { type: 'string' },
				},
				required: ['required_field'],
			});
			if (!('optional_field' in result)) {
				missingCount++;
			}
		}

		// With 30% chance of omission, we expect ~30 misses in 100 runs.
		// Allow a wide range to avoid flaky tests.
		expect(missingCount).toBeGreaterThan(5);
		expect(missingCount).toBeLessThan(60);
	});
});

// ---------------------------------------------------------------------------
// Union types
// ---------------------------------------------------------------------------

describe('union types', () => {
	test('handles ["string", "null"] by generating a string', () => {
		const result = generateFakeEntity({ type: ['string', 'null'] });
		expect(typeof result).toBe('string');
	});

	test('handles ["integer", "null"] by generating an integer', () => {
		const result = generateFakeEntity({ type: ['integer', 'null'] });
		expect(typeof result).toBe('number');
		expect(Number.isInteger(result)).toBe(true);
	});
});

// ---------------------------------------------------------------------------
// generateFakeEntities (batch)
// ---------------------------------------------------------------------------

describe('generateFakeEntities', () => {
	test('generates the requested number of entities', () => {
		const schema = {
			type: 'object',
			properties: {
				title: { type: 'string' },
				done: { type: 'boolean' },
			},
			required: ['title', 'done'],
		};

		const results = generateFakeEntities(schema, 5);
		expect(results).toHaveLength(5);
		for (const result of results) {
			const r = result as Record<string, unknown>;
			expect(typeof r.title).toBe('string');
			expect(typeof r.done).toBe('boolean');
		}
	});

	test('generates zero entities when count is 0', () => {
		const results = generateFakeEntities({ type: 'object' }, 0);
		expect(results).toHaveLength(0);
	});
});

// ---------------------------------------------------------------------------
// Realistic schema (end-to-end)
// ---------------------------------------------------------------------------

describe('realistic schema', () => {
	test('generates a valid user entity', () => {
		const userSchema = {
			type: 'object',
			properties: {
				name: { type: 'string', minLength: 1, maxLength: 50 },
				email: { type: 'string', format: 'email' },
				age: { type: 'integer', minimum: 18, maximum: 120 },
				role: { type: 'string', enum: ['admin', 'editor', 'viewer'] },
				active: { type: 'boolean' },
				profile_url: { type: 'string', format: 'uri' },
				joined: { type: 'string', format: 'date-time' },
				tags: {
					type: 'array',
					items: { type: 'string' },
				},
				settings: {
					type: 'object',
					properties: {
						theme: { type: 'string', enum: ['light', 'dark'] },
						notifications: { type: 'boolean' },
					},
					required: ['theme'],
				},
			},
			required: ['name', 'email', 'age', 'role', 'active'],
		};

		for (let i = 0; i < 10; i++) {
			const user = entity(userSchema);
			expect(typeof user.name).toBe('string');
			expect((user.name as string).length).toBeGreaterThanOrEqual(1);
			expect((user.name as string).length).toBeLessThanOrEqual(50);
			expect(user.email as string).toContain('@');
			expect(user.age as number).toBeGreaterThanOrEqual(18);
			expect(user.age as number).toBeLessThanOrEqual(120);
			expect(['admin', 'editor', 'viewer']).toContain(user.role as string);
			expect(typeof user.active).toBe('boolean');
		}
	});
});

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

describe('edge cases', () => {
	test('empty schema generates an empty object', () => {
		const result = generateFakeEntity({});
		expect(typeof result).toBe('object');
		expect(result).not.toBeNull();
	});

	test('schema with no properties generates an empty object', () => {
		const result = entity({ type: 'object' });
		expect(Object.keys(result)).toHaveLength(0);
	});

	test('schema with default value uses the default', () => {
		const result = generateFakeEntity({ type: 'string', default: 'hello' });
		expect(result).toBe('hello');
	});
});
