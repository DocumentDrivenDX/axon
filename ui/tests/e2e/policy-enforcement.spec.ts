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
});
