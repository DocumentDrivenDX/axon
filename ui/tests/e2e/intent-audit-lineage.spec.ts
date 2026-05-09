import { expect, test } from '@playwright/test';

import {
	SCN017_COLLECTIONS,
	SCN017_SUBJECTS,
	TASK_COLLECTION,
	activateProposedPolicy,
	approveIntent,
	captureDataPlaneRequests,
	commitIntent,
	createBudgetRecord,
	dbAuditUrl,
	dbIntentUrl,
	dbIntentsUrl,
	expectGraphqlPrimaryDataPlane,
	graphqlPath,
	patchBudgetRecordAs,
	previewBudgetIntent,
	proposedPolicyDraftDenyHigh,
	routeGraphqlAs,
	seedApprovalCollections,
	seedIntentStates,
	seedScn017PolicyUiFixture,
} from './helpers';

test.describe('Intent audit lineage', () => {
	test('shows delegated MCP intent metadata in the inbox and detail panels @US-119', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'intent-audit-lineage');
		const ids = await seedIntentStates(request, db);
		const requests = captureDataPlaneRequests(page, db);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentsUrl(db));

		const row = page.getByTestId(`intent-row-${ids.approveTarget}`);
		await expect(row).toBeVisible();
		await expect(page.getByTestId(`intent-origin-${ids.approveTarget}`)).toContainText('MCP');
		await expect(page.getByTestId(`intent-origin-${ids.approveTarget}`)).toContainText(
			'tool.review-console',
		);
		await expect(page.getByTestId(`intent-origin-${ids.approveTarget}`)).toContainText(
			'finance-agent',
		);
		await expect(page.getByTestId(`intent-outcome-${ids.approveTarget}`)).toContainText(
			'needs_approval',
		);
		await expect(page.getByTestId(`intent-outcome-${ids.approveTarget}`)).toContainText('pending');

		await row.click();
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('tool.review-console');
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('finance-agent');
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('cred-finance-bot');
		await expect(page.getByTestId('intent-inline-mcp')).toContainText('13');
		await expect(page.getByTestId('intent-inline-tool-arguments')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-inline-tool-arguments')).not.toContainText('23000');
		await expect(page.getByTestId('intent-inline-structured-outcome')).toContainText(
			'needs_approval',
		);
		await expect(page.getByTestId('intent-inline-structured-outcome')).toContainText('pending');

		await page.getByTestId('intent-open-detail').click();
		await expect(page).toHaveURL(dbIntentUrl(db, ids.approveTarget));
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('tool.review-console');
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('finance-agent');
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('cred-finance-bot');
		await expect(page.getByTestId('intent-origin-metadata')).toContainText('13');
		await expect(page.getByTestId('intent-tool-arguments')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-tool-arguments')).not.toContainText('23000');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('needs_approval');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('pending');

		expectGraphqlPrimaryDataPlane(requests, 'intent audit lineage route should stay GraphQL-primary');
	});

	test('shows conflict outcomes for stale MCP-originated intent commits @US-118', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'intent-audit-conflict');
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-conflict');
		const staleIntent = await previewBudgetIntent(request, db, 'task-conflict', 27_000, {
			agentId: 'mcp.gateway',
			grantVersion: 21,
		});
		await approveIntent(request, db, staleIntent.intentId);
		await patchBudgetRecordAs(request, db, 'finance-agent', TASK_COLLECTION, 'task-conflict', 5100);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(dbIntentUrl(db, staleIntent.intentId));

		await expect(page.getByTestId('intent-origin-metadata')).toContainText('mcp.gateway');
		await expect(page.getByTestId('intent-tool-arguments')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-tool-arguments')).not.toContainText('27000');

		await page.getByTestId('intent-commit-token').fill(staleIntent.token);
		await page.getByTestId('intent-commit-action').click();
		await expect(page.getByTestId('intent-commit-error')).toContainText('intent_stale');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('conflict');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText('intent_stale');
		await expect(page.getByTestId('intent-structured-outcome')).toContainText(
			'commit_validation_failed',
		);
	});

	test('records administrative audit evidence for policy activation', async ({ request }) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'schema-policy-audit');
		const proposed = proposedPolicyDraftDenyHigh();
		const result = await activateProposedPolicy(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
			proposed,
			{ actor: 'admin' },
		);
		expect(result.schema.version).toBe(2);

		// Probe the audit log for the schema_update entry on the invoices
		// collection. Activation must record old + new schema/policy versions.
		const response = await request.post(graphqlPath(fixture.db), {
			data: {
				query: `query($collection: String!) {
					auditLog(collection: $collection, operation: "schema.update") {
						totalCount
						edges { node { metadata } }
					}
				}`,
				variables: { collection: SCN017_COLLECTIONS.invoices },
			},
		});
		const body = (await response.json()) as {
			data?: {
				auditLog?: {
					totalCount: number;
					edges: Array<{ node: { metadata: Record<string, string> } }>;
				};
			};
			errors?: unknown;
		};
		expect(response.ok(), `${response.status()} ${JSON.stringify(body)}`).toBe(true);
		expect(body.errors ?? null).toBeNull();
		const auditLog = body.data?.auditLog;
		expect(auditLog?.totalCount ?? 0).toBeGreaterThanOrEqual(1);
		// The most recent schema.update entry corresponds to our activation.
		const metadata = auditLog?.edges.at(-1)?.node.metadata ?? {};
		expect(metadata.old_schema_version).toBe('1');
		expect(metadata.new_schema_version).toBe('2');
		expect(metadata.old_policy_version).toBe('1');
		expect(metadata.new_policy_version).toBe('2');
	});
});

