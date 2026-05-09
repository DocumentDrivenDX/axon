import { expect, test, type APIRequestContext, type Page } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_ROLES,
	SCN017_SUBJECTS,
	activateProposedPolicy,
	captureDataPlaneRequests,
	expectGraphqlPrimaryDataPlane,
	fetchPersistedAccessControl,
	graphqlPath,
	proposedPolicyDraftBroken,
	proposedPolicyDraftDenyHigh,
	routeGraphqlAs,
	seedScn017PolicyUiFixture,
} from './helpers';

const IMPACT_MATRIX_DELTA_OPERATIONS = ['read', 'create', 'update', 'patch', 'delete'] as const;
const IMPACT_MATRIX_VISIBLE_SUBJECTS = [
	SCN017_SUBJECTS.contractor,
	SCN017_SUBJECTS.financeAgent,
	SCN017_SUBJECTS.financeApprover,
] as const;

type DraftProvider = () => Record<string, unknown> | null;

async function fetchCollectionSchema(
	request: APIRequestContext,
	db: Awaited<ReturnType<typeof seedScn017PolicyUiFixture>>['db'],
	collection: string,
): Promise<Record<string, unknown>> {
	const response = await request.post(graphqlPath(db), {
		data: {
			query: `query($name: String!) { collection(name: $name) { schema } }`,
			variables: { name: collection },
		},
	});
	const body = (await response.json()) as {
		data?: { collection?: { schema?: Record<string, unknown> } };
		errors?: unknown;
	};
	expect(response.ok(), `${response.status()} ${JSON.stringify(body)}`).toBe(true);
	expect(body.errors ?? null, JSON.stringify(body.errors)).toBeNull();
	const schema = body.data?.collection?.schema;
	expect(schema, `schema for ${collection}`).toBeTruthy();
	return schema ?? {};
}

function matrixDeltaDraft(): Record<string, unknown> {
	const policy = proposedPolicyDraftDenyHigh();
	const fields = policy.fields as Record<string, Record<string, unknown> | undefined>;
	const commercialTerms = fields.commercial_terms as Record<string, unknown>;
	const commercialRead = commercialTerms.read as { deny: Array<Record<string, unknown>> };
	commercialTerms.read = {
		deny: [
			...commercialRead.deny,
			{
				name: 'matrix-redacts-commercial-terms-for-finance-agent',
				when: { subject: 'procurement_role', eq: SCN017_ROLES.financeAgent },
				redact_as: null,
			},
		],
	};

	const amountCents = fields.amount_cents as Record<string, unknown>;
	amountCents.write = {
		deny: [
			{
				name: 'matrix-denies-amount-writes-for-finance-agent',
				when: { subject: 'procurement_role', eq: SCN017_ROLES.financeAgent },
			},
		],
	};

	const read = policy.read as { deny?: Array<Record<string, unknown>> };
	read.deny = [
		...(read.deny ?? []),
		{
			name: 'matrix-denies-contractor-reads',
			when: { subject: 'procurement_role', eq: SCN017_ROLES.contractor },
		},
	];

	return policy;
}

