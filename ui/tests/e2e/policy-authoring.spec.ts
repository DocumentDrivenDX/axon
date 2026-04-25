import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_ROLES,
	SCN017_SUBJECTS,
	captureDataPlaneRequests,
	expectGraphqlPrimaryDataPlane,
	routeGraphqlAs,
	seedScn017PolicyUiFixture,
} from './helpers';

test.describe('Policy authoring', () => {
	test('runs read, patch, and transaction policy evaluations from the workspace', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-authoring');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;
		const requests = captureDataPlaneRequests(page, fixture.db);

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await expect(page).toHaveURL(policiesUrl);
		await expect(page.getByTestId('policy-scope')).toContainText(fixture.tenant.db_name);
		await expect(page.getByTestId('policy-scope')).toContainText(fixture.db.name);
		await expect(page.getByTestId('policy-operation-picker')).toBeVisible();
		await expect(page.getByTestId('policy-entity-picker')).toBeVisible();
		await expect(page.getByTestId('policy-sample-row')).toBeVisible();

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		await expect(page.getByTestId('policy-schema-version')).toHaveText('v1');
		await expect(page.getByTestId('policy-version')).toHaveText('v1');
		await expect(page.getByTestId('policy-sample-entity')).toContainText('invoices/');
		await expect(page.getByTestId('policy-redacted-fields')).toContainText('amount_cents');
		await expect(page.getByTestId('policy-redacted-fields')).toContainText('commercial_terms');
		await expect(page.getByTestId('policy-explanation')).toContainText(
			'contractor-reads-assigned-invoices',
		);
		await expect(page.getByTestId('policy-reason-code')).toHaveText('allowed');
		await expect(page.getByTestId('policy-rule-ids')).not.toHaveText('None');

		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.financeAgent);
		await page.getByTestId('policy-entity-picker').selectOption(fixture.invoices.large.id);
		await page.getByTestId('policy-operation-picker').selectOption('patch');
		await page
			.getByTestId('policy-patch-fixture')
			.fill(
				JSON.stringify({ amount_cents: fixture.invoices.large.amountCents + 500_000 }, null, 2),
			);
		await page.getByTestId('policy-run-evaluator').click();

		await expect(page.getByTestId('policy-reason-code')).toHaveText('needs_approval');
		await expect(page.getByTestId('policy-approval-role')).toHaveText(SCN017_ROLES.financeApprover);

		const transactionFixture = [
			{
				updateEntity: {
					collection: fixture.collections.invoices,
					id: fixture.invoices.large.id,
					expectedVersion: 1,
					data: {
						number: fixture.invoices.large.number,
						vendor_id: fixture.invoices.large.vendorId,
						requester_id: fixture.invoices.large.requesterId,
						assigned_contractor_id: fixture.invoices.large.assignedContractorId,
						status: 'submitted',
						amount_cents: fixture.invoices.large.amountCents + 500_000,
						currency: 'USD',
						commercial_terms: 'net-15 expedited infrastructure terms',
					},
				},
			},
		];
		await page.getByTestId('policy-operation-picker').selectOption('transaction');
		await page
			.getByTestId('policy-transaction-fixture')
			.fill(JSON.stringify(transactionFixture, null, 2));
		await page.getByTestId('policy-run-evaluator').click();

		await expect(page.getByTestId('policy-transaction-operations')).toContainText('update');
		await expect(page.getByTestId('policy-transaction-operations')).toContainText('needs_approval');

		expectGraphqlPrimaryDataPlane(requests, 'policy route should stay GraphQL-primary');
	});

	test('surfaces missing-index diagnostics for policy_filter_unindexed fixtures', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-authoring-diagnostics');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page
			.getByTestId('policy-collection-picker')
			.selectOption(SCN017_COLLECTIONS.policyFilterUnindexed);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		await expect(page.getByTestId('policy-diagnostics')).toContainText('policy_filter_unindexed');
		await expect(page.getByTestId('policy-diagnostics')).toContainText('reviewer_email');
		await expect(page.getByTestId('policy-diagnostics')).toContainText('Add an index');
	});
});
