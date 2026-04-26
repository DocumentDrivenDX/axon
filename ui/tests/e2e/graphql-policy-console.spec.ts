import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_ROLES,
	SCN017_SUBJECTS,
	routeGraphqlAs,
	seedScn017PolicyUiFixture,
} from './helpers';

function escapeRegExp(value: string): string {
	return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

test.describe('GraphQL policy console', () => {
	test('opens an effectivePolicy preset from the policy workspace', async ({ page, request }) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'graphql-policy-console-effective');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(policiesUrl);

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-entity-picker').selectOption(fixture.invoices.small.id);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		await expect(page.getByTestId('policy-redacted-fields')).toContainText('amount_cents');
		await expect(page.getByTestId('policy-redacted-fields')).toContainText('commercial_terms');

		await page.getByTestId('policy-open-effective-graphql').click();

		await expect(page).toHaveURL(/\/graphql\?/);
		await expect(page.getByTestId('graphql-preset')).toHaveText('effectivePolicy');
		await expect(page.getByTestId('graphql-actor')).toHaveValue(SCN017_SUBJECTS.contractor);
		await expect(page.getByTestId('graphql-query')).toHaveValue(/effectivePolicy/);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(
			new RegExp(`"collection": "${escapeRegExp(fixture.collections.invoices)}"`),
		);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(
			new RegExp(`"entityId": "${escapeRegExp(fixture.invoices.small.id)}"`),
		);

		await page.getByRole('button', { name: /Run/ }).click();
		await expect(page.getByTestId('graphql-response')).toContainText('"effectivePolicy"');
		await expect(page.getByTestId('graphql-response')).toContainText('"canRead": true');
		await expect(page.getByTestId('graphql-response')).toContainText('amount_cents');
		await expect(page.getByTestId('graphql-response')).toContainText('commercial_terms');
	});

	test('opens an explainPolicy preset from the policy workspace', async ({ page, request }) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'graphql-policy-console-explain');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
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

		await page.getByTestId('policy-open-explain-graphql').click();

		await expect(page).toHaveURL(/\/graphql\?/);
		await expect(page.getByTestId('graphql-preset')).toHaveText('explainPolicy');
		await expect(page.getByTestId('graphql-actor')).toHaveValue(SCN017_SUBJECTS.financeAgent);
		await expect(page.getByTestId('graphql-query')).toHaveValue(/explainPolicy/);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(/"operation": "patch"/);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(
			new RegExp(`"entityId": "${escapeRegExp(fixture.invoices.large.id)}"`),
		);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(/"amount_cents"/);

		await page.getByRole('button', { name: /Run/ }).click();
		await expect(page.getByTestId('graphql-response')).toContainText('"explainPolicy"');
		await expect(page.getByTestId('graphql-response')).toContainText(
			'"decision": "needs_approval"',
		);
		await expect(page.getByTestId('graphql-response')).toContainText('"reason": "needs_approval"');
		await expect(page.getByTestId('graphql-response')).toContainText(
			`"role": "${SCN017_ROLES.financeApprover}"`,
		);
	});

	test('opens an explainPolicy preset from an impact-matrix row', async ({ page, request }) => {
		const fixture = await seedScn017PolicyUiFixture(
			request,
			'graphql-policy-console-impact-matrix',
		);
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		const matrix = page.getByTestId('policy-impact-matrix');
		await expect(matrix).toBeVisible();

		const contractorReadSmall = matrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.small.id}"][data-subject-id="${SCN017_SUBJECTS.contractor}"][data-operation="read"]`,
		);
		await expect(contractorReadSmall).toHaveAttribute('data-decision', 'allowed');
		await contractorReadSmall.getByTestId('policy-impact-matrix-open-graphql').click();

		await expect(page).toHaveURL(/\/graphql\?/);
		await expect(page.getByTestId('graphql-preset')).toHaveText('explainPolicy');
		await expect(page.getByTestId('graphql-actor')).toHaveValue(SCN017_SUBJECTS.contractor);
		await expect(page.getByTestId('graphql-query')).toHaveValue(/explainPolicy/);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(/"operation": "read"/);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(
			new RegExp(`"entityId": "${escapeRegExp(fixture.invoices.small.id)}"`),
		);
	});
});
