/**
 * Shared utilities for rendering policy-redacted entity data without
 * leaking the original value into the DOM, copy buffer, exports, or logs.
 *
 * The contract: callers pass entity data through {@link redactValue} BEFORE
 * handing it to any renderer (JsonTree, `<pre>{JSON.stringify(...)}</pre>`,
 * list previews, audit payloads). Leaves at paths in the policy's
 * `redactedFields` list are replaced with the {@link REDACTED_PLACEHOLDER}
 * literal. The original value is dropped from the cloned tree, so even
 * `JSON.stringify`, copy-to-clipboard, and `console.log` only ever see the
 * placeholder.
 *
 * Field paths follow the same dotted shape the FEAT-029 policy compiler
 * emits (e.g. `commercial_terms`, `nested.field`, `line_items[].sku`).
 */

export const REDACTED_PLACEHOLDER = '[redacted]';

/** Combine a parent field path with a child key into a dotted path. */
export function joinFieldPath(parent: string | undefined, key: string): string {
	return parent ? `${parent}.${key}` : key;
}

/**
 * Whether a given dotted field path is in the policy's redacted-field list.
 * Supports the `field[]` array-element marker: a redacted path of
 * `line_items[].sku` matches a runtime path `line_items.0.sku`.
 */
export function isFieldRedacted(
	path: string,
	redactedFields: readonly string[] | null | undefined,
): boolean {
	if (!redactedFields || redactedFields.length === 0) return false;
	if (redactedFields.includes(path)) return true;
	const arrayNormalized = path.replace(/\.\d+(?=\.|$)/g, '[]');
	return redactedFields.includes(arrayNormalized);
}

/**
 * Whether to mask only the server-side nulls that signal redaction
 * (`replace-nulls`, the default) or to also mask any non-null leaf at a
 * redacted path (`force-mask`).
 *
 * - `replace-nulls`: trust the server. The GraphQL/REST read paths return
 *   `null` for redacted fields, so any non-null value at a redacted path
 *   was allowed through (e.g. a row-dependent field policy let it pass).
 *   Use this for entity reads, list previews, and audit `data_before` /
 *   `data_after` payloads — those go through read enforcement that has
 *   already applied the precise per-row policy.
 * - `force-mask`: assume the payload was *not* policy-filtered before it
 *   reached the client. Use this for rollback preview targets and any
 *   other surface that exposes raw entity state, where the server may
 *   return real values that the caller is supposed to see redacted.
 */
export type RedactMode = 'replace-nulls' | 'force-mask';

/**
 * Return a deep clone of `value` with redacted leaves replaced by
 * {@link REDACTED_PLACEHOLDER}. See {@link RedactMode} for the policy.
 *
 * Containers (objects, arrays) are recursed into. If `redactedFields` is
 * empty or null the input is returned unchanged.
 */
export function redactValue<T = unknown>(
	value: T,
	redactedFields: readonly string[] | null | undefined,
	mode: RedactMode = 'replace-nulls',
): T {
	if (!redactedFields || redactedFields.length === 0) return value;
	return redactValueInternal(value, redactedFields, '', mode) as T;
}

function redactValueInternal(
	value: unknown,
	redactedFields: readonly string[],
	parentPath: string,
	mode: RedactMode,
): unknown {
	if (Array.isArray(value)) {
		return value.map((entry, index) =>
			redactLeafOrRecurse(entry, redactedFields, joinFieldPath(parentPath, String(index)), mode),
		);
	}
	if (value && typeof value === 'object') {
		const out: Record<string, unknown> = {};
		for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
			const childPath = joinFieldPath(parentPath, key);
			out[key] = redactLeafOrRecurse(child, redactedFields, childPath, mode);
		}
		return out;
	}
	// Primitive at the root with empty parentPath has no "field path" to
	// match against; pass through unchanged.
	return value;
}

function redactLeafOrRecurse(
	value: unknown,
	redactedFields: readonly string[],
	path: string,
	mode: RedactMode,
): unknown {
	// Check the container path before recursing: a redacted container
	// must be replaced wholesale (in force-mask mode) so its raw
	// contents never reach the cloned value or downstream renderers.
	if (mode === 'force-mask' && isFieldRedacted(path, redactedFields)) {
		return REDACTED_PLACEHOLDER;
	}
	if (Array.isArray(value) || (value && typeof value === 'object')) {
		return redactValueInternal(value, redactedFields, path, mode);
	}
	if (!isFieldRedacted(path, redactedFields)) return value;
	// Default `replace-nulls`: only relabel server-supplied nulls, never
	// re-redact a value the server allowed through.
	return value === null ? REDACTED_PLACEHOLDER : value;
}

/**
 * Whether a leaf string value matches the placeholder used by
 * {@link redactValue}. JsonTree and other renderers use this to apply a
 * `data-testid="redacted-field"` marker so e2e tests can assert redaction
 * happened.
 */
export function isRedactedPlaceholder(value: unknown): boolean {
	return value === REDACTED_PLACEHOLDER;
}
