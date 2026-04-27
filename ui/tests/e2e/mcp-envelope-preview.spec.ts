import { expect, test } from '@playwright/test';
import {
	SCN017_COLLECTIONS,
	SCN017_ROLES,
	SCN017_SUBJECTS,
	captureDataPlaneRequests,
	dbIntentUrl,
	expectGraphqlPrimaryDataPlane,
	routeGraphqlAs,
	seedApprovalCollections,
	seedIntentStates,
	seedScn017PolicyUiFixture,
} from './helpers';

function escapeRegExp(value: string): string {
	return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

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

	test('opens an axon.query bridge in the GraphQL console matching the envelope outcome', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'mcp-envelope-bridge');
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

		await expect(page.getByTestId('mcp-envelope-outcome')).toHaveText('needs_approval');
		const envelopeReason = await page.getByTestId('mcp-envelope-reason').textContent();
		const envelopePolicyVersion = await page
			.getByTestId('mcp-envelope-policy-version')
			.textContent();

		// Comparison view explicitly states the envelope and explainPolicy
		// agree on reason and policy version. This is the bridge guarantee
		// FEAT-031 / US-119 requires.
		await expect(page.getByTestId('mcp-envelope-comparison-outcome')).toHaveAttribute(
			'data-match',
			'match',
		);
		await expect(page.getByTestId('mcp-envelope-comparison-policy')).toHaveAttribute(
			'data-match',
			'match',
		);

		await page.getByTestId('mcp-envelope-bridge-graphql').click();

		await expect(page).toHaveURL(/\/graphql\?/);
		await expect(page.getByTestId('graphql-preset')).toHaveText('axon.query');
		await expect(page.getByTestId('graphql-actor')).toHaveValue(SCN017_SUBJECTS.financeAgent);
		await expect(page.getByTestId('graphql-query')).toHaveValue(/explainPolicy/);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(/"operation": "patch"/);
		await expect(page.getByTestId('graphql-variables')).toHaveValue(
			new RegExp(`"entityId": "${escapeRegExp(fixture.invoices.large.id)}"`),
		);

		await page.getByRole('button', { name: /Run/ }).click();
		const response = page.getByTestId('graphql-response');
		await expect(response).toContainText('"explainPolicy"');
		await expect(response).toContainText('"decision": "needs_approval"');
		await expect(response).toContainText(`"reason": "${envelopeReason ?? 'needs_approval'}"`);
		// Envelope policy version label is `vN`; the bridge response prints
		// the raw integer. Strip the prefix and prove the values match.
		const policyVersionDigits = (envelopePolicyVersion ?? 'v1').replace(/^v/, '');
		await expect(response).toContainText(`"policyVersion": ${policyVersionDigits}`);
		await expect(response).toContainText(`"role": "${SCN017_ROLES.financeApprover}"`);
	});
});

test.describe('MCP stdio provenance', () => {
	test('shows stdio command/config status and redacted env for an MCP-originated intent', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'mcp-stdio-provenance');
		const ids = await seedIntentStates(request, db);

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeApprover);
		await page.goto(dbIntentUrl(db, ids.approveTarget));

		const provenance = page.getByTestId('mcp-stdio-provenance');
		await expect(provenance).toBeVisible();
		await expect(page.getByTestId('mcp-stdio-surface')).toHaveText('mcp');
		await expect(page.getByTestId('mcp-stdio-transport')).toHaveText('stdio');
		await expect(page.getByTestId('mcp-stdio-agent')).toHaveText('tool.review-console');
		// `toolNameLabel` derives the displayed tool from `agent_id` by
		// stripping any namespace prefix, so `tool.review-console` renders
		// as `review-console` in the stdio provenance grid.
		await expect(page.getByTestId('mcp-stdio-tool')).toHaveText('review-console');
		await expect(page.getByTestId('mcp-stdio-credential')).toHaveText('cred-finance-bot');
		await expect(page.getByTestId('mcp-stdio-grant-version')).toHaveText('13');
		await expect(page.getByTestId('mcp-stdio-delegated')).toHaveText(SCN017_SUBJECTS.financeAgent);

		// Launch command embeds the routing scope so an operator can mirror
		// the agent's stdio configuration locally.
		await expect(page.getByTestId('mcp-stdio-command')).toContainText('axon-server --mcp-stdio');
		await expect(page.getByTestId('mcp-stdio-command')).toContainText(
			`--tenant ${db.tenant.db_name}`,
		);
		await expect(page.getByTestId('mcp-stdio-command')).toContainText(`--database ${db.name}`);
		await expect(page.getByTestId('mcp-stdio-status')).toHaveText(/active|idle|recent/i);

		// Env preview surfaces non-secret routing values verbatim.
		const tenantRow = page
			.getByTestId('mcp-stdio-env-row')
			.filter({ has: page.locator('code', { hasText: 'AXON_TENANT' }) });
		await expect(tenantRow).toHaveAttribute('data-redacted', 'false');
		await expect(tenantRow).toContainText(db.tenant.db_name);

		// Secret-keyed env entries are redacted before they reach the DOM.
		// AXON_CREDENTIAL_ID is a real routing-scope entry whose key matches
		// the secret-key pattern, so it must not surface the literal id even
		// though that id is shown elsewhere as an explicit identifier.
		const credentialEnvRow = page
			.getByTestId('mcp-stdio-env-row')
			.filter({ has: page.locator('code', { hasText: 'AXON_CREDENTIAL_ID' }) });
		await expect(credentialEnvRow).toHaveAttribute('data-redacted', 'true');
		await expect(credentialEnvRow).toContainText('[redacted]');

		// The credential id is shown in the dedicated identifier row but
		// must never appear inside the env preview table.
		const envTableText = (await page.getByTestId('mcp-stdio-env').textContent()) ?? '';
		expect(envTableText).not.toContain('cred-finance-bot');
	});

	test('hides the stdio provenance panel for human-originated intents', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'mcp-stdio-human-only', {
			seedIntentFixtures: false,
		});
		// Preview a human-originated (no agent_id) intent through GraphQL so
		// the intent detail page renders without an MCP provenance section.
		const previewBody = await request.post(
			`/tenants/${encodeURIComponent(fixture.tenant.db_name)}/databases/${encodeURIComponent(fixture.db.name)}/graphql`,
			{
				headers: { 'x-axon-actor': SCN017_SUBJECTS.financeAgent },
				data: {
					query: `mutation {
						previewMutation(input: {
							operation: {
								operationKind: "patch_entity"
								operation: {
									collection: "task"
									id: "task-a"
									expected_version: 1
									patch: { budget_cents: 6000 }
								}
							}
							subject: {
								userId: "${SCN017_SUBJECTS.financeAgent}"
								tenantRole: "${SCN017_ROLES.financeAgent}"
								credentialId: "cred-finance-agent"
								grantVersion: 7
							}
							expiresInSeconds: 600
						}) { intent { id } }
					}`,
				},
			},
		);
		const previewJson = (await previewBody.json()) as {
			data?: { previewMutation?: { intent?: { id?: string } } };
		};
		const humanIntentId = previewJson.data?.previewMutation?.intent?.id ?? '';
		expect(humanIntentId).not.toBe('');

		await routeGraphqlAs(page, SCN017_SUBJECTS.financeApprover);
		await page.goto(dbIntentUrl(fixture.db, humanIntentId));
		await expect(page.getByTestId('intent-detail')).toBeVisible();
		await expect(page.getByTestId('mcp-stdio-provenance')).toHaveCount(0);
	});
});