async function routeGraphqlWithDraft(
	page: Page,
	baseSchema: Record<string, unknown>,
	currentDraft: DraftProvider,
) {
	await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent, (postData) => {
		const parsed = JSON.parse(postData) as {
			query?: string;
			variables?: { name?: string; policyOverride?: Record<string, unknown> | null };
		};
		const query = parsed.query ?? '';
		const variables = parsed.variables ?? {};

		if (
			query.includes('query AxonUiCollection') &&
			variables.name === SCN017_COLLECTIONS.invoices
		) {
			const draft = currentDraft();
			if (!draft) return null;
			return {
				data: {
					collection: {
						name: SCN017_COLLECTIONS.invoices,
						entityCount: 2,
						schemaVersion: baseSchema.version ?? 1,
						schema: { ...baseSchema, draft: { access_control: draft } },
					},
				},
			};
		}

		const policyOverride = variables.policyOverride;
		const isBrokenOverride = JSON.stringify(policyOverride ?? {}).includes(
			'broken-unknown-subject',
		);
		if (query.includes('query AxonUiExplainPolicy') && isBrokenOverride) {
			return {
				data: { explainPolicy: null },
				errors: [
					{
						message: 'policy_filter_unindexed',
						extensions: {
							code: 'POLICY_FILTER_UNINDEXED',
							detail: {
								reason: 'policy_filter_unindexed',
								collection: SCN017_COLLECTIONS.invoices,
								missing_index: 'reviewer_email',
								candidate_count: 2,
								cost_limit: 1,
							},
						},
					},
				],
			};
		}

		return null;
	});
}

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

		const updateData = {
			number: fixture.invoices.large.number,
			vendor_id: fixture.invoices.large.vendorId,
			requester_id: fixture.invoices.large.requesterId,
			assigned_contractor_id: fixture.invoices.large.assignedContractorId,
			status: 'submitted',
			amount_cents: fixture.invoices.large.amountCents + 500_000,
			currency: 'USD',
			commercial_terms: 'net-15 expedited infrastructure terms',
		};
		await page.getByTestId('policy-operation-picker').selectOption('transaction');
		// The structured editor renders with one default op; configure it as updateEntity.
		const op0 = page.getByTestId('transaction-fixture-op').nth(0);
		await op0.getByTestId('transaction-fixture-op-kind').selectOption('updateEntity');
		await op0
			.getByTestId('transaction-fixture-op-collection')
			.fill(fixture.collections.invoices);
		await op0.getByTestId('transaction-fixture-op-id').fill(fixture.invoices.large.id);
		await op0.getByTestId('transaction-fixture-op-expected-version').fill('1');
		await op0
			.getByTestId('transaction-fixture-op-data')
			.fill(JSON.stringify(updateData, null, 2));
		await page.getByTestId('policy-run-evaluator').click();

		await expect(page.getByTestId('policy-transaction-operations')).toContainText('update');
		await expect(page.getByTestId('policy-transaction-operations')).toContainText('needs_approval');

		expectGraphqlPrimaryDataPlane(requests, 'policy route should stay GraphQL-primary');
	});

	test('policy-version testid increments after policy activation', async ({ page, request }) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-version-update');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		// Baseline: both indicators at v1.
		await expect(page.getByTestId('policy-version')).toHaveText('v1');

		// Activate a new policy version via API, then reload the page to pick up the new state.
		await activateProposedPolicy(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
			proposedPolicyDraftDenyHigh(),
		);

		await page.goto(policiesUrl);
		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		// policy-version must reflect the activated version (v2).
		await expect(page.getByTestId('policy-version')).toHaveText('v2');
	});

	// AC2: policy-schema-version update after schema migration.
	// activateProposedPolicy bumps schema.version by 1 (it writes schema.version + 1 in putSchema),
	// which is also the field driving the policy-schema-version indicator.  The page reloads the
	// collections list on navigation, so the indicator updates after the same activation + reload
	// pattern used for policy-version above.
	test('policy-schema-version testid increments after schema version bump via activation', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(
			request,
			'policy-schema-version-update',
		);
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		// Baseline: schema version at v1.
		await expect(page.getByTestId('policy-schema-version')).toHaveText('v1');

		// activateProposedPolicy writes schema.version + 1, so the schema version increments.
		await activateProposedPolicy(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
			proposedPolicyDraftDenyHigh(),
		);

		// Reload the page — fetchCollections re-queries schemaVersion from the backend.
		await page.goto(policiesUrl);
		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-subject-picker').selectOption(SCN017_SUBJECTS.contractor);
		await page.getByTestId('policy-operation-picker').selectOption('read');
		await page.getByTestId('policy-run-evaluator').click();

		// policy-schema-version must reflect the new schema version (v2).
		await expect(page.getByTestId('policy-schema-version')).toHaveText('v2');
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
		await expect(financePatchLarge.getByTestId('policy-impact-matrix-approval-role')).toContainText(
			SCN017_ROLES.financeApprover,
		);
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

	test('surfaces active-vs-proposed deltas across read|create|update|patch|delete fixture rows', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-impact-matrix-deltas');
		const baseSchema = await fetchCollectionSchema(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
		);
		let activeDraft: Record<string, unknown> | null = null;
		await routeGraphqlWithDraft(page, baseSchema, () => activeDraft);

		const schemasUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/schemas?collection=${encodeURIComponent(SCN017_COLLECTIONS.invoices)}`;
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await page.goto(schemasUrl);
		await page.getByTestId('schema-policy-view-toggle').click();
		await expect(page.getByTestId('schema-policy-view')).toBeVisible();

		const proposed = matrixDeltaDraft();
		await page.getByTestId('schema-policy-editor').fill(JSON.stringify(proposed, null, 2));
		await page.getByTestId('schema-policy-run-compile').click();
		await expect(page.getByTestId('schema-policy-errors')).toHaveCount(0);
		activeDraft = proposed;

		await page.goto(policiesUrl);
		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);

		const matrix = page.getByTestId('policy-impact-matrix');
		await expect(matrix).toBeVisible();

		const invoiceIds = [fixture.invoices.small.id, fixture.invoices.large.id] as const;
		for (const entityId of invoiceIds) {
			for (const operation of IMPACT_MATRIX_DELTA_OPERATIONS) {
				for (const subjectId of IMPACT_MATRIX_VISIBLE_SUBJECTS) {
					const cell = matrix.locator(
						`[data-testid="policy-impact-matrix-cell"][data-entity-id="${entityId}"][data-subject-id="${subjectId}"][data-operation="${operation}"]`,
					);
					await expect(cell).toBeVisible();
					await expect(
						cell.locator(
							'[data-testid="policy-impact-matrix-cell-delta"], [data-testid="policy-impact-matrix-cell-unchanged"]',
						),
						`${entityId} ${subjectId} ${operation} should render a proposed-policy delta state`,
					).toHaveCount(1);
				}
			}
		}

		const contractorReadSmall = matrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.small.id}"][data-subject-id="${SCN017_SUBJECTS.contractor}"][data-operation="read"]`,
		);
		await expect(
			contractorReadSmall.getByTestId('policy-impact-matrix-cell-delta'),
		).toHaveAttribute('data-decision-changed', 'true');

		const financeReadSmall = matrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.small.id}"][data-subject-id="${SCN017_SUBJECTS.financeAgent}"][data-operation="read"]`,
		);
		await expect(financeReadSmall.getByTestId('policy-impact-matrix-cell-delta')).toHaveAttribute(
			'data-redacted-changed',
			'true',
		);

		const financeUpdateSmall = matrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.small.id}"][data-subject-id="${SCN017_SUBJECTS.financeAgent}"][data-operation="update"]`,
		);
		await expect(financeUpdateSmall.getByTestId('policy-impact-matrix-cell-delta')).toHaveAttribute(
			'data-denied-changed',
			'true',
		);

		const financeApproverCreateSmall = matrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.small.id}"][data-subject-id="${SCN017_SUBJECTS.financeApprover}"][data-operation="create"]`,
		);
		await expect(
			financeApproverCreateSmall.getByTestId('policy-impact-matrix-cell-unchanged'),
		).toHaveText('unchanged');
		await expect(
			financeApproverCreateSmall.getByTestId('policy-impact-matrix-cell-proposed'),
		).toHaveCount(0);

		activeDraft = proposedPolicyDraftBroken();
		await page.goto(policiesUrl);
		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		const diagnosticMatrix = page.getByTestId('policy-impact-matrix');
		const diagnosticCell = diagnosticMatrix.locator(
			`[data-testid="policy-impact-matrix-cell"][data-entity-id="${fixture.invoices.small.id}"][data-subject-id="${SCN017_SUBJECTS.contractor}"][data-operation="read"]`,
		);
		await expect(diagnosticCell.getByTestId('policy-impact-matrix-cell-delta')).toHaveAttribute(
			'data-diagnostic-changed',
			'true',
		);
		await expect(
			diagnosticCell.getByTestId('policy-impact-matrix-diagnostic-proposed'),
		).toContainText('policy_filter_unindexed');

		// The "transaction delta unavailable" affordance was removed in axon-5c6e4a79.
		await expect(
			diagnosticMatrix.getByTestId('policy-impact-matrix-cell-transaction-unavailable'),
		).toHaveCount(0);
	});
});

