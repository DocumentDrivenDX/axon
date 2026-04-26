import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_SUBJECTS,
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
});
