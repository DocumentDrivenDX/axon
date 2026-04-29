import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_SUBJECTS,
	createTestLink,
	dbCollectionUrl,
	routeGraphqlAs,
	seedScn017PolicyUiFixture,
} from './helpers';

test.describe('Link preview policy retry', () => {
	test('retries policy fetch on subsequent toggle after transient failure', async ({
		page,
		request,
	}) => {
		// BEAD hx-92456dd2: ensureTargetPolicy should distinguish between
		// successful and failed fetches. When a fetch fails (transient network
		// error), the cache should store a { failed: true } sentinel, allowing
		// the next expand of a row in that collection to retry rather than
		// permanently using a permissive fallback.
		//
		// This test verifies the fix by:
		// 1. Mocking the policy endpoint to fail on first call
		// 2. Clicking to expand a link (ensureTargetPolicy called)
		// 3. Verifying data renders but [redacted] markers are absent (permissive fallback)
		// 4. Mocking the policy endpoint to succeed on second call
		// 5. Closing the link preview, then clicking again (ensureTargetPolicy retries)
		// 6. Verifying [redacted] markers now appear (policy fetch succeeded)

		const fixture = await seedScn017PolicyUiFixture(request, 'link-policy-retry');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);

		// Seed an outbound link from large → small invoice.
		// The target invoice has redacted_fields=['amount_cents', 'commercial_terms']
		// in the contractor's policy, so if the policy fetch succeeds, we expect
		// [redacted] markers on those fields.
		await createTestLink(request, fixture.db, {
			source_collection: SCN017_COLLECTIONS.invoices,
			source_id: fixture.invoices.large.id,
			target_collection: SCN017_COLLECTIONS.invoices,
			target_id: fixture.invoices.small.id,
			link_type: 'related-invoice',
		});

		let policyFetchCount = 0;

		// Set up mocking for the GraphQL policy query.
		// First call will return an error to simulate a transient failure.
		// Second call will return the real policy.
		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			if (postData.includes('AxonUiEffectivePolicy')) {
				policyFetchCount += 1;
				if (policyFetchCount === 1) {
					// First call: return a server error
					return {
						data: null,
						errors: [
							{
								message: 'Failed to fetch effective policy',
								extensions: { code: 'INTERNAL_SERVER_ERROR' },
							},
						],
					};
				}
				// Second call: return null to pass through to the real server
			}
			return null;
		});
		await page.goto(collectionUrl);

		// Click the large invoice row to select it.
		await page.locator('tr', { hasText: fixture.invoices.large.id }).first().click();

		// Switch to the Links tab.
		await page.getByTestId('entity-tab-links').click();
		await expect(page.locator('table[data-testid="entity-links-table"]')).toBeVisible();

		// Find the "Show data" button for the related-invoice link (target: small invoice).
		const linkRowTestId = `related-invoice-${fixture.invoices.small.id}`;
		const toggleButton = page.getByTestId(`entity-link-preview-toggle-${linkRowTestId}`);
		await expect(toggleButton).toBeVisible();

		// ── First expand: policy fetch fails ──────────────────────────────────
		// Click to expand the link. The policy fetch will fail, so the
		// cached value becomes { failed: true }. The preview renders with
		// empty redactedFields, so [redacted] markers do NOT appear.
		await toggleButton.click();
		const previewRow = page.getByTestId(`entity-link-preview-${linkRowTestId}`);
		await expect(previewRow).toBeVisible({ timeout: 10_000 });

		// With a permissive fallback (empty redactedFields), the sensitive
		// fields should render as the server-supplied values or null.
		// The field JSON should be visible, and we should NOT see [redacted].
		const firstPreviewContent = await previewRow.innerText();
		expect(firstPreviewContent).not.toContain('[redacted]');

		// ── Second expand: policy fetch succeeds ────────────────────────────
		// Close the preview by clicking the toggle again.
		await toggleButton.click();
		await expect(previewRow).not.toBeVisible();

		// Reset the mock to allow the policy fetch to succeed on the next call.
		// Actually, our mock above already handles this: policyFetchCount === 1
		// only on the first call, so the second call passes through.

		// Click to expand again. Now ensureTargetPolicy will retry because
		// the cached value is { failed: true }, and the fetch will succeed.
		await toggleButton.click();
		await expect(previewRow).toBeVisible({ timeout: 10_000 });

		// With the correct policy (redacted_fields=['amount_cents', 'commercial_terms']),
		// those fields should now render as [redacted] markers.
		const secondPreviewContent = await previewRow.innerText();
		expect(secondPreviewContent).toContain('[redacted]');

		// Verify the policy was retried (second fetch should have been attempted).
		expect(policyFetchCount).toBeGreaterThanOrEqual(2);
	});
});
