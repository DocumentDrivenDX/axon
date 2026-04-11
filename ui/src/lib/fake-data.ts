/**
 * Schema-aware fake data generator for test seeding.
 *
 * Generates valid random entity data from a JSON Schema without
 * external dependencies (no faker.js). Handles nested objects,
 * arrays, enums, format constraints, and numeric bounds.
 */

type JsonSchema = {
	type?: string | string[];
	properties?: Record<string, JsonSchema>;
	required?: string[];
	items?: JsonSchema;
	enum?: unknown[];
	format?: string;
	minLength?: number;
	maxLength?: number;
	minimum?: number;
	maximum?: number;
	default?: unknown;
};

// ---------------------------------------------------------------------------
// Word bank for generating plausible-looking strings
// ---------------------------------------------------------------------------

const WORDS = [
	'alpha',
	'bravo',
	'charlie',
	'delta',
	'echo',
	'foxtrot',
	'golf',
	'hotel',
	'india',
	'juliet',
	'kilo',
	'lima',
	'mike',
	'november',
	'oscar',
	'papa',
	'quebec',
	'romeo',
	'sierra',
	'tango',
	'uniform',
	'victor',
	'whiskey',
	'xray',
	'yankee',
	'zulu',
];

const DOMAINS = ['example.com', 'test.org', 'demo.net', 'sample.io'];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function randomInt(min: number, max: number): number {
	return Math.floor(Math.random() * (max - min + 1)) + min;
}

function randomFloat(min: number, max: number): number {
	return Math.random() * (max - min) + min;
}

function pickRandom<T>(items: readonly T[]): T {
	return items[randomInt(0, items.length - 1)] as T;
}

function randomWord(): string {
	return pickRandom(WORDS);
}

// ---------------------------------------------------------------------------
// Format-specific generators
// ---------------------------------------------------------------------------

function generateEmail(): string {
	return `${randomWord()}.${randomWord()}@${pickRandom(DOMAINS)}`;
}

function generateDateTime(): string {
	const year = randomInt(2020, 2026);
	const month = String(randomInt(1, 12)).padStart(2, '0');
	const day = String(randomInt(1, 28)).padStart(2, '0');
	const hour = String(randomInt(0, 23)).padStart(2, '0');
	const minute = String(randomInt(0, 59)).padStart(2, '0');
	const second = String(randomInt(0, 59)).padStart(2, '0');
	return `${year}-${month}-${day}T${hour}:${minute}:${second}Z`;
}

function generateDate(): string {
	const year = randomInt(2020, 2026);
	const month = String(randomInt(1, 12)).padStart(2, '0');
	const day = String(randomInt(1, 28)).padStart(2, '0');
	return `${year}-${month}-${day}`;
}

function generateUri(): string {
	return `https://${pickRandom(DOMAINS)}/${randomWord()}/${randomWord()}`;
}

function generateUuid(): string {
	const hex = () => randomInt(0, 0xffff).toString(16).padStart(4, '0');
	return `${hex()}${hex()}-${hex()}-4${hex().slice(1)}-${(randomInt(8, 11)).toString(16)}${hex().slice(1)}-${hex()}${hex()}${hex()}`;
}

// ---------------------------------------------------------------------------
// Core generator
// ---------------------------------------------------------------------------

function resolveType(schema: JsonSchema): string {
	if (Array.isArray(schema.type)) {
		// Pick the first non-null type for generation purposes
		const concrete = schema.type.find((t) => t !== 'null');
		return concrete ?? 'null';
	}
	return schema.type ?? 'object';
}

function generateString(schema: JsonSchema): string {
	// Format-aware generation
	switch (schema.format) {
		case 'email':
			return generateEmail();
		case 'date-time':
			return generateDateTime();
		case 'date':
			return generateDate();
		case 'uri':
		case 'url':
			return generateUri();
		case 'uuid':
			return generateUuid();
	}

	const minLen = schema.minLength ?? 3;
	const maxLen = schema.maxLength ?? 24;
	const targetLen = randomInt(minLen, Math.max(minLen, maxLen));

	// Build a string of random words, trimmed to the target length
	let result = '';
	while (result.length < targetLen) {
		if (result.length > 0) result += ' ';
		result += randomWord();
	}

	// Ensure we respect maxLength by truncating
	if (result.length > maxLen) {
		result = result.slice(0, maxLen).trimEnd();
	}

	// Ensure we respect minLength by padding
	while (result.length < minLen) {
		result += 'a';
	}

	return result;
}

function generateNumber(schema: JsonSchema, asInteger: boolean): number {
	const min = schema.minimum ?? 0;
	const max = schema.maximum ?? 1000;
	if (asInteger) {
		return randomInt(min, max);
	}
	return Math.round(randomFloat(min, max) * 100) / 100;
}

function generateArray(schema: JsonSchema): unknown[] {
	const count = randomInt(1, 3);
	const itemSchema = schema.items ?? {};
	return Array.from({ length: count }, () => generateValue(itemSchema));
}

function generateObject(schema: JsonSchema): Record<string, unknown> {
	const result: Record<string, unknown> = {};
	const required = new Set(schema.required ?? []);
	const properties = schema.properties ?? {};

	for (const [key, propSchema] of Object.entries(properties)) {
		const isRequired = required.has(key);
		// Required fields always present; optional fields included ~70% of the time
		if (isRequired || Math.random() < 0.7) {
			result[key] = generateValue(propSchema);
		}
	}

	return result;
}

function generateValue(schema: JsonSchema): unknown {
	// Enum takes priority
	if (schema.enum && schema.enum.length > 0) {
		return pickRandom(schema.enum);
	}

	// Default value
	if (schema.default !== undefined) {
		return schema.default;
	}

	const type = resolveType(schema);

	switch (type) {
		case 'string':
			return generateString(schema);
		case 'number':
			return generateNumber(schema, false);
		case 'integer':
			return generateNumber(schema, true);
		case 'boolean':
			return Math.random() < 0.5;
		case 'array':
			return generateArray(schema);
		case 'object':
			return generateObject(schema);
		case 'null':
			return null;
		default:
			return generateString(schema);
	}
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Generate a single fake entity from a JSON Schema.
 *
 * @param schema - A JSON Schema object describing the entity shape
 * @returns A valid random value matching the schema (object for object schemas,
 *          string/number/etc. for primitive schemas)
 */
export function generateFakeEntity(schema: object): unknown {
	return generateValue(schema as JsonSchema);
}

/**
 * Generate multiple fake entities from a JSON Schema.
 *
 * @param schema - A JSON Schema object describing the entity shape
 * @param count - Number of entities to generate
 * @returns An array of valid random values matching the schema
 */
export function generateFakeEntities(schema: object, count: number): unknown[] {
	return Array.from({ length: count }, () => generateFakeEntity(schema));
}