test.describe('Policy authoring (transaction fixture editor)', () => {
	test('renders structured transaction fixture editor and updates the evaluator', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-txn-editor');
		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(policiesUrl);

		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);
		await page.getByTestId('policy-operation-picker').selectOption('transaction');

		// Structured editor should be visible instead of raw textarea.
		const editor = page.getByTestId('transaction-fixture-editor');
		await expect(editor).toBeVisible();

		// Starts with one default operation seeded from the selected entity.
		await expect(editor.getByTestId('transaction-fixture-op')).toHaveCount(1);

		// Add a second operation.
		await editor.getByTestId('transaction-fixture-add-op').click();
		await expect(editor.getByTestId('transaction-fixture-op')).toHaveCount(2);

		// Set the second op's kind to readEntity.
		await editor
			.getByTestId('transaction-fixture-op')
			.nth(1)
			.getByTestId('transaction-fixture-op-kind')
			.selectOption('readEntity');

		// Set collection on second op.
		await editor
			.getByTestId('transaction-fixture-op')
			.nth(1)
			.getByTestId('transaction-fixture-op-collection')
			.fill(SCN017_COLLECTIONS.invoices);

		// Set id on second op.
		await editor
			.getByTestId('transaction-fixture-op')
			.nth(1)
			.getByTestId('transaction-fixture-op-id')
			.fill(fixture.invoices.small.id);

		// Reorder: move second op up to first.
		await editor
			.getByTestId('transaction-fixture-op')
			.nth(1)
			.getByTestId('transaction-fixture-op-move-up')
			.click();
		await expect(
			editor.getByTestId('transaction-fixture-op').nth(0),
		).toHaveAttribute('data-op-kind', 'readEntity');

		// Remove the first op (now readEntity).
		await editor
			.getByTestId('transaction-fixture-op')
			.nth(0)
			.getByTestId('transaction-fixture-op-remove')
			.click();
		await expect(editor.getByTestId('transaction-fixture-op')).toHaveCount(1);

		// Run evaluator with the transaction fixture — should get a result.
		await page.getByTestId('policy-run-evaluator').click();
		await expect(page.getByTestId('policy-transaction-operations')).toBeVisible();
	});
});

