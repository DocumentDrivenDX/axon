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

		await createTestLink(request, fixture.db, {
			source_collection: SCN017_COLLECTIONS.invoices,
			source_id: fixture.invoices.large.id,
			target_collection: SCN017_COLLECTIONS.invoices,
			target_id: fixture.invoices.small.id,
			link_type: 'related-invoice',
		});

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
		await page.locator('tr', { hasText: fixture.invoices.large.id }).first().click();
		await page.getByTestId('entity-tab-links').click();

		const linkRowTestId = `related-invoice-${fixture.invoices.small.id}`;
		const toggleButton = page.getByTestId(`entity-link-preview-toggle-${linkRowTestId}`);
		await expect(toggleButton).toBeVisible();

		nextPolicyFetchShouldFail = true;
		await toggleButton.click();
		const previewRow = page.getByTestId(`entity-link-preview-${linkRowTestId}`);
		await expect(previewRow).toBeVisible({ timeout: 10_000 });
		const firstPreviewContent = await previewRow.innerText();
		expect(firstPreviewContent).not.toContain('[redacted]');
		expect(policyFetches.failed).toBe(1);

		await toggleButton.click();
		await expect(previewRow).not.toBeVisible();

		const succeededBefore = policyFetches.succeeded;
		await toggleButton.click();
		await expect(previewRow).toBeVisible({ timeout: 10_000 });
		const secondPreviewContent = await previewRow.innerText();
		expect(secondPreviewContent).toContain('[redacted]');
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

		let resolvePolicyFetch!: () => void;
		const policyFetchPending = new Promise<void>((resolve) => {
			resolvePolicyFetch = resolve;
		});

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			if (postData.includes('AxonUiEffectivePolicy')) {
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

		await toggleButton.click();
		await toggleButton.click();
		resolvePolicyFetch();

		await expect(previewRow).toBeHidden({ timeout: 10_000 });
		await expect(page.getByTestId('redacted-field')).toHaveCount(0);
	});

	test('no-visible-rows empty state shows policy version with no hidden counts', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-empty');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.policyFilterUnindexed);

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(collectionUrl);

		const emptyState = page.getByTestId('entity-list-empty');
		await expect(emptyState).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-title')).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-subject')).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-subject')).toContainText(
			SCN017_SUBJECTS.contractor,
		);
		await expect(page.getByTestId('entity-list-empty-policy-version')).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-policy-link')).toBeVisible();

		const emptyText = await emptyState.innerText();
		expect(emptyText.toLowerCase()).not.toMatch(/\b\d+\s+hidden\b/);
		expect(emptyText.toLowerCase()).not.toMatch(/policy-shadow/);

		const html = await page.content();
		expect(html).not.toContain(fixture.db);
	});
});
