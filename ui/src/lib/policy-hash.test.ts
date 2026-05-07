import { describe, expect, it } from 'bun:test';
import { computeAccessControlHash } from './policy-hash';

describe('computeAccessControlHash', () => {
	it('returns exactly 16 hex characters', async () => {
		const hash = await computeAccessControlHash({});
		expect(hash).toMatch(/^[0-9a-f]{16}$/);
	});

	it('produces the same hash regardless of top-level key order', async () => {
		const h1 = await computeAccessControlHash({ b: 1, a: 2 });
		const h2 = await computeAccessControlHash({ a: 2, b: 1 });
		expect(h1).toBe(h2);
	});

	it('produces different hashes for different content', async () => {
		const h1 = await computeAccessControlHash({ rules: ['allow'] });
		const h2 = await computeAccessControlHash({ rules: ['deny'] });
		expect(h1).not.toBe(h2);
	});

	it('sorts nested object keys so key order does not affect the hash', async () => {
		const h1 = await computeAccessControlHash({
			fields: { b: { deny: [] }, a: { allow: true } },
		});
		const h2 = await computeAccessControlHash({
			fields: { a: { allow: true }, b: { deny: [] } },
		});
		expect(h1).toBe(h2);
	});

	it('is stable — same input produces same hash across calls', async () => {
		const obj = { rules: [{ name: 'test', when: { subject: 'user', eq: 'admin' } }] };
		const h1 = await computeAccessControlHash(obj);
		const h2 = await computeAccessControlHash(obj);
		expect(h1).toBe(h2);
	});

	it('treats null and empty object as different', async () => {
		const h1 = await computeAccessControlHash(null);
		const h2 = await computeAccessControlHash({});
		expect(h1).not.toBe(h2);
	});

	it('ignores whitespace differences in equivalent JSON', async () => {
		// Both parse to the same structure; the canonical form strips whitespace.
		const obj = { read: { deny: [{ name: 'block-all', when: { subject: 'role', eq: 'guest' } }] } };
		const h1 = await computeAccessControlHash(obj);
		const h2 = await computeAccessControlHash(JSON.parse(JSON.stringify(obj, null, 4)));
		expect(h1).toBe(h2);
	});
});
