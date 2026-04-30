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

		// One-shot fail flag — armed right before the action that should fail,
		// so initial page-load policy fetches (source-collection list view)
		// always pass through to the real server. The page and the link
		// target collection are the same in this test, so the GraphQL
		// operation/variables are identical for both source and target
		// fetches; timing is the only reliable discriminator.
		let nextPolicyFetchShouldFail = false;
		const policyFetches = { failed: 0, succeeded: 0 };

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			if (postData.includes('AxonUiEffectivePolicy')) {
				if (nextPolicyFetchShouldFail) {
					nextPolicyFetchShouldFail = false;
					policyFetches.failed += 1;
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
				policyFetches.succeeded += 1;
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

		// ── First expand: arm fail flag so the target-policy fetch errors ──
		nextPolicyFetchShouldFail = true;
		await toggleButton.click();
		const previewRow = page.getByTestId(`entity-link-preview-${linkRowTestId}`);
		await expect(previewRow).toBeVisible({ timeout: 10_000 });

		// Cache should now hold { failed: true }; targetRedactedFields returns
		// []; redactValue should not introduce [redacted] markers.
		const firstPreviewContent = await previewRow.innerText();
		expect(firstPreviewContent).not.toContain('[redacted]');
		expect(policyFetches.failed).toBe(1);

		// ── Second expand: flag is disarmed, fetch succeeds, retry happens ─
		await toggleButton.click();
		await expect(previewRow).not.toBeVisible();

		const succeededBefore = policyFetches.succeeded;
		await toggleButton.click();
		await expect(previewRow).toBeVisible({ timeout: 10_000 });

		// Successful policy fetch with redacted_fields=['amount_cents',
		// 'commercial_terms'] → those fields render as [redacted].
		const secondPreviewContent = await previewRow.innerText();
		expect(secondPreviewContent).toContain('[redacted]');

		// Verify the cache was reissued (a new fetch happened after the
		// failure — proves the cache wasn't poisoned by the permissive
		// fallback).
		expect(policyFetches.succeeded).toBeGreaterThan(succeededBefore);
	});

	test('rapid double-click on link preview toggle leaves row collapsed while fetch is in flight', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'link-preview-double-click');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);

		await createTestLink(request, fixture.db, {
			source_collection: SCN017_COLLECTIONS.invoices,
			source_id: fixture.invoices.large.id,
			target_collection: SCN017_COLLECTIONS.invoices,
			target_id: fixture.invoices.small.id,
			link_type: 'related-invoice',
		});

		// One-shot stall flag — armed right before the toggle click so the
		// page-load source-policy fetch always completes. Otherwise the page
		// would deadlock waiting for its own initial AxonUiEffectivePolicy
		// query, the row would never render, and the test would time out
		// before exercising the double-click race at all.
		let stallNextPolicyFetch = false;
		let resolvePolicyFetch!: () => void;
		const policyFetchPending = new Promise<void>((resolve) => {
			resolvePolicyFetch = resolve as () => void;
		});

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			if (postData.includes('AxonUiEffectivePolicy') && stallNextPolicyFetch) {
				stallNextPolicyFetch = false;
				await policyFetchPending;
			}
			return null;
		});
		await page.goto(collectionUrl);
		await page.locator('tr', { hasText: fixture.invoices.large.id }).first().click();
		await page.getByTestId('entity-tab-links').click();

		const linkRowTestId = `related-invoice-${fixture.invoices.small.id}`;
		const toggleButton = page.getByTestId(`entity-link-preview-toggle-${linkRowTestId}`);
		const previewRow = page.getByTestId(`entity-link-preview-${linkRowTestId}`);

		stallNextPolicyFetch = true;
		await toggleButton.click();
		await toggleButton.click();
		resolvePolicyFetch();

		await expect(previewRow).toBeHidden({ timeout: 10_000 });
		await expect(page.getByTestId('redacted-field')).toHaveCount(0);
	});
});