test.describe('Intent audit deep link and lineage panel', () => {
	test('deep link filters /audit by intent ID and pre-populates filter @US-116', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'audit-deeplink');
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-dl');
		const intent = await previewBudgetIntent(request, db, 'task-dl', 6_000, {
			agentId: 'tool.commit-dl',
		});
		await commitIntent(request, db, intent);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(`${dbAuditUrl(db)}?intent=${encodeURIComponent(intent.intentId)}`);

		// Filter field pre-populated from URL param
		await expect(page.getByTestId('audit-intent-filter')).toHaveValue(intent.intentId);

		// Banner shows the filtered intent ID
		await expect(page.getByTestId('audit-intent-banner')).toBeVisible();
		await expect(page.getByTestId('audit-intent-banner')).toContainText(intent.intentId);

		// Entries are present; preview comes before commit (chronological order)
		await expect(page.locator('[data-testid="audit-entry-row"]').first()).toBeVisible();
		const operations = await page
			.locator('[data-testid="audit-entry-row"] td:nth-child(5)')
			.allTextContents();
		const previewIdx = operations.findIndex((op) => op.includes('mutation_intent.preview'));
		const commitIdx = operations.findIndex((op) => op.includes('intent.commit'));
		expect(previewIdx).toBeGreaterThanOrEqual(0);
		expect(commitIdx).toBeGreaterThan(previewIdx);
	});

	test('shows intent lineage panel when an intent audit entry is selected', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'audit-lineage-panel');
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-lp');
		const intent = await previewBudgetIntent(request, db, 'task-lp', 6_000, {
			agentId: 'tool.panel-test',
		});
		await commitIntent(request, db, intent);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(`${dbAuditUrl(db)}?intent=${encodeURIComponent(intent.intentId)}`);

		// Click the commit row to select it
		const commitRow = page.locator('[data-testid="audit-entry-row"]', {
			has: page.locator('td', { hasText: 'intent.commit' }),
		});
		await expect(commitRow).toBeVisible();
		await commitRow.click();

		// Lineage panel is visible in the detail section
		await expect(page.getByTestId('audit-intent-lineage')).toBeVisible();

		// Intent link navigates to the correct intent detail page
		const intentLink = page.getByTestId('audit-intent-link');
		await expect(intentLink).toBeVisible();
		await expect(intentLink).toContainText(intent.intentId);
		const href = await intentLink.getAttribute('href');
		expect(href).toContain(encodeURIComponent(intent.intentId));

		// AC2: version indicators are visible in the lineage panel
		await expect(page.getByTestId('audit-lineage-policy-version')).toBeVisible();
		await expect(page.getByTestId('audit-lineage-schema-version')).toBeVisible();
	});

	// AC3: policy_version increments after activateProposedPolicy
	test('audit lineage policy-version and schema-version increment after policy activation', async ({
		page,
		request,
	}) => {
		const fixture = await seedScn017PolicyUiFixture(request, 'audit-lineage-version-increment');

		// Create a budget record and commit an intent at the baseline policy/schema version.
		await createBudgetRecord(request, fixture.db, TASK_COLLECTION, 'task-ver-incr');
		const intent = await previewBudgetIntent(request, fixture.db, 'task-ver-incr', 6_000, {
			agentId: 'tool.version-incr-test',
		});
		await commitIntent(request, fixture.db, intent);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(`${dbAuditUrl(fixture.db)}?intent=${encodeURIComponent(intent.intentId)}`);

		const commitRow = page.locator('[data-testid="audit-entry-row"]', {
			has: page.locator('td', { hasText: 'intent.commit' }),
		});
		await expect(commitRow).toBeVisible();
		await commitRow.click();

		await expect(page.getByTestId('audit-intent-lineage')).toBeVisible();

		// Capture baseline versions; they should be numeric strings at v1.
		const baselinePolicyVersion = await page
			.getByTestId('audit-lineage-policy-version')
			.textContent();
		const baselineSchemaVersion = await page
			.getByTestId('audit-lineage-schema-version')
			.textContent();
		expect(Number(baselinePolicyVersion)).toBeGreaterThanOrEqual(1);
		expect(Number(baselineSchemaVersion)).toBeGreaterThanOrEqual(1);

		// Activate a new policy (bumps both policy_version and schema_version).
		const result = await activateProposedPolicy(
			request,
			fixture.db,
			SCN017_COLLECTIONS.invoices,
			proposedPolicyDraftDenyHigh(),
		);
		expect(result.schema.version).toBeGreaterThan(Number(baselineSchemaVersion));

		// Commit a second intent under the new policy version so there is an audit
		// entry that recorded the incremented versions.
		const intent2 = await previewBudgetIntent(request, fixture.db, 'task-ver-incr', 7_000, {
			agentId: 'tool.version-incr-test-2',
		});
		await commitIntent(request, fixture.db, intent2);

		await page.goto(`${dbAuditUrl(fixture.db)}?intent=${encodeURIComponent(intent2.intentId)}`);

		const commitRow2 = page.locator('[data-testid="audit-entry-row"]', {
			has: page.locator('td', { hasText: 'intent.commit' }),
		});
		await expect(commitRow2).toBeVisible();
		await commitRow2.click();

		await expect(page.getByTestId('audit-intent-lineage')).toBeVisible();

		// policy_version testid must reflect the activated (incremented) version.
		const updatedPolicyVersion = await page
			.getByTestId('audit-lineage-policy-version')
			.textContent();
		expect(Number(updatedPolicyVersion)).toBeGreaterThan(Number(baselinePolicyVersion));

		// schema_version testid must also reflect the incremented version.
		await expect(page.getByTestId('audit-lineage-schema-version')).toBeVisible();
		// grant_version is not surfaced in this view — N/A for this test.
	});

	test('intent detail "open audit log" links to filtered audit view and back', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'audit-backlink');
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-back');
		const intent = await previewBudgetIntent(request, db, 'task-back', 6_000);
		await commitIntent(request, db, intent);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(dbIntentUrl(db, intent.intentId));

		// The "Open audit log" link should carry the intent ID as a filter param
		const auditLink = page.getByRole('link', { name: 'Open audit log' });
		await expect(auditLink).toBeVisible();
		const href = await auditLink.getAttribute('href');
		expect(href).toMatch(/\/audit/);
		expect(href).toContain(encodeURIComponent(intent.intentId));

		// Click through; the audit page should show the filtered view
		await auditLink.click();
		await expect(page).toHaveURL(/\/audit/);
		await expect(page.getByTestId('audit-intent-filter')).toHaveValue(intent.intentId);
		await expect(page.getByTestId('audit-intent-banner')).toBeVisible();

		// Clearing the intent filter dismisses the banner
		await page
			.getByTestId('audit-intent-banner')
			.getByRole('button', { name: 'Clear filter' })
			.click();
		await expect(page.getByTestId('audit-intent-banner')).not.toBeVisible();
	});

	test('shows preview and approval events for approved intent in chronological order', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'audit-order-approve');
		await createBudgetRecord(request, db, TASK_COLLECTION, 'task-order');
		const intent = await previewBudgetIntent(request, db, 'task-order', 20_001, {
			agentId: 'tool.order-test',
		});
		await approveIntent(request, db, intent.intentId);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(`${dbAuditUrl(db)}?intent=${encodeURIComponent(intent.intentId)}`);

		await expect(page.locator('[data-testid="audit-entry-row"]').first()).toBeVisible();
		const operations = await page
			.locator('[data-testid="audit-entry-row"] td:nth-child(5)')
			.allTextContents();
		const previewIdx = operations.findIndex((op) => op.includes('mutation_intent.preview'));
		const approveIdx = operations.findIndex((op) => op.includes('intent.approve'));
		expect(previewIdx).toBeGreaterThanOrEqual(0);
		expect(approveIdx).toBeGreaterThan(previewIdx);
	});

	test('shows rejection event for rejected intent in chronological order', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'audit-reject-lineage');
		const ids = await seedIntentStates(request, db);

		await routeGraphqlAs(page, 'finance-agent');
		await page.goto(`${dbAuditUrl(db)}?intent=${encodeURIComponent(ids.rejected)}`);

		await expect(page.locator('[data-testid="audit-entry-row"]').first()).toBeVisible();
		const operations = await page
			.locator('[data-testid="audit-entry-row"] td:nth-child(5)')
			.allTextContents();
		const previewIdx = operations.findIndex((op) => op.includes('mutation_intent.preview'));
		const rejectIdx = operations.findIndex((op) => op.includes('intent.reject'));
		expect(previewIdx).toBeGreaterThanOrEqual(0);
		expect(rejectIdx).toBeGreaterThan(previewIdx);

		// Select the reject entry and verify lineage panel shows the intent link
		const rejectRow = page.locator('[data-testid="audit-entry-row"]', {
			has: page.locator('td', { hasText: 'intent.reject' }),
		});
		await rejectRow.click();
		await expect(page.getByTestId('audit-intent-lineage')).toBeVisible();
		await expect(page.getByTestId('audit-intent-link')).toBeVisible();
		await expect(page.getByTestId('audit-lineage-decision')).toContainText('needs_approval');
	});

	test('contractor lineage redacts metadata fields', async ({ page, request }) => {
		const db = await seedApprovalCollections(request, 'audit-lineage-contractor-redact');

		const mockIntentId = 'contractor-lineage-redact-intent';
		const sensitiveReason = 'contract-review-alpha-confidential';
		const sensitiveApproverActor = 'restricted-finance-approver-actor';

		// Mock GraphQL: effectivePolicy returns redactedFields that overlap lineage field names,
		// simulating a policy that strips these values for contractor role.
		await routeGraphqlAs(page, SCN017_SUBJECTS.contractor, async (postData) => {
			if (postData.includes('AxonUiEffectivePolicy')) {
				return {
					data: {
						effectivePolicy: {
							collection: TASK_COLLECTION,
							canRead: true,
							canCreate: false,
							canUpdate: false,
							canDelete: false,
							redactedFields: ['reason', 'approver.actor', 'approver.user_id'],
							deniedFields: [],
							policyVersion: 1,
						},
					},
				};
			}
			return null;
		});

		// Mock REST audit endpoint: server returns null for fields it stripped from the lineage.
		await page.route('**/audit/query*', async (route) => {
			const url = new URL(route.request().url());
			if (url.searchParams.get('intent_id') === mockIntentId) {
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({
						entries: [
							{
								id: 1001,
								timestamp_ns: Date.now() * 1_000_000,
								collection: TASK_COLLECTION,
								entity_id: 'task-contractor-redact',
								version: 2,
								mutation: 'intent.commit',
								data_before: null,
								data_after: { budget_cents: 15000, status: 'approved' },
								actor: SCN017_SUBJECTS.financeAgent,
								transaction_id: null,
								metadata: null,
								intent_lineage: {
									intent_id: mockIntentId,
									decision: 'allow',
									approval_id: null,
									policy_version: 1,
									schema_version: 1,
									subject_snapshot: null,
									approver: {
										actor: null,
										user_id: null,
										tenant_role: null,
										credential_id: null,
									},
									reason: null,
									origin: {
										surface: 'mcp',
										tool_name: 'tool.audit-test',
									},
									lineage_links: null,
								},
							},
						],
						next_cursor: null,
					}),
				});
				return;
			}
			await route.continue();
		});

		await page.goto(`${dbAuditUrl(db)}?intent=${encodeURIComponent(mockIntentId)}`);
		await page.waitForLoadState('networkidle');

		const entryRow = page.locator('[data-testid="audit-entry-row"]').first();
		await expect(entryRow).toBeVisible();
		await entryRow.click();

		await expect(page.getByTestId('audit-intent-lineage')).toBeVisible();

		// Null fields covered by redactedFields render as [redacted], not blank.
		await expect(page.getByTestId('audit-lineage-reason')).toBeVisible();
		await expect(page.getByTestId('audit-lineage-reason')).toContainText('[redacted]');
		await expect(page.getByTestId('audit-lineage-approver')).toBeVisible();
		await expect(page.getByTestId('audit-lineage-approver')).toContainText('[redacted]');

		// Fields not in redactedFields render as plain text.
		await expect(page.getByTestId('audit-lineage-decision')).toContainText('allow');
		await expect(page.getByTestId('audit-lineage-origin')).toContainText('mcp');

		// DOM-leakage assertions (axon-c3895a14 sibling pattern).
		const html = await page.content();
		expect(html).not.toContain(sensitiveReason);
		expect(html).not.toContain(sensitiveApproverActor);

		const leakProbe = await page.evaluate((needle: string) => {
			const dump = (s: Storage) => Object.values(s).join(' ');
			return {
				inLocalStorage: dump(localStorage).includes(needle),
				inSessionStorage: dump(sessionStorage).includes(needle),
				inDocumentTitle: document.title.includes(needle),
			};
		}, sensitiveReason);
		expect(leakProbe.inLocalStorage).toBe(false);
		expect(leakProbe.inSessionStorage).toBe(false);
		expect(leakProbe.inDocumentTitle).toBe(false);
	});
});