test.describe('Policy authoring (transaction-row delta)', () => {
	test('impact matrix renders active-vs-proposed delta for transaction-row cells', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'policy-txn-delta');
		const baseSchema = await fetchCollectionSchema(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
		);
		let activeDraft: Record<string, unknown> | null = null;
		await routeGraphqlWithDraft(page, baseSchema, () => activeDraft);

		const policiesUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/policies`;

		// Load the page and set a proposed draft — now all cells (including any
		// future transaction-collection entities) render delta UX without the
		// blocking affordance.
		activeDraft = matrixDeltaDraft();
		await page.goto(policiesUrl);
		await page.getByTestId('policy-collection-picker').selectOption(SCN017_COLLECTIONS.invoices);

		const matrix = page.getByTestId('policy-impact-matrix');
		await expect(matrix).toBeVisible();

		// The "transaction delta unavailable" affordance must not appear anywhere.
		await expect(
			matrix.getByTestId('policy-impact-matrix-cell-transaction-unavailable'),
		).toHaveCount(0);

		// Wait for at least one cell to resolve.
		await expect(matrix.getByTestId('policy-impact-matrix-cell').first()).toHaveAttribute(
			'data-decision',
			/(allowed|denied|needs_approval)/,
		);

		// Every entity×subject×operation cell must render a proposed-policy delta
		// state (unchanged or changed) — same parity guarantee as entity rows.
		for (const entityId of [fixture.invoices.small.id, fixture.invoices.large.id]) {
			for (const operation of IMPACT_MATRIX_DELTA_OPERATIONS) {
				for (const subjectId of IMPACT_MATRIX_VISIBLE_SUBJECTS) {
					const cell = matrix.locator(
						`[data-testid="policy-impact-matrix-cell"][data-entity-id="${entityId}"][data-subject-id="${subjectId}"][data-operation="${operation}"]`,
					);
					await expect(cell).toBeVisible();
					await expect(
						cell.locator(
							'[data-testid="policy-impact-matrix-cell-delta"], [data-testid="policy-impact-matrix-cell-unchanged"]',
						),
						`${entityId} ${subjectId} ${operation} should render a proposed-policy delta state`,
					).toHaveCount(1);
				}
			}
		}
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
				JSON.stringify({ amount_cents: fixture.invoices.large.amountCents + 500_000 }, null, 2),
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

	test('matrix dry-run gate: activation blocked until fixture dry-run recorded; editing policy invalidates gate', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'schemas-matrix-gate');
		const schemasUrl = `/ui/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/schemas?collection=${encodeURIComponent(SCN017_COLLECTIONS.invoices)}`;

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeAgent);
		await page.goto(schemasUrl);
		await page.getByTestId('schema-policy-view-toggle').click();
		await expect(page.getByTestId('schema-policy-view')).toBeVisible();

		// Propose a policy and compile it — no errors.
		const proposed = proposedPolicyDraftDenyHigh();
		await page.getByTestId('schema-policy-editor').fill(JSON.stringify(proposed, null, 2));
		await page.getByTestId('schema-policy-run-compile').click();
		await expect(page.getByTestId('schema-policy-errors')).toHaveCount(0);

		// Activate must be disabled: matrix dry-run has not been recorded for this hash.
		await expect(page.getByTestId('schema-policy-activate')).toBeDisabled();

		// Deep-link to the impact matrix is visible so operators can run it inline.
		await expect(page.getByTestId('schema-policy-matrix-link')).toBeVisible();

		// Run the fixture dry-run — this records the matrix dry-run hash for the
		// current proposed access_control.
		await page.getByTestId('schema-policy-fixture-subject').fill(SCN017_SUBJECTS.financeAgent);
		await page.getByTestId('schema-policy-fixture-operation').selectOption('patch');
		await page.getByTestId('schema-policy-fixture-entity').fill(fixture.invoices.large.id);
		await page
			.getByTestId('schema-policy-fixture-patch')
			.fill(
				JSON.stringify({ amount_cents: fixture.invoices.large.amountCents + 500_000 }, null, 2),
			);
		await page.getByTestId('schema-policy-fixture-run').click();
		await expect(page.getByTestId('schema-policy-fixture-decision')).toBeVisible();

		// Activation must now be enabled — dry-run hash recorded for this policy.
		await expect(page.getByTestId('schema-policy-activate')).toBeEnabled();

		// Editing the policy content changes the canonical hash — gate must be invalidated.
		await page
			.getByTestId('schema-policy-editor')
			.fill(JSON.stringify(matrixDeltaDraft(), null, 2));

		// Activate must be disabled again: the recorded hash no longer matches.
		await expect(page.getByTestId('schema-policy-activate')).toBeDisabled();
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
