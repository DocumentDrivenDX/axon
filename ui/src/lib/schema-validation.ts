type ValidationIssue = string;

type JsonSchema = {
	type?: string | string[];
	required?: string[];
	properties?: Record<string, JsonSchema>;
	items?: JsonSchema;
};

function matchesType(value: unknown, expected: string): boolean {
	switch (expected) {
		case 'object':
			return typeof value === 'object' && value !== null && !Array.isArray(value);
		case 'array':
			return Array.isArray(value);
		case 'string':
			return typeof value === 'string';
		case 'integer':
			return Number.isInteger(value);
		case 'number':
			return typeof value === 'number' && Number.isFinite(value);
		case 'boolean':
			return typeof value === 'boolean';
		case 'null':
			return value === null;
		default:
			return true;
	}
}

function validateNode(
	schema: JsonSchema | undefined,
	value: unknown,
	path: string,
): ValidationIssue[] {
	if (!schema) {
		return [];
	}

	const issues: ValidationIssue[] = [];
	const declaredTypes = Array.isArray(schema.type) ? schema.type : schema.type ? [schema.type] : [];

	if (
		declaredTypes.length > 0 &&
		!declaredTypes.some((expectedType) => matchesType(value, expectedType))
	) {
		issues.push(`${path} must be ${declaredTypes.join(' or ')}`);
		return issues;
	}

	if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
		const objectValue = value as Record<string, unknown>;
		for (const field of schema.required ?? []) {
			if (!(field in objectValue)) {
				issues.push(`${path}.${field} is required`);
			}
		}

		for (const [field, fieldSchema] of Object.entries(schema.properties ?? {})) {
			if (field in objectValue) {
				issues.push(...validateNode(fieldSchema, objectValue[field], `${path}.${field}`));
			}
		}
	}

	if (Array.isArray(value) && schema.items) {
		value.forEach((entry, index) => {
			issues.push(...validateNode(schema.items, entry, `${path}[${index}]`));
		});
	}

	return issues;
}

export function validateEntityData(schema: unknown, value: unknown): ValidationIssue[] {
	if (!schema || typeof schema !== 'object') {
		return [];
	}

	return validateNode(schema as JsonSchema, value, '$');
}
