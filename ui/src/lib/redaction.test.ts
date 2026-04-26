import { describe, expect, test } from 'bun:test';
import {
	REDACTED_PLACEHOLDER,
	isFieldRedacted,
	isRedactedPlaceholder,
	joinFieldPath,
	redactValue,
} from './redaction';

describe('joinFieldPath', () => {
	test('returns the key when the parent path is empty', () => {
		expect(joinFieldPath('', 'amount_cents')).toBe('amount_cents');
		expect(joinFieldPath(undefined, 'amount_cents')).toBe('amount_cents');
	});
	test('joins a parent path with a child key', () => {
		expect(joinFieldPath('billing', 'amount')).toBe('billing.amount');
	});
});

describe('isFieldRedacted', () => {
	test('returns false when no redacted fields are configured', () => {
		expect(isFieldRedacted('amount_cents', null)).toBe(false);
		expect(isFieldRedacted('amount_cents', [])).toBe(false);
		expect(isFieldRedacted('amount_cents', undefined)).toBe(false);
	});
	test('matches an exact path', () => {
		expect(isFieldRedacted('amount_cents', ['amount_cents'])).toBe(true);
		expect(isFieldRedacted('commercial_terms', ['amount_cents', 'commercial_terms'])).toBe(true);
	});
	test('does not match a different path', () => {
		expect(isFieldRedacted('vendor_id', ['amount_cents'])).toBe(false);
	});
	test('matches an array-marker path against an indexed runtime path', () => {
		expect(isFieldRedacted('line_items.0.sku', ['line_items[].sku'])).toBe(true);
		expect(isFieldRedacted('line_items.42.price', ['line_items[].price'])).toBe(true);
	});
});

describe('redactValue', () => {
	test('returns the input unchanged when no fields are redacted', () => {
		const data = { amount_cents: 100, vendor_id: 'v1' };
		expect(redactValue(data, [])).toBe(data);
		expect(redactValue(data, null)).toBe(data);
	});
	test('relabels a null leaf at a redacted path with the placeholder', () => {
		const data = { amount_cents: null, vendor_id: 'v1' };
		const out = redactValue(data, ['amount_cents']) as Record<string, unknown>;
		expect(out.amount_cents).toBe(REDACTED_PLACEHOLDER);
		expect(out.vendor_id).toBe('v1');
	});
	test('preserves non-null values even at redacted paths (server allowed them)', () => {
		// Row-dependent field policies redact only sometimes; the server
		// returning a non-null value means this row was allowed through,
		// so the client must not re-redact.
		const data = { amount_cents: 100, vendor_id: 'v1' };
		const out = redactValue(data, ['amount_cents']) as Record<string, unknown>;
		expect(out.amount_cents).toBe(100);
		expect(out.vendor_id).toBe('v1');
	});
	test('preserves a null leaf at a non-redacted path (genuine null)', () => {
		const data = { amount_cents: null, vendor_id: 'v1' };
		const out = redactValue(data, ['commercial_terms']) as Record<string, unknown>;
		expect(out.amount_cents).toBe(null);
	});
	test('does not mutate the input', () => {
		const data = { amount_cents: null };
		redactValue(data, ['amount_cents']);
		expect(data.amount_cents).toBe(null);
	});
	test('recurses into nested objects', () => {
		const data = { billing: { amount_cents: null, currency: 'USD' } };
		const out = redactValue(data, ['billing.amount_cents']) as {
			billing: { amount_cents: unknown; currency: string };
		};
		expect(out.billing.amount_cents).toBe(REDACTED_PLACEHOLDER);
		expect(out.billing.currency).toBe('USD');
	});
	test('recurses into arrays and matches by indexed path', () => {
		const data = {
			line_items: [
				{ sku: null, price: 100 },
				{ sku: null, price: 200 },
			],
		};
		const out = redactValue(data, ['line_items[].sku']) as {
			line_items: Array<{ sku: unknown; price: number }>;
		};
		const items = out.line_items;
		expect(items[0]?.sku).toBe(REDACTED_PLACEHOLDER);
		expect(items[0]?.price).toBe(100);
		expect(items[1]?.sku).toBe(REDACTED_PLACEHOLDER);
		expect(items[1]?.price).toBe(200);
	});
	test('JSON.stringify of the result never contains the original null marker for redacted fields', () => {
		const data = { commercial_terms: null, vendor_id: 'v1' };
		const out = redactValue(data, ['commercial_terms']);
		const serialized = JSON.stringify(out);
		expect(serialized.includes(REDACTED_PLACEHOLDER)).toBe(true);
		// Plain JSON.stringify would have produced "commercial_terms":null;
		// the redacted version must use the placeholder string instead.
		expect(serialized.includes('"commercial_terms":"[redacted]"')).toBe(true);
	});

	test('force-mask mode replaces non-null values at redacted paths', () => {
		const secret = 'net-15 expedited infrastructure terms';
		const data = { commercial_terms: secret, vendor_id: 'v1' };
		const out = redactValue(data, ['commercial_terms'], 'force-mask') as Record<string, unknown>;
		expect(out.commercial_terms).toBe(REDACTED_PLACEHOLDER);
		expect(out.vendor_id).toBe('v1');
		expect(JSON.stringify(out).includes(secret)).toBe(false);
	});

	test('force-mask still recurses into containers without re-redacting non-redacted leaves', () => {
		const data = { billing: { amount_cents: 100, currency: 'USD' } };
		const out = redactValue(data, ['billing.amount_cents'], 'force-mask') as {
			billing: { amount_cents: unknown; currency: string };
		};
		expect(out.billing.amount_cents).toBe(REDACTED_PLACEHOLDER);
		expect(out.billing.currency).toBe('USD');
	});

	test('force-mask masks an entire redacted container instead of recursing into it', () => {
		const secret = { sku: 'A1', price: 100 };
		const data = { line_items: [secret], status: 'open' };
		const out = redactValue(data, ['line_items'], 'force-mask') as {
			line_items: unknown;
			status: string;
		};
		expect(out.line_items).toBe(REDACTED_PLACEHOLDER);
		expect(out.status).toBe('open');
		const serialized = JSON.stringify(out);
		expect(serialized.includes('A1')).toBe(false);
		expect(serialized.includes('"price":100')).toBe(false);
	});
});

describe('isRedactedPlaceholder', () => {
	test('returns true only for the placeholder literal', () => {
		expect(isRedactedPlaceholder(REDACTED_PLACEHOLDER)).toBe(true);
		expect(isRedactedPlaceholder('redacted')).toBe(false);
		expect(isRedactedPlaceholder(null)).toBe(false);
		expect(isRedactedPlaceholder(0)).toBe(false);
	});
});
