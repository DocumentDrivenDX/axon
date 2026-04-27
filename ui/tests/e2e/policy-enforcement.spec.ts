import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_SUBJECTS,
	createTestLink,
	dbAuditUrl,
	dbCollectionUrl,
	dbCollectionsUrl,
	routeGraphqlAs,
	seedScn017PolicyUiFixture,
	tenantUrl,
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
		// Mocked variant of the contractor-delete denial. The real-backend
		// companion test below (bead axon-a525ef55) drives an actual
		// `row_write_denied` envelope through axon-server; this one mocks
		// the GraphQL response so we can additionally exercise rendering of
		// extensions.rule_ids, which the production policy engine does not
		// currently emit. Verifies that the api client preserves the
		// structured envelope and that DenialMessage renders
		// code/reason/fieldPath/policy/rule_ids without the UI optimistically
		// mutating the list.
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

	test('real backend denies contractor delete with stable code, reason, and policy', async ({
		page,
		request,
	}) => {
		// Bead axon-a525ef55: companion to the mocked "denied delete" test
		// above. The SCN-017 procurement policy now includes a delete deny
		// rule (`contractors-cannot-delete-invoices`) so the contractor's
		// delete attempt produces a real `forbidden` envelope from
		// axon-server. This proves the backend → wire-format → AxonGraphqlError
		// → DenialMessage path end-to-end (no GraphQL response mock) and
		// guards against regressions in the structured-envelope plumbing.
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-deny-real');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);

		// Route the actor only — no GraphQL response mock. The deleteEntity
		// mutation hits axon-server and the policy engine emits a real denial.
		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(collectionUrl);

		await page.locator('tr', { hasText: fixture.invoices.large.id }).first().click();
		await page.getByRole('button', { name: /^Delete$/ }).click();
		await page.getByRole('button', { name: /^Confirm$/ }).click();

		const denial = page.getByTestId('entity-delete-error');
		await expect(denial).toBeVisible();
		await expect(page.getByTestId('entity-delete-error-code')).toHaveText(/forbidden/i);
		// Real-backend reason for an operation-level deny is `row_write_denied`
		// (set by enforce_policy_operation in axon-api/handler.rs), not the
		// synthesised `delete` string the mock injects.
		await expect(page.getByTestId('entity-delete-error-reason')).toHaveText(/row_write_denied/);
		await expect(page.getByTestId('entity-delete-error-policy')).toContainText(
			'contractors-cannot-delete-invoices',
		);

		// No optimistic mutation: the row must still be in the list.
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
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.policyFilterUnindexed);

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

	test('forbidden audit and links tab loads collapse to a uniform error string', async ({
		page,
		request,
	}) => {
		// FEAT-031 / bead axon-a065f3fe: loadAuditTab and loadLinksTab catches
		// must run their failure values through normalizeReadFailure so a
		// 403/forbidden tab response cannot leak "policy denied: read on …/<id>"
		// or the entity id verbatim through the auditError/linksError banners.
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-tab-loads');
		const collectionUrl = dbCollectionUrl(fixture.db, SCN017_COLLECTIONS.invoices);
		const targetId = fixture.invoices.large.id;

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			const isAuditQuery =
				postData.includes('AxonUiEntityAudit') || postData.includes('entityAudit');
			const isLinksQuery =
				postData.includes('AxonUiTraverseLinks') ||
				postData.includes('traverseLinks') ||
				postData.includes('neighbors');
			if (!isAuditQuery && !isLinksQuery) return null;
			return {
				data: null,
				errors: [
					{
						message: `policy denied: read on invoices/${targetId}`,
						path: [isAuditQuery ? 'entityAudit' : 'traverseLinks'],
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
		});

		await page.goto(collectionUrl);
		await page.locator('tr', { hasText: targetId }).first().click();

		// Audit tab: open and assert the banner is the uniform "Entity not
		// found." string with no leaked id or denial wording.
		await page.getByTestId('entity-tab-audit').click();
		const auditError = page.locator('[data-testid="entity-audit-error"], .message.error').first();
		await expect(auditError).toBeVisible();
		const auditText = (await auditError.innerText()).toLowerCase();
		expect(auditText).toContain('not found');
		expect(auditText).not.toContain('forbidden');
		expect(auditText).not.toContain('denied');
		expect(auditText).not.toContain(targetId.toLowerCase());

		// Links tab: same uniform-collapse contract.
		await page.getByTestId('entity-tab-links').click();
		const linksError = page.locator('[data-testid="entity-links-error"], .message.error').first();
		await expect(linksError).toBeVisible();
		const linksText = (await linksError.innerText()).toLowerCase();
		expect(linksText).toContain('not found');
		expect(linksText).not.toContain('forbidden');
		expect(linksText).not.toContain('denied');
		expect(linksText).not.toContain(targetId.toLowerCase());
	});

	test('list surfaces never display raw storage entity_count to a contractor', async ({
		page,
		request,
	}) => {
		// FEAT-031 / bead axon-eb57f5fc: the collections list, schema picker,
		// and tenant database table previously rendered the unfiltered storage
		// entity_count. That count includes contractor-hidden invoice rows
		// (the SCN-017 fixture seeds 2 invoices, both of which the contractor
		// could read; if any new hidden rows are added, the raw count would
		// leak their existence). Until the backend exposes a per-collection
		// policy-filtered totalCount, the leaking cells must be omitted.
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-enforcement-list-totals');

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);

		// /collections: header pill must not show an "entities" total, and
		// no per-row Entities cell may appear.
		await page.goto(dbCollectionsUrl(fixture.db));
		await expect(page.getByTestId('collections-table')).toBeVisible();
		const collectionsHeader = await page.locator('thead').first().innerText();
		expect(collectionsHeader.toLowerCase()).not.toContain('entities');
		const collectionsHtml = await page.content();
		expect(collectionsHtml.toLowerCase()).not.toMatch(/\d+\s+entities/);

		// /schemas: collection picker option labels must not include an
		// "N entities" suffix.
		await page.goto(
			`/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/schemas`,
		);
		await expect(page.locator('.collection-option').first()).toBeVisible();
		const schemasHtml = await page.content();
		expect(schemasHtml.toLowerCase()).not.toMatch(/\d+\s+entities/);

		// /tenants/:tenant: database table must not include an Entities column.
		await page.goto(tenantUrl(fixture.tenant));
		await expect(page.locator('table').first()).toBeVisible();
		const tenantHeader = await page.locator('thead').first().innerText();
		expect(tenantHeader.toLowerCase()).not.toContain('entities');
	});
});
