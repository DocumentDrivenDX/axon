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

function fnv1a64(data: Uint8Array): string {
	let hash = 0xcbf29ce484222325n;
	const prime = 0x100000001b3n;
	const mask = 0xffffffffffffffffn;
	for (const byte of data) {
		hash ^= BigInt(byte);
		hash = (hash * prime) & mask;
	}
	return hash.toString(16).padStart(16, '0');
}

// Canonical form: sort keys recursively, no whitespace, SHA-256 first 16 hex
// chars where Web Crypto is available. The deterministic FNV fallback keeps
// the browser-only activation gate working on non-secure local origins.
export async function computeAccessControlHash(accessControl: unknown): Promise<string> {
	const canonical = JSON.stringify(sortKeysDeep(accessControl));
	const data = new TextEncoder().encode(canonical);
	const subtle = globalThis.crypto?.subtle;
	if (!subtle) return fnv1a64(data);
	try {
		const buffer = await subtle.digest('SHA-256', data);
		const hex = Array.from(new Uint8Array(buffer))
			.map((b) => b.toString(16).padStart(2, '0'))
			.join('');
		return hex.slice(0, 16);
	} catch {
		return fnv1a64(data);
	}
}
