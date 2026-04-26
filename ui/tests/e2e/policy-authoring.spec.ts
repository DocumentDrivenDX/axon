import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_ROLES,
	SCN017_SUBJECTS,
	captureDataPlaneRequests,
	expectGraphqlPrimaryDataPlane,
	fetchPersistedAccessControl,
	proposedPolicyDraftBroken,
	proposedPolicyDraftDenyHigh,
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

test.describe('Policy authoring (impact matrix)', () => {
	test('renders subject × operation × fixture-row outcomes for the active policy', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-impact-matrix');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		const matrix = page.getByTestId('policy-impact-matrix');
		await expect(matrix).toBeVisible();
		// Wait for at least one cell to populate.
		await expect(matrix.getByTestId('policy-impact-matrix-cell').first()).toHaveAttribute(
			'data-decision',
			/(allowed|denied|needs_approval)/,
		);

		// A read against the contractor should be allowed with amount_cents redacted on a small invoice.
		const contractorReadSmall = matrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.small.id}"][data-subject-id="${SCN017_SUBJECTS.contractor}"][data-operation="read"]`,
		);
		await expect(contractorReadSmall).toHaveAttribute('data-decision', 'allowed');
		await expect(
			contractorReadSmall.getByTestId('policy-impact-matrix-redacted-fields'),
		).toContainText('amount_cents');

		// Finance-agent patching the large invoice should need approval.
		const financePatchLarge = matrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.large.id}"][data-subject-id="${SCN017_SUBJECTS.financeAgent}"][data-operation="patch"]`,
		);
		await expect(financePatchLarge).toHaveAttribute('data-decision', 'needs_approval');
		await expect(
			financePatchLarge.getByTestId('policy-impact-matrix-approval-role'),
		).toContainText(SCN017_ROLES.financeApprover);
	});

	test('surfaces policy_filter_unindexed remediation in the matrix', async ({ page, request }) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-impact-matrix-unindexed');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page
			.getByTestId('policy-collection-picker')
			.selectOption(SCN017_COLLECTIONS.policyFilterUnindexed);

		const matrix = page.getByTestId('policy-impact-matrix');
		await expect(matrix).toBeVisible();
		const diagnostic = matrix.getByTestId('policy-impact-matrix-diagnostic').first();
		await expect(diagnostic).toContainText('policy_filter_unindexed');
		await expect(diagnostic).toContainText('reviewer_email');
	});
});

test.describe('Policy authoring (schemas tab)', () => {
	test('compile + fixture dry-run + activate updates the persisted policy', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'schemas-policy-happy');
		const schemasUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/schemas?collection=${encodeURIComponent(SCN017_COLLECTIONS.invoices)}`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(schemasUrl);

		// Open the Policy view.
		await page.getByTestId('schema-policy-view-toggle').click();
		await expect(page.getByTestId('schema-policy-view')).toBeVisible();
		// Textareas surface their text via `value`, not innerText.
		await expect(page.getByTestId('schema-policy-editor')).toHaveValue(
			/finance-and-operators-read-invoices/,
		);

		// Replace the editor contents with the proposed (tightened) policy.
		const proposed = proposedPolicyDraftDenyHigh();
		await page.getByTestId('schema-policy-editor').fill(JSON.stringify(proposed, null, 2));

		// Compile preview.
		await page.getByTestId('schema-policy-run-compile').click();
		await expect(page.getByTestId('schema-policy-errors')).toHaveCount(0);
		await expect(page.getByTestId('schema-policy-nullable-fields')).toContainText('amount_cents');
		await expect(page.getByTestId('schema-policy-envelopes')).toContainText('needs_approval');
		await expect(page.getByTestId('schema-policy-envelopes')).toContainText(
			SCN017_ROLES.financeApprover,
		);

		// Fixture dry-run as the finance agent: a large invoice patch passes
		// the update.allow rule and then routes through the
		// require-approval-large-invoice-update envelope, requiring the
		// finance-approver role.
		await page.getByTestId('schema-policy-fixture-subject').fill(SCN017_SUBJECTS.financeAgent);
		await page.getByTestId('schema-policy-fixture-operation').selectOption('patch');
		await page.getByTestId('schema-policy-fixture-entity').fill(fixture.invoices.large.id);
		await page
			.getByTestId('schema-policy-fixture-patch')
			.fill(
				JSON.stringify(
					{ amount_cents: fixture.invoices.large.amountCents + 500_000 },
					null,
					2,
				),
			);
		await page.getByTestId('schema-policy-fixture-run').click();
		await expect(page.getByTestId('schema-policy-fixture-decision')).toContainText(
			'needs_approval',
		);
		await expect(page.getByTestId('schema-policy-fixture-approval-role')).toContainText(
			SCN017_ROLES.financeApprover,
		);

		// Activate. Audit metadata records old + new schema/policy versions
		// (covered by axon-server contract test); here we verify the UI flow.
		await page.getByTestId('schema-policy-activate').click();
		await expect(page.getByTestId('schema-policy-activation-status')).toContainText(
			'Activated policy version v2',
		);

		// Persisted access_control reflects the new policy.
		const persisted = (await fetchPersistedAccessControl(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
		)) as { fields?: { amount_cents?: { read?: { deny?: unknown[] } } } } | null;
		expect(persisted, 'persisted access_control after activate').toBeTruthy();
		expect(persisted?.fields?.amount_cents?.read?.deny?.length ?? 0).toBeGreaterThanOrEqual(2);
	});

	test('failed compile blocks activation and leaves the persisted policy unchanged', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'schemas-policy-broken');
		const schemasUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/schemas?collection=${encodeURIComponent(SCN017_COLLECTIONS.invoices)}`;

		const before = await fetchPersistedAccessControl(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
		);

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(schemasUrl);
		await page.getByTestId('schema-policy-view-toggle').click();

		const broken = proposedPolicyDraftBroken();
		await page.getByTestId('schema-policy-editor').fill(JSON.stringify(broken, null, 2));
		await page.getByTestId('schema-policy-run-compile').click();

		const errorsPanel = page.getByTestId('schema-policy-errors');
		await expect(errorsPanel).toBeVisible();
		await expect(errorsPanel).toContainText('policy_expression_invalid');
		await expect(errorsPanel).toContainText('unknown subject reference');
		await expect(page.getByTestId('schema-policy-error-row-0')).toBeFocused();

		// Activate must be disabled while errors are present.
		await expect(page.getByTestId('schema-policy-activate')).toBeDisabled();

		// Persisted access_control unchanged — no activation occurred.
		const after = await fetchPersistedAccessControl(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
		);
		expect(after).toEqual(before);
	});
});
