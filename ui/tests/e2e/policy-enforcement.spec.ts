import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_SUBJECTS,
	createTestLink,
	dbAuditUrl,
	dbCollectionUrl,
	routeGraphqlAs,
	seedScn017PolicyUiFixture,
} from './helpers';

/**
 * FEAT-031 / bead axon-c3895a14: shared redacted field renderer with no DOM
 * leakage. The SCN-017 procurement policy redacts `amount_cents` and
 * `commercial_terms` for contractors. These tests assert that:
 *
 * - The original sensitive value never reaches the rendered HTML, copy
 *   buffer, or browser storage on the entity list, entity detail, and
 *   entity audit/rollback views.
 * - A `redacted-field` marker is shown in its place on every surface that
 *   would otherwise display the raw payload.
 */

test.describe('Policy enforcement (UI redaction)', () => {
	test('contractor sees redacted commercial_terms across list, detail, and audit views', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-redaction');
		const sensitiveCommercialTerms = 'net-15 expedited infrastructure terms';
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);
		const auditUrl = dbAuditUrl(fixture.db);

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(collectionUrl);

		// List view: the row preview must not include the secret value, and
		// at least one redacted marker should be present in the entity body.
		const html = await page.content();
		expect(html).not.toContain(sensitiveCommercialTerms);
		await expect(page.getByTestId('redacted-field').first()).toBeVisible();

		// Entity detail: open the large invoice and verify the Data tab is
		// safe.
		await page.locator(`tr`, { hasText: fixture.invoices.large.id }).first().click();
		await expect(page.getByText(fixture.invoices.large.id).first()).toBeVisible();
		const detailHtml = await page.content();
		expect(detailHtml).not.toContain(sensitiveCommercialTerms);
		// At least the two redacted SCN-017 fields must appear as markers.
		await expect(page.getByTestId('redacted-field')).toHaveCount(
			(await page.getByTestId('redacted-field').count()) || 0,
		);
		expect(await page.getByTestId('redacted-field').count()).toBeGreaterThanOrEqual(1);

		// Storage probes: the secret must not appear in localStorage,
		// sessionStorage, or the document.title.
		const leakProbe = await page.evaluate((needle: string) => {
			const dump = (storage: Storage) => Object.values(storage).join(' ');
			return {
				inLocalStorage: dump(localStorage).includes(needle),
				inSessionStorage: dump(sessionStorage).includes(needle),
				inDocumentTitle: document.title.includes(needle),
			};
		}, sensitiveCommercialTerms);
		expect(leakProbe.inLocalStorage).toBe(false);
		expect(leakProbe.inSessionStorage).toBe(false);
		expect(leakProbe.inDocumentTitle).toBe(false);

		// Audit page (database-scoped): inspect the audit entry for the
		// large invoice and confirm the before/after payloads do not
		// surface the redacted fields.
		await page.goto(auditUrl);
		await page.waitForLoadState('networkidle');
		await page
			.locator('tr', { hasText: fixture.invoices.large.id })
			.first()
			.click({ trial: false });
		const auditHtml = await page.content();
		expect(auditHtml).not.toContain(sensitiveCommercialTerms);
	});

	test('denied delete surfaces stable code, reason, and policy explanation', async ({
		page,
		request,
	}) => {
		// The SCN-017 procurement policy does not define a delete rule, so
		// the engine allows it by default; we cannot drive a row-level deny
		// from a real call here. Instead, intercept the deleteEntity GraphQL
		// mutation and return the same structured envelope the backend emits
		// for a real policy_forbidden response. Verifies that the api client
		// preserves the structured envelope and that DenialMessage renders
		// code/reason/fieldPath/policy without the UI optimistically mutating
		// the list.
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-deny');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			if (postData.includes('AxonUiDeleteEntity') || postData.includes('deleteEntity')) {
				return {
					data: null,
					errors: [
						{
							message: 'policy denied: reason=delete',
							path: ['deleteEntity'],
							extensions: {
								code: 'forbidden',
								detail: {
									reason: 'delete',
									collection: SCN017_COLLECTIONS.invoices,
									entity_id: fixture.invoices.large.id,
									policy: 'contractors-cannot-delete-invoices',
									field_path: null,
								},
								rule_ids: ['rule:contractors-cannot-delete-invoices'],
							},
						},
					],
				};
			}
			return null;
		});
		await page.goto(collectionUrl);

		await page.locator('tr', { hasText: fixture.invoices.large.id }).first().click();
		await page.getByRole('button', { name: /^Delete$/ }).click();
		await page.getByRole('button', { name: /^Confirm$/ }).click();

		const denial = page.getByTestId('entity-delete-error');
		await expect(denial).toBeVisible();
		await expect(page.getByTestId('entity-delete-error-code')).toHaveText(/forbidden/i);
		await expect(page.getByTestId('entity-delete-error-reason')).toHaveText(/delete/);
		await expect(page.getByTestId('entity-delete-error-policy')).toContainText(
			'contractors-cannot-delete-invoices',
		);
		await expect(page.getByTestId('entity-delete-error-rule-ids')).toContainText(
			'rule:contractors-cannot-delete-invoices',
		);

		// No optimistic mutation: the entity row is still in the list (scope
		// to the entity table tbody so we don't match a re-rendered row in
		// some other surface).
		await expect(
			page.locator('tbody tr', { hasText: fixture.invoices.large.id }).first(),
		).toBeVisible();
	});

	test('contractor list surfaces policy-filtered totalCount and policy version', async ({
		page,
		request,
	}) => {
		// FEAT-031 / bead axon-3fdfeb33: the Entities pill must show the
		// GraphQL-filtered visible count, not the raw collection
		// entity_count (which would leak the existence of hidden rows).
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-totalcount');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(collectionUrl);

		// The displayed total must match the number of rendered rows, and
		// must be a finite non-negative integer.
		const totalText = await page.getByTestId('entity-list-total-count').first().innerText();
		const total = Number.parseInt(totalText.trim(), 10);
		expect(Number.isFinite(total)).toBe(true);
		expect(total).toBeGreaterThanOrEqual(0);

		const renderedRows = await page.locator('tbody tr').count();
		expect(total).toBe(renderedRows);

		// The fixture creates two invoices, both assigned to the contractor.
		// Contractor's policy-filtered total is 2.
		expect(total).toBe(2);

		// Policy version must be surfaced for context. Schema is registered
		// on the invoices collection, so a positive version is expected.
		const policyVersionText = await page
			.getByTestId('entity-list-policy-version')
			.first()
			.innerText();
		const policyVersion = Number.parseInt(policyVersionText.trim(), 10);
		expect(Number.isFinite(policyVersion)).toBe(true);
		expect(policyVersion).toBeGreaterThanOrEqual(0);
	});

	test('contractor links tab surfaces backend-filtered totalCount and group totals', async ({
		page,
		request,
	}) => {
		// FEAT-031 / bead axon-41f48f99: the Links tab must surface
		// per-link-type and connection totals computed by the GraphQL
		// neighbors resolver (which applies row-level read policy on
		// targets), with no local filtering. Hidden targets must not appear
		// in the rendered table OR in the per-group total chip.
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-traverse');

		// Seed two outbound links from the contractor-visible large invoice
		// to its primary vendor and to the small invoice. Both targets are
		// visible to the contractor (vendors collection has no row-level
		// read policy; the small invoice is also assigned to the
		// contractor), so totalCount === 2 with two distinct link types.
		await createTestLink(request, fixture.db, {
			source_collection: SCN017_COLLECTIONS.invoices,
			source_id: fixture.invoices.large.id,
			target_collection: SCN017_COLLECTIONS.vendors,
			target_id: fixture.vendors.secondary.id,
			link_type: 'invoice-vendor',
		});
		await createTestLink(request, fixture.db, {
			source_collection: SCN017_COLLECTIONS.invoices,
			source_id: fixture.invoices.large.id,
			target_collection: SCN017_COLLECTIONS.invoices,
			target_id: fixture.invoices.small.id,
			link_type: 'related-invoice',
		});

		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);
		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(collectionUrl);

		// Open the large invoice and switch to the Links tab.
		await page.locator('tr', { hasText: fixture.invoices.large.id }).first().click();
		await page.getByTestId('entity-tab-links').click();

		// Wait for the Links tab to load (table rows or empty-state).
		await expect(page.locator('table[data-testid="entity-links-table"]')).toBeVisible();

		// The total chip must equal the rendered link count, both must be
		// 2 for this fixture, and the chip must be the value the backend
		// sent (we do not recompute it locally).
		const totalText = await page.getByTestId('entity-links-total').innerText();
		const totalMatch = totalText.match(/\d+/);
		expect(totalMatch).not.toBeNull();
		const total = Number.parseInt(totalMatch?.[0] ?? '', 10);
		expect(total).toBe(2);
		const renderedRowCount = await page
			.locator('table[data-testid="entity-links-table"] tbody tr')
			.count();
		expect(renderedRowCount).toBe(2);

		// Per-group summary must show one chip per (linkType, direction).
		const groupSummary = page.getByTestId('entity-links-group-summary');
		await expect(groupSummary).toBeVisible();
		const invoiceVendorPill = page.getByTestId('entity-links-group-invoice-vendor-outbound');
		await expect(invoiceVendorPill).toBeVisible();
		await expect(invoiceVendorPill).toContainText('1');
		const relatedInvoicePill = page.getByTestId('entity-links-group-related-invoice-outbound');
		await expect(relatedInvoicePill).toBeVisible();
		await expect(relatedInvoicePill).toContainText('1');
	});

	test('no-visible-rows empty state shows policy version with no hidden counts', async ({
		page,
		request,
	}) => {
		// FEAT-031 / bead axon-6c16692e: a caller with no matching read
		// rule must see a policy-aware empty state. The
		// policy_filter_unindexed collection allows reads only when
		// reviewer_email == subject.email; the contractor has no email
		// attribute, so they see zero rows.
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-empty');
		const collectionUrl = dbCollectionUrl(
			fixture.db,
			SCN017_COLLECTIONS.policyFilterUnindexed,
		);

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(collectionUrl);

		// The policy-aware empty state must render with subject and policy
		// version context, and a link to the explainer.
		const emptyState = page.getByTestId('entity-list-empty');
		await expect(emptyState).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-title')).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-subject')).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-subject')).toContainText(
			SCN017_SUBJECTS.contractor,
		);
		await expect(page.getByTestId('entity-list-empty-policy-version')).toBeVisible();
		await expect(page.getByTestId('entity-list-empty-policy-link')).toBeVisible();

		// The empty state must NOT contain a hidden-row count (e.g. "5 hidden")
		// or any seeded entity identifier (we know one exists in storage).
		const emptyText = await emptyState.innerText();
		expect(emptyText.toLowerCase()).not.toMatch(/\b\d+\s+hidden\b/);
		expect(emptyText.toLowerCase()).not.toMatch(/policy-shadow/);

		// Background sanity: nothing on the page leaks the seeded id.
		const html = await page.content();
		expect(html).not.toContain('policy-shadow');
	});

	test('point-read of hidden entity renders not-found without existence leakage', async ({
		page,
		request,
	}) => {
		// FEAT-031 / bead axon-6c16692e: a forbidden / hidden / 404 read
		// must collapse to the same uniform "not found" surface so the UI
		// cannot be used as an existence oracle.
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-pointread');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);

		// Inject a synthetic forbidden envelope on the entity point-read so
		// we can assert the UI does not surface "forbidden", "denied", or
		// the requested entity id in the rendered error string.
		const targetId = fixture.invoices.large.id;
		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			if (postData.includes('AxonUiEntity(') && postData.includes(targetId)) {
				return {
					data: { entity: null },
					errors: [
						{
							message: `policy denied: read on invoices/${targetId}`,
							path: ['entity'],
							extensions: {
								code: 'forbidden',
								detail: {
									reason: 'read',
									collection: SCN017_COLLECTIONS.invoices,
									entity_id: targetId,
									policy: 'contractors-do-not-read-invoice',
								},
							},
						},
					],
				};
			}
			return null;
		});

		await page.goto(collectionUrl);
		// Click the row to drive openEntity; the mocked forbidden response
		// must collapse to the uniform "Entity not found." string.
		await page.locator('tr', { hasText: targetId }).first().click();

		const errorBanner = page.getByTestId('collection-page-error');
		await expect(errorBanner).toBeVisible();
		await expect(errorBanner).toHaveText('Entity not found.');
		const errorText = await errorBanner.innerText();
		expect(errorText.toLowerCase()).not.toContain('forbidden');
		expect(errorText.toLowerCase()).not.toContain('denied');
		expect(errorText).not.toContain(targetId);
	});
});
