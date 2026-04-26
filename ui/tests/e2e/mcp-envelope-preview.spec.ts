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

test.describe('MCP envelope preview', () => {
	test('mirrors explainPolicy outcomes for read, needs_approval, and denied flows', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'mcp-envelope-preview');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;
		const requests = captureDataPlaneRequests(page, fixture.db);

		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.goto(policiesUrl);

		await expect(page.getByTestId('mcp-envelope-panel')).toBeVisible();

		// 1. Contractor reads assigned invoice → outcome `allowed`.
		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		await expect(page.getByTestId('policy-reason-code')).toHaveText('allowed');
		await expect(page.getByTestId('mcp-envelope-tool')).toHaveText(
			`${SCN017_COLLECTIONS.invoices}.get`,
		);
		await expect(page.getByTestId('mcp-envelope-subject')).toHaveText(SCN017_SUBJECTS.contractor);
		await expect(page.getByTestId('mcp-envelope-operation')).toHaveText('read');
		await expect(page.getByTestId('mcp-envelope-policy-version')).toHaveText('v1');
		const allowedOutcome = page.getByTestId('mcp-envelope-outcome');
		await expect(allowedOutcome).toHaveText('allowed');
		await expect(allowedOutcome).toHaveAttribute('data-outcome', 'allowed');
		// MCP reason code should match the policy explanation reason exactly.
		const explainReason = await page.getByTestId('policy-reason-code').textContent();
		await expect(page.getByTestId('mcp-envelope-reason')).toHaveText(explainReason ?? 'allowed');

		const allowedReproductionRaw = await page
			.getByTestId('mcp-envelope-reproduction')
			.textContent();
		const allowedReproduction = JSON.parse(allowedReproductionRaw ?? '{}');
		expect(allowedReproduction.tool).toBe(`${SCN017_COLLECTIONS.invoices}.get`);
		expect(allowedReproduction.subject).toBe(SCN017_SUBJECTS.contractor);
		expect(allowedReproduction.outcome).toBe('allowed');
		expect(allowedReproduction.policy_version).toBe(1);
		expect(allowedReproduction.reason_code).toBe('allowed');
		expect(allowedReproduction.arguments).toEqual(
			expect.objectContaining({ collection: SCN017_COLLECTIONS.invoices }),
		);
		// Non-secret invariants: redacted contractor fields must not leak through the
		// reproduction JSON, even if the operator typed them into a fixture editor.
		expect(allowedReproductionRaw ?? '').not.toContain('net-30 standard procurement terms');

		// 2. Finance agent patching the large invoice → outcome `needs_approval`.
		await page.unroute('**/graphql');
		await page.unroute('**/auth/me');
		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
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
		const needsApprovalOutcome = page.getByTestId('mcp-envelope-outcome');
		await expect(needsApprovalOutcome).toHaveText('needs_approval');
		await expect(needsApprovalOutcome).toHaveAttribute('data-outcome', 'needs_approval');
		await expect(page.getByTestId('mcp-envelope-tool')).toHaveText(
			`${SCN017_COLLECTIONS.invoices}.patch`,
		);
		await expect(page.getByTestId('mcp-envelope-approval-role')).toHaveText(
			SCN017_ROLES.financeApprover,
		);
		const needsApprovalReproduction = JSON.parse(
			(await page.getByTestId('mcp-envelope-reproduction').textContent()) ?? '{}',
		);
		expect(needsApprovalReproduction.outcome).toBe('needs_approval');
		expect(needsApprovalReproduction.reason_code).toBe('needs_approval');
		expect(needsApprovalReproduction.arguments.entityId).toBe(fixture.invoices.large.id);

		// 3. Contractor delete on the same invoice → outcome `denied`.
		// Procurement policy has no delete allow rule so default deny applies for
		// every subject, which keeps this assertion stable against envelope drift.
		await page.unroute('**/graphql');
		await page.unroute('**/auth/me');
		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-operation-picker').selectOption('delete');
		await page.getByTestId('policy-run-evaluator').click();

		const deniedOutcome = page.getByTestId('mcp-envelope-outcome');
		await expect(deniedOutcome).toHaveText('denied');
		await expect(deniedOutcome).toHaveAttribute('data-outcome', 'denied');
		await expect(page.getByTestId('mcp-envelope-tool')).toHaveText(
			`${SCN017_COLLECTIONS.invoices}.delete`,
		);
		const deniedReason = await page.getByTestId('mcp-envelope-reason').textContent();
		const deniedExplain = await page.getByTestId('policy-reason-code').textContent();
		expect(deniedReason).toBe(deniedExplain);

		// Copy button is wired and visible for operator workflows.
		await expect(page.getByTestId('mcp-envelope-copy')).toBeVisible();

		expectGraphqlPrimaryDataPlane(requests, 'mcp envelope preview should stay GraphQL-primary');
	});
});
