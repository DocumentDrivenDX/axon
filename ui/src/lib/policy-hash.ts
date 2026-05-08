function sortKeysDeep(value: unknown): unknown {
	if (value === null || typeof value !== 'object') return value;
	if (Array.isArray(value)) return value.map(sortKeysDeep);
	const obj = value as Record<string, unknown>;
	return Object.fromEntries(
		Object.keys(obj)
			.sort()
			.map((k) => [k, sortKeysDeep(obj[k])]),
	);
}

// Canonical form: sort keys recursively, no whitespace, SHA-256 first 16 hex chars.
export async function computeAccessControlHash(accessControl: unknown): Promise<string> {
	const canonical = JSON.stringify(sortKeysDeep(accessControl));
	const data = new TextEncoder().encode(canonical);
	const buffer = await crypto.subtle.digest('SHA-256', data);
	const hex = Array.from(new Uint8Array(buffer))
		.map((b) => b.toString(16).padStart(2, '0'))
		.join('');
	return hex.slice(0, 16);
}
