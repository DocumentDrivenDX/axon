import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_SUBJECTS,
	captureDataPlaneRequests,
	expectGraphqlPrimaryDataPlane,
	routeGraphqlAs,
	seedScn017PolicyUiFixture,
} from './helpers';

test.describe('Policy authoring', () => {
	test('renders the database policies route shell and loads effective policy via GraphQL', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-authoring');
		const databaseUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}`;
		const policiesUrl = `${databaseUrl}/policies`;
		const requests = captureDataPlaneRequests(page, fixture.db);

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(databaseUrl);
		await page.getByRole('link', { name: 'Policies' }).first().click();

		await expect(page).toHaveURL(policiesUrl);
		await expect(page.getByTestId('policy-scope')).toContainText(fixture.tenant.db_name);
		await expect(page.getByTestId('policy-scope')).toContainText(fixture.db.name);

		const collectionPicker = page.getByTestId('policy-collection-picker');
		const subjectPicker = page.getByTestId('policy-subject-picker');
		await expect(collectionPicker).toBeVisible();
		await expect(subjectPicker).toBeVisible();

		await collectionPicker.selectOption(SCN017_COLLECTIONS.invoices);
		await subjectPicker.selectOption(SCN017_SUBJECTS.contractor);

		await expect(page.getByTestId('policy-schema-version')).toHaveText('v1');
		await expect(page.getByTestId('policy-version')).toHaveText('v1');
		await expect(page.getByTestId('policy-sample-entity')).toContainText('invoices/');
		await expect(page.getByTestId('policy-redacted-fields')).toContainText('amount_cents');
		await expect(page.getByTestId('policy-redacted-fields')).toContainText('commercial_terms');
		await expect(page.getByTestId('policy-explanation')).toContainText('allow');
		await expect(page.getByTestId('policy-explanation')).toContainText(
			'contractor-reads-assigned-invoices',
		);

		expectGraphqlPrimaryDataPlane(requests, 'policy route should stay GraphQL-primary');
	});
});
