/**
 * Shared helpers for Axon e2e tests running against the live HTTPS server.
 *
 * All helpers accept a Playwright `request` fixture so HTTPS errors and
 * auth headers are inherited from the project config. Tests should use
 * these to provision tenants/databases/collections/entities instead of
 * going through the UI — keeps the UI tests focused on UI behavior.
 */
import type { APIRequestContext, APIResponse, Page } from '@playwright/test';
import { expect } from '@playwright/test';

export type TestTenant = {
	id: string;
	name: string;
	db_name: string;
};

export type TestDatabase = {
	tenant: TestTenant;
	name: string;
};

export const E2E_FIXTURE_PREFIX = 'e2e-';

function withE2eFixturePrefix(value: string): string {
	return value.startsWith(E2E_FIXTURE_PREFIX) ? value : `${E2E_FIXTURE_PREFIX}${value}`;
}

async function expectOkResponse(response: APIResponse, label: string) {
	if (response.ok()) return;
	let body = '';
	try {
		body = await response.text();
	} catch {
		body = '<unreadable response body>';
	}
	expect(response.ok(), `${label}: ${response.status()} ${body}`).toBe(true);
}

/** Create a tenant with a unique name. */
export async function createTestTenant(
	request: APIRequestContext,
	prefix: string,
): Promise<TestTenant> {
	const suffix = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
	const name = `${withE2eFixturePrefix(prefix)}-${suffix}`;
	const response = await request.post('/control/tenants', {
		data: { name },
	});
	await expectOkResponse(response, `create tenant ${name}`);
	return (await response.json()) as TestTenant;
}

/** Create a database under a tenant. */
export async function createTestDatabase(
	request: APIRequestContext,
	tenant: TestTenant,
	dbName = 'default',
): Promise<TestDatabase> {
	const response = await request.post(
		`/control/tenants/${encodeURIComponent(tenant.id)}/databases`,
		{ data: { name: dbName } },
	);
	await expectOkResponse(response, `create database ${dbName}`);
	return { tenant, name: dbName };
}

/** Create a collection on a database. Pass an optional schema. */
export async function createTestCollection(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	schema: {
		entity_schema?: Record<string, unknown> | null;
		access_control?: Record<string, unknown>;
		indexes?: Record<string, unknown>[];
		lifecycles?: Record<string, unknown>;
		link_types?: Record<string, unknown>;
	} = {},
): Promise<void> {
	const url = `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/collections/${encodeURIComponent(collection)}`;
	const response = await request.post(url, {
		data: {
			schema: {
				description: null,
				version: 1,
				entity_schema: schema.entity_schema ?? null,
				access_control: schema.access_control,
				indexes: schema.indexes ?? [],
				link_types: schema.link_types ?? {},
				lifecycles: schema.lifecycles ?? undefined,
			},
			actor: 'e2e',
		},
	});
	await expectOkResponse(response, `create collection ${collection}`);
}

/** Create an entity in a collection. */
export async function createTestEntity(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	id: string,
	data: Record<string, unknown>,
): Promise<void> {
	const url = `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`;
	const response = await request.post(url, {
		data: { data, actor: 'e2e' },
	});
	await expectOkResponse(response, `create entity ${id}`);
}

/** Update an entity in a collection (PATCH/PUT). */
export async function updateTestEntity(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	id: string,
	data: Record<string, unknown>,
	expectedVersion: number,
): Promise<void> {
	const url = `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/entities/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`;
	const response = await request.put(url, {
		data: { data, expected_version: expectedVersion, actor: 'e2e' },
	});
	await expectOkResponse(response, `update entity ${id}`);
}

/** Create a link between two entities via the REST endpoint. */
export async function createTestLink(
	request: APIRequestContext,
	db: TestDatabase,
	link: {
		source_collection: string;
		source_id: string;
		target_collection: string;
		target_id: string;
		link_type: string;
	},
): Promise<void> {
	const url = `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/links`;
	const response = await request.post(url, {
		data: link,
	});
	await expectOkResponse(
		response,
		`create link ${link.source_collection}/${link.source_id} -[${link.link_type}]-> ${link.target_collection}/${link.target_id}`,
	);
}

/** Build a UI URL for the database collections page. */
export function dbCollectionsUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/collections`;
}

export function dbCollectionUrl(db: TestDatabase, collection: string): string {
	return `${dbCollectionsUrl(db)}/${encodeURIComponent(collection)}`;
}

export function dbOverviewUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}`;
}

export function dbGraphqlUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/graphql`;
}

export function dbAuditUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/audit`;
}

export function tenantUrl(
	tenant: TestTenant,
	section: 'members' | 'credentials' | '' = '',
): string {
	const base = `/ui/tenants/${encodeURIComponent(tenant.db_name)}`;
	return section ? `${base}/${section}` : base;
}

/** Click the first row in the entity list to select it. */
export async function selectFirstEntity(page: Page): Promise<void> {
	const firstRow = page.locator('table tr').nth(1);
	await firstRow.click();
}

export type TestUser = {
	id: string;
	display_name: string;
	email: string | null;
	created_at_ms: number;
	suspended_at_ms: number | null;
};

/** Provision a user row via POST /control/users/provision. */
export async function createTestUser(
	request: APIRequestContext,
	displayName?: string,
	email?: string | null,
): Promise<TestUser> {
	const name = withE2eFixturePrefix(displayName ?? `test-${Date.now().toString(36)}`);
	const response = await request.post('/control/users/provision', {
		data: { display_name: name, email: email ?? null },
	});
	await expectOkResponse(response, `create user ${name}`);
	return (await response.json()) as TestUser;
}

export async function addTestTenantMember(
	request: APIRequestContext,
	tenant: TestTenant,
	user: TestUser,
	role: 'admin' | 'write' | 'read' = 'read',
): Promise<void> {
	const response = await request.put(
		`/control/tenants/${encodeURIComponent(tenant.id)}/members/${encodeURIComponent(user.id)}`,
		{ data: { role } },
	);
	await expectOkResponse(response, `add member ${user.id} to ${tenant.id}`);
}

export type GraphqlMock = (
	postData: string,
) => Promise<Record<string, unknown> | null> | Record<string, unknown> | null;

export function graphqlPath(db: TestDatabase): string {
	return `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/graphql`;
}

export async function gqlAs(
	request: APIRequestContext,
	db: TestDatabase,
	actor: string,
	query: string,
): Promise<Record<string, unknown>> {
	const response = await request.post(graphqlPath(db), {
		headers: { 'x-axon-actor': actor },
		data: { query },
	});
	const body = (await response.json()) as {
		data?: Record<string, unknown>;
		errors?: unknown;
	};
	expect(response.ok(), `${response.status()} ${JSON.stringify(body)}`).toBe(true);
	expect(body.errors ?? null, JSON.stringify(body.errors)).toBeNull();
	return body.data ?? {};
}

export async function routeGraphqlAs(page: Page, actor: string, mock?: GraphqlMock) {
	await page.route('**/graphql', async (route) => {
		const postData = route.request().postData() ?? '';
		const mocked = mock ? await mock(postData) : null;
		if (mocked) {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(mocked),
			});
			return;
		}

		const headers = route.request().headers();
		await route.continue({
			headers: {
				...headers,
				'x-axon-actor': headers['x-axon-actor'] ?? actor,
			},
		});
	});
}

export function captureDataPlaneRequests(page: Page, db: TestDatabase): string[] {
	const requests: string[] = [];
	page.on('request', (request) => {
		const path = new URL(request.url()).pathname;
		if (
			path.startsWith(
				`/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/`,
			)
		) {
			requests.push(path);
		}
	});
	return requests;
}

export function expectGraphqlPrimaryDataPlane(requests: string[], message: string): void {
	expect(requests.some((path) => path.endsWith('/graphql'))).toBe(true);
	expect(
		requests.filter((path) => !path.endsWith('/graphql')),
		message,
	).toEqual([]);
}

export const SCN017_COLLECTIONS = {
	users: 'users',
	vendors: 'vendors',
	invoices: 'invoices',
	task: 'task',
	expense: 'expense',
	policyFilterUnindexed: 'policy_filter_unindexed',
} as const;

export const TASK_COLLECTION = SCN017_COLLECTIONS.task;
export const EXPENSE_COLLECTION = SCN017_COLLECTIONS.expense;

export const SCN017_SUBJECTS = {
	financeAgent: 'finance-agent',
	financeApprover: 'finance-approver',
	requester: 'requester',
	operator: 'operator',
	contractor: 'contractor',
} as const;

export const SCN017_ROLES = {
	financeAgent: 'finance_agent',
	financeApprover: 'finance_approver',
	requester: 'requester',
	operator: 'operator',
	contractor: 'contractor',
} as const;

export const SCN017_STORY_TAGS = {
	policyAuthoring: 'policy-authoring',
	policyEnforcement: 'policy-enforcement',
	graphqlPolicyConsole: 'graphql-policy-console',
	mutationIntents: 'mutation-intents',
	approvalInbox: 'approval-inbox',
	intentAuditLineage: 'intent-audit-lineage',
	mcpEnvelopePreview: 'mcp-envelope-preview',
} as const;

export const SCN017_WORKFLOW_TAGS = {
	smallInvoice: 'small-invoice-autonomous',
	largeInvoice: 'large-invoice-needs-approval',
	staleIntent: 'stale-intent',
	expiredIntent: 'expired-intent',
	policyFilterUnindexed: 'policy-filter-unindexed',
} as const;

export type PreviewedIntent = {
	intentId: string;
	token: string;
};

type Scn017IntentFixture = PreviewedIntent & {
	actor: string;
	collection: string;
	entityId: string;
	budgetCents: number;
	state: 'pending' | 'approved' | 'rejected' | 'expired' | 'committed';
	workflowTag: string;
};

type Scn017IntentFixtures = {
	pending: Scn017IntentFixture;
	approved: Scn017IntentFixture;
	rejected: Scn017IntentFixture;
	expired: Scn017IntentFixture;
	committed: Scn017IntentFixture;
	approveTarget: Scn017IntentFixture;
	rejectTarget: Scn017IntentFixture;
	stale: Scn017IntentFixture;
};

export type SeededIntentIds = {
	pending: string;
	approved: string;
	rejected: string;
	expired: string;
	committed: string;
	approveTarget: string;
	rejectTarget: string;
};

export type Scn017PolicyUiFixture = {
	tenant: TestTenant;
	db: TestDatabase;
	collections: typeof SCN017_COLLECTIONS;
	subjects: typeof SCN017_SUBJECTS;
	roles: typeof SCN017_ROLES;
	users: Record<
		keyof typeof SCN017_SUBJECTS,
		{
			id: string;
			displayName: string;
			approvalRole: string;
			procurementRole: string;
		}
	>;
	vendors: {
		primary: { id: string; name: string };
		secondary: { id: string; name: string };
	};
	invoices: {
		small: {
			id: string;
			number: string;
			amountCents: number;
			vendorId: string;
			requesterId: string;
			assignedContractorId: string;
			workflowTag: string;
		};
		large: {
			id: string;
			number: string;
			amountCents: number;
			vendorId: string;
			requesterId: string;
			assignedContractorId: string;
			workflowTag: string;
		};
	};
	policyVariants: {
		smallInvoice: { collection: string; entityId: string; workflowTag: string };
		largeInvoice: { collection: string; entityId: string; workflowTag: string };
	};
	intentFixtures: Partial<Scn017IntentFixtures>;
	policyFilterUnindexed: {
		collection: string;
		entityId: string;
		sampleRow: Record<string, unknown>;
		draftSchema: Record<string, unknown>;
		workflowTag: string;
	};
	storyTags: typeof SCN017_STORY_TAGS;
	workflowTags: typeof SCN017_WORKFLOW_TAGS;
};

type SeedScn017Options = {
	seedIntentFixtures?: boolean;
	includeTaskSecret?: boolean;
};

function scn017UserSchema() {
	return {
		type: 'object',
		required: ['user_id', 'display_name', 'approval_role', 'procurement_role'],
		properties: {
			user_id: { type: 'string' },
			display_name: { type: 'string' },
			approval_role: { type: 'string' },
			procurement_role: { type: 'string' },
		},
	};
}

function scn017VendorSchema() {
	return {
		type: 'object',
		required: ['name', 'risk_rating'],
		properties: {
			name: { type: 'string' },
			risk_rating: { type: 'string' },
		},
	};
}

function scn017InvoiceSchema() {
	return {
		type: 'object',
		required: [
			'number',
			'vendor_id',
			'requester_id',
			'assigned_contractor_id',
			'status',
			'amount_cents',
			'currency',
			'commercial_terms',
		],
		properties: {
			number: { type: 'string' },
			vendor_id: { type: 'string' },
			requester_id: { type: 'string' },
			assigned_contractor_id: { type: 'string' },
			status: { type: 'string' },
			amount_cents: { type: 'integer' },
			currency: { type: 'string' },
			commercial_terms: { type: 'string' },
		},
	};
}

function scn017BudgetSchema() {
	return {
		type: 'object',
		properties: {
			title: { type: 'string' },
			budget_cents: { type: 'integer' },
			secret: { type: 'string' },
			status: { type: 'string' },
		},
	};
}

function scn017Identity() {
	return {
		user_id: 'subject.user_id',
		role: 'subject.attributes.approval_role',
		attributes: {
			approval_role: {
				from: 'collection',
				collection: SCN017_COLLECTIONS.users,
				key_field: 'user_id',
				key_subject: 'user_id',
				value_field: 'approval_role',
			},
			procurement_role: {
				from: 'collection',
				collection: SCN017_COLLECTIONS.users,
				key_field: 'user_id',
				key_subject: 'user_id',
				value_field: 'procurement_role',
			},
		},
	};
}

function approvalPolicy() {
	return {
		identity: scn017Identity(),
		read: { allow: [{ name: 'fixture-read' }] },
		create: { allow: [{ name: 'fixture-create' }] },
		update: {
			allow: [
				{
					name: 'finance-update',
					when: {
						subject: 'user_id',
						in: [SCN017_SUBJECTS.financeAgent, SCN017_SUBJECTS.financeApprover, 'finance-bot'],
					},
				},
			],
		},
		fields: {
			secret: {
				write: {
					deny: [
						{
							name: 'finance-agent-cannot-write-secret',
							when: { subject: 'user_id', eq: SCN017_SUBJECTS.financeAgent },
						},
					],
				},
			},
		},
		envelopes: {
			write: [
				{
					name: 'large-budget-needs-finance-approval',
					when: {
						all: [{ operation: 'update' }, { field: 'budget_cents', gt: 10000 }],
					},
					decision: 'needs_approval',
					approval: {
						role: SCN017_ROLES.financeApprover,
						reason_required: true,
						deadline_seconds: 86400,
						separation_of_duties: true,
					},
				},
			],
		},
	};
}

function procurementInvoicePolicy() {
	return {
		identity: scn017Identity(),
		read: {
			allow: [
				{
					name: 'finance-and-operators-read-invoices',
					when: {
						subject: 'procurement_role',
						in: [SCN017_ROLES.financeAgent, SCN017_ROLES.financeApprover, SCN017_ROLES.operator],
					},
				},
				{
					name: 'requester-reads-own-invoices',
					where: { field: 'requester_id', eq_subject: 'user_id' },
				},
				{
					name: 'contractor-reads-assigned-invoices',
					when: { subject: 'procurement_role', eq: SCN017_ROLES.contractor },
					where: { field: 'assigned_contractor_id', eq_subject: 'user_id' },
				},
			],
		},
		create: { allow: [{ name: 'fixture-create' }] },
		update: {
			allow: [
				{
					name: 'finance-agent-updates-invoice-metadata',
					when: { subject: 'procurement_role', eq: SCN017_ROLES.financeAgent },
				},
			],
		},
		fields: {
			amount_cents: {
				read: {
					deny: [
						{
							name: 'contractors-do-not-see-invoice-amounts',
							when: { subject: 'procurement_role', eq: SCN017_ROLES.contractor },
							redact_as: null,
						},
					],
				},
			},
			commercial_terms: {
				read: {
					deny: [
						{
							name: 'contractors-do-not-see-commercial-terms',
							when: { subject: 'procurement_role', eq: SCN017_ROLES.contractor },
							redact_as: null,
						},
					],
				},
			},
		},
		envelopes: {
			write: [
				{
					name: 'auto-approve-small-invoice-update',
					when: {
						all: [{ operation: 'update' }, { field: 'amount_cents', lt: 1_000_000 }],
					},
					decision: 'allow',
				},
				{
					name: 'require-approval-large-invoice-update',
					when: {
						all: [{ operation: 'update' }, { field: 'amount_cents', gt: 1_000_000 }],
					},
					decision: 'needs_approval',
					approval: {
						role: SCN017_ROLES.financeApprover,
						reason_required: true,
						deadline_seconds: 86400,
						separation_of_duties: true,
					},
				},
			],
		},
	};
}

function policyFilterUnindexedDraft() {
	return {
		version: 1,
		entity_schema: {
			type: 'object',
			required: ['title', 'reviewer_email', 'status'],
			properties: {
				title: { type: 'string' },
				reviewer_email: { type: 'string' },
				status: { type: 'string' },
			},
		},
		access_control: {
			identity: {
				user_id: 'subject.user_id',
				email: 'subject.attributes.email',
			},
			read: {
				allow: [
					{
						name: 'reviewers-read-own-items',
						where: { field: 'reviewer_email', eq_subject: 'email' },
					},
				],
			},
			create: { allow: [{ name: 'fixture-create' }] },
		},
		indexes: [{ field: 'status', type: 'string', unique: false }],
	};
}

function scn017UserSeeds() {
	return {
		financeAgent: {
			id: SCN017_SUBJECTS.financeAgent,
			displayName: 'Finance Agent',
			approvalRole: SCN017_ROLES.financeAgent,
			procurementRole: SCN017_ROLES.financeAgent,
		},
		financeApprover: {
			id: SCN017_SUBJECTS.financeApprover,
			displayName: 'Finance Approver',
			approvalRole: SCN017_ROLES.financeApprover,
			procurementRole: SCN017_ROLES.financeApprover,
		},
		requester: {
			id: SCN017_SUBJECTS.requester,
			displayName: 'Requester',
			approvalRole: SCN017_ROLES.requester,
			procurementRole: SCN017_ROLES.requester,
		},
		operator: {
			id: SCN017_SUBJECTS.operator,
			displayName: 'Operator',
			approvalRole: SCN017_ROLES.operator,
			procurementRole: SCN017_ROLES.operator,
		},
		contractor: {
			id: SCN017_SUBJECTS.contractor,
			displayName: 'Contractor',
			approvalRole: SCN017_ROLES.contractor,
			procurementRole: SCN017_ROLES.contractor,
		},
	};
}

export function dbIntentsUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/intents`;
}

export function dbIntentUrl(db: TestDatabase, intentId: string): string {
	return `${dbIntentsUrl(db)}/${encodeURIComponent(intentId)}`;
}

export async function createBudgetRecord(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	id: string,
	data: Record<string, unknown> = {},
) {
	await createTestEntity(request, db, collection, id, {
		title: id,
		budget_cents: 5000,
		status: 'draft',
		secret: 'alpha',
		...data,
	});
}

export async function updateApprovalRole(
	request: APIRequestContext,
	db: TestDatabase,
	actor: string,
	role: string,
	expectedVersion = 1,
) {
	await updateTestEntity(
		request,
		db,
		SCN017_COLLECTIONS.users,
		actor,
		{
			user_id: actor,
			display_name: actor,
			approval_role: role,
			procurement_role:
				actor === SCN017_SUBJECTS.contractor ? SCN017_ROLES.contractor : SCN017_ROLES.operator,
		},
		expectedVersion,
	);
}

export async function previewBudgetIntent(
	request: APIRequestContext,
	db: TestDatabase,
	entityId: string,
	budgetCents: number,
	options: {
		collection?: string;
		actor?: string;
		agentId?: string;
		credentialId?: string;
		delegatedBy?: string;
		grantVersion?: number;
		tenantRole?: string;
		expiresInSeconds?: number;
	} = {},
): Promise<PreviewedIntent> {
	const {
		collection = TASK_COLLECTION,
		actor = SCN017_SUBJECTS.financeAgent,
		agentId,
		credentialId = `cred-${actor}`,
		delegatedBy,
		grantVersion = 7,
		tenantRole = actor === SCN017_SUBJECTS.financeApprover
			? SCN017_ROLES.financeApprover
			: SCN017_ROLES.financeAgent,
		expiresInSeconds = 600,
	} = options;
	const subjectBlock = [`userId: "${actor}"`, ...(agentId ? [`agentId: "${agentId}"`] : [])].join(
		'\n\t\t\t\t\t',
	);
	const fullSubjectBlock = [
		subjectBlock,
		`tenantRole: "${tenantRole}"`,
		`credentialId: "${credentialId}"`,
		`grantVersion: ${grantVersion}`,
		...(delegatedBy ? [`delegatedBy: "${delegatedBy}"`] : []),
	].join('\n\t\t\t\t\t');
	const data = await gqlAs(
		request,
		db,
		actor,
		`mutation {
			previewMutation(input: {
				operation: {
					operationKind: "patch_entity"
					operation: {
						collection: "${collection}"
						id: "${entityId}"
						expected_version: 1
						patch: { budget_cents: ${budgetCents} }
					}
				}
				subject: {
					${fullSubjectBlock}
				}
				expiresInSeconds: ${expiresInSeconds}
			}) {
				intentToken
				intent { id approvalState decision }
			}
		}`,
	);
	const preview = data.previewMutation as {
		intentToken: string;
		intent: { id: string };
	};
	return { intentId: preview.intent.id, token: preview.intentToken };
}

export async function approveIntent(
	request: APIRequestContext,
	db: TestDatabase,
	intentId: string,
) {
	await gqlAs(
		request,
		db,
		SCN017_SUBJECTS.financeApprover,
		`mutation {
			approveMutationIntent(input: {
				intentId: "${intentId}"
				reason: "approved"
			}) { id approvalState }
		}`,
	);
}

export async function rejectIntent(request: APIRequestContext, db: TestDatabase, intentId: string) {
	await gqlAs(
		request,
		db,
		SCN017_SUBJECTS.financeApprover,
		`mutation {
			rejectMutationIntent(input: {
				intentId: "${intentId}"
				reason: "rejected"
			}) { id approvalState }
		}`,
	);
}

export async function commitIntent(
	request: APIRequestContext,
	db: TestDatabase,
	intent: PreviewedIntent,
) {
	await gqlAs(
		request,
		db,
		SCN017_SUBJECTS.financeAgent,
		`mutation {
			commitMutationIntent(input: {
				intentToken: "${intent.token}"
				intentId: "${intent.intentId}"
			}) { committed intent { id approvalState } }
		}`,
	);
}

export async function patchBudgetRecordAs(
	request: APIRequestContext,
	db: TestDatabase,
	actor: string,
	collection: string,
	id: string,
	budgetCents: number,
	expectedVersion = 1,
) {
	await gqlAs(
		request,
		db,
		actor,
		`mutation {
			update${collection === TASK_COLLECTION ? 'Task' : 'Expense'}(
				id: "${id}"
				version: ${expectedVersion}
				input: {
					title: "${id}"
					budget_cents: ${budgetCents}
					status: "changed-after-preview"
				}
			) { id version }
		}`,
	);
}

async function seedScn017IntentFixtures(
	request: APIRequestContext,
	db: TestDatabase,
): Promise<Scn017IntentFixtures> {
	for (const id of [
		'task-pending',
		'task-approved',
		'task-rejected',
		'task-expired',
		'task-committed',
		'task-ui-approve',
		'task-stale',
	]) {
		await createBudgetRecord(request, db, TASK_COLLECTION, id);
	}
	await createBudgetRecord(request, db, EXPENSE_COLLECTION, 'expense-ui-reject');

	const pendingIntent = await previewBudgetIntent(request, db, 'task-pending', 20_000, {
		agentId: 'mcp.finance-cli',
		grantVersion: 9,
	});
	const approvedIntent = await previewBudgetIntent(request, db, 'task-approved', 21_000, {
		agentId: 'mcp.finance-cli',
		grantVersion: 11,
	});
	await approveIntent(request, db, approvedIntent.intentId);
	const rejectedIntent = await previewBudgetIntent(request, db, 'task-rejected', 22_000, {
		agentId: 'tool.bulk-review',
	});
	await rejectIntent(request, db, rejectedIntent.intentId);
	const expiredIntent = await previewBudgetIntent(request, db, 'task-expired', 6000, {
		agentId: 'tool.expiring',
		expiresInSeconds: 0,
	});
	const committedIntent = await previewBudgetIntent(request, db, 'task-committed', 6000, {
		agentId: 'tool.commit',
	});
	await commitIntent(request, db, committedIntent);
	const approveTargetIntent = await previewBudgetIntent(request, db, 'task-ui-approve', 23_000, {
		actor: 'finance-bot',
		agentId: 'tool.review-console',
		delegatedBy: SCN017_SUBJECTS.financeAgent,
		grantVersion: 13,
	});
	const rejectTargetIntent = await previewBudgetIntent(request, db, 'expense-ui-reject', 24_000, {
		collection: EXPENSE_COLLECTION,
		agentId: 'mcp.gateway',
		grantVersion: 15,
	});
	const staleIntent = await previewBudgetIntent(request, db, 'task-stale', 27_000, {
		grantVersion: 21,
	});
	await approveIntent(request, db, staleIntent.intentId);
	await patchBudgetRecordAs(
		request,
		db,
		SCN017_SUBJECTS.financeAgent,
		TASK_COLLECTION,
		'task-stale',
		5100,
	);

	await new Promise((resolve) => setTimeout(resolve, 5));

	return {
		pending: {
			...pendingIntent,
			actor: SCN017_SUBJECTS.financeAgent,
			collection: TASK_COLLECTION,
			entityId: 'task-pending',
			budgetCents: 20_000,
			state: 'pending',
			workflowTag: SCN017_WORKFLOW_TAGS.largeInvoice,
		},
		approved: {
			...approvedIntent,
			actor: SCN017_SUBJECTS.financeAgent,
			collection: TASK_COLLECTION,
			entityId: 'task-approved',
			budgetCents: 21_000,
			state: 'approved',
			workflowTag: SCN017_WORKFLOW_TAGS.largeInvoice,
		},
		rejected: {
			...rejectedIntent,
			actor: SCN017_SUBJECTS.financeAgent,
			collection: TASK_COLLECTION,
			entityId: 'task-rejected',
			budgetCents: 22_000,
			state: 'rejected',
			workflowTag: SCN017_WORKFLOW_TAGS.largeInvoice,
		},
		expired: {
			...expiredIntent,
			actor: SCN017_SUBJECTS.financeAgent,
			collection: TASK_COLLECTION,
			entityId: 'task-expired',
			budgetCents: 6000,
			state: 'expired',
			workflowTag: SCN017_WORKFLOW_TAGS.expiredIntent,
		},
		committed: {
			...committedIntent,
			actor: SCN017_SUBJECTS.financeAgent,
			collection: TASK_COLLECTION,
			entityId: 'task-committed',
			budgetCents: 6000,
			state: 'committed',
			workflowTag: SCN017_WORKFLOW_TAGS.smallInvoice,
		},
		approveTarget: {
			...approveTargetIntent,
			actor: 'finance-bot',
			collection: TASK_COLLECTION,
			entityId: 'task-ui-approve',
			budgetCents: 23_000,
			state: 'pending',
			workflowTag: SCN017_WORKFLOW_TAGS.largeInvoice,
		},
		rejectTarget: {
			...rejectTargetIntent,
			actor: SCN017_SUBJECTS.financeAgent,
			collection: EXPENSE_COLLECTION,
			entityId: 'expense-ui-reject',
			budgetCents: 24_000,
			state: 'pending',
			workflowTag: SCN017_WORKFLOW_TAGS.largeInvoice,
		},
		stale: {
			...staleIntent,
			actor: SCN017_SUBJECTS.financeAgent,
			collection: TASK_COLLECTION,
			entityId: 'task-stale',
			budgetCents: 27_000,
			state: 'approved',
			workflowTag: SCN017_WORKFLOW_TAGS.staleIntent,
		},
	};
}

export async function seedScn017PolicyUiFixture(
	request: APIRequestContext,
	prefix: string,
	options: SeedScn017Options = {},
): Promise<Scn017PolicyUiFixture> {
	const tenant = await createTestTenant(request, prefix);
	const db = await createTestDatabase(request, tenant);

	await createTestCollection(request, db, SCN017_COLLECTIONS.users, {
		entity_schema: scn017UserSchema(),
	});
	await createTestCollection(request, db, SCN017_COLLECTIONS.vendors, {
		entity_schema: scn017VendorSchema(),
	});
	await createTestCollection(request, db, SCN017_COLLECTIONS.invoices, {
		entity_schema: scn017InvoiceSchema(),
		access_control: procurementInvoicePolicy(),
	});
	await createTestCollection(request, db, TASK_COLLECTION, {
		entity_schema: scn017BudgetSchema(),
		access_control: approvalPolicy(),
	});
	await createTestCollection(request, db, EXPENSE_COLLECTION, {
		entity_schema: scn017BudgetSchema(),
		access_control: approvalPolicy(),
	});
	await createTestCollection(request, db, SCN017_COLLECTIONS.policyFilterUnindexed, {
		entity_schema: policyFilterUnindexedDraft().entity_schema as Record<string, unknown>,
		access_control: policyFilterUnindexedDraft().access_control as Record<string, unknown>,
		indexes: policyFilterUnindexedDraft().indexes as Record<string, unknown>[],
	});

	const users = scn017UserSeeds();
	for (const user of Object.values(users)) {
		await createTestEntity(request, db, SCN017_COLLECTIONS.users, user.id, {
			user_id: user.id,
			display_name: user.displayName,
			approval_role: user.approvalRole,
			procurement_role: user.procurementRole,
		});
	}

	const vendors = {
		primary: { id: 'vendor-acme', name: 'Acme Office Supply' },
		secondary: { id: 'vendor-zenith', name: 'Zenith Infrastructure' },
	};
	await createTestEntity(request, db, SCN017_COLLECTIONS.vendors, vendors.primary.id, {
		name: vendors.primary.name,
		risk_rating: 'low',
	});
	await createTestEntity(request, db, SCN017_COLLECTIONS.vendors, vendors.secondary.id, {
		name: vendors.secondary.name,
		risk_rating: 'medium',
	});

	const invoices = {
		small: {
			id: 'inv-under-threshold',
			number: 'INV-1001',
			amountCents: 750_000,
			vendorId: vendors.primary.id,
			requesterId: SCN017_SUBJECTS.requester,
			assignedContractorId: SCN017_SUBJECTS.contractor,
			workflowTag: SCN017_WORKFLOW_TAGS.smallInvoice,
		},
		large: {
			id: 'inv-over-threshold',
			number: 'INV-2001',
			amountCents: 1_250_000,
			vendorId: vendors.secondary.id,
			requesterId: SCN017_SUBJECTS.requester,
			assignedContractorId: SCN017_SUBJECTS.contractor,
			workflowTag: SCN017_WORKFLOW_TAGS.largeInvoice,
		},
	};
	await createTestEntity(request, db, SCN017_COLLECTIONS.invoices, invoices.small.id, {
		number: invoices.small.number,
		vendor_id: invoices.small.vendorId,
		requester_id: invoices.small.requesterId,
		assigned_contractor_id: invoices.small.assignedContractorId,
		status: 'submitted',
		amount_cents: invoices.small.amountCents,
		currency: 'USD',
		commercial_terms: 'net-30 standard procurement terms',
	});
	await createTestEntity(request, db, SCN017_COLLECTIONS.invoices, invoices.large.id, {
		number: invoices.large.number,
		vendor_id: invoices.large.vendorId,
		requester_id: invoices.large.requesterId,
		assigned_contractor_id: invoices.large.assignedContractorId,
		status: 'submitted',
		amount_cents: invoices.large.amountCents,
		currency: 'USD',
		commercial_terms: 'net-15 expedited infrastructure terms',
	});

	const taskA: Record<string, unknown> = {
		title: 'Budget request',
		budget_cents: 5000,
		status: 'draft',
	};
	if (options.includeTaskSecret) {
		taskA.secret = 'alpha';
	}
	await createTestEntity(request, db, TASK_COLLECTION, 'task-a', taskA);
	await createTestEntity(request, db, SCN017_COLLECTIONS.policyFilterUnindexed, 'policy-shadow', {
		title: 'Shadow policy row',
		reviewer_email: 'reviewer@example.com',
		status: 'pending',
	});

	const fixture: Scn017PolicyUiFixture = {
		tenant,
		db,
		collections: SCN017_COLLECTIONS,
		subjects: SCN017_SUBJECTS,
		roles: SCN017_ROLES,
		users,
		vendors,
		invoices,
		policyVariants: {
			smallInvoice: {
				collection: SCN017_COLLECTIONS.invoices,
				entityId: invoices.small.id,
				workflowTag: invoices.small.workflowTag,
			},
			largeInvoice: {
				collection: SCN017_COLLECTIONS.invoices,
				entityId: invoices.large.id,
				workflowTag: invoices.large.workflowTag,
			},
		},
		intentFixtures: {},
		policyFilterUnindexed: {
			collection: SCN017_COLLECTIONS.policyFilterUnindexed,
			entityId: 'policy-shadow',
			sampleRow: {
				title: 'Shadow policy row',
				reviewer_email: 'reviewer@example.com',
				status: 'pending',
			},
			draftSchema: policyFilterUnindexedDraft(),
			workflowTag: SCN017_WORKFLOW_TAGS.policyFilterUnindexed,
		},
		storyTags: SCN017_STORY_TAGS,
		workflowTags: SCN017_WORKFLOW_TAGS,
	};

	if (options.seedIntentFixtures) {
		fixture.intentFixtures = await seedScn017IntentFixtures(request, db);
	}

	return fixture;
}

export async function seedApprovalCollections(
	request: APIRequestContext,
	prefix: string,
): Promise<TestDatabase> {
	const fixture = await seedScn017PolicyUiFixture(request, prefix);
	return fixture.db;
}

export async function seedIntentCollection(
	request: APIRequestContext,
	prefix: string,
	includeSecret = false,
): Promise<TestDatabase> {
	const fixture = await seedScn017PolicyUiFixture(request, prefix, {
		includeTaskSecret: includeSecret,
	});
	return fixture.db;
}

/**
 * Build a proposed access_control draft based on the SCN-017 procurement
 * invoice policy with the contractor read tightened to deny on a stricter
 * threshold. Used by the schemas-tab happy-path test to prove an edited
 * policy compiles cleanly and changes nothing structurally relevant the
 * existing policy didn't already cover.
 */
export function proposedPolicyDraftDenyHigh(): Record<string, unknown> {
	const policy = procurementInvoicePolicy() as Record<string, unknown>;
	const fields = policy.fields as Record<string, Record<string, unknown> | undefined>;
	const amountCents = fields.amount_cents as Record<string, unknown> | undefined;
	if (!amountCents) {
		throw new Error('procurementInvoicePolicy.fields.amount_cents missing');
	}
	const original = amountCents.read as { deny: Array<Record<string, unknown>> };
	const tightenedDeny = [
		...original.deny,
		{
			name: 'tightened-amount-deny-during-dry-run',
			when: { subject: 'procurement_role', eq: SCN017_ROLES.requester },
			redact_as: null,
		},
	];
	amountCents.read = { deny: tightenedDeny };
	return policy;
}

/**
 * Build a proposed access_control draft with an unknown subject reference
 * so the FEAT-029 compiler emits a `policy_expression_invalid` diagnostic.
 * Used by the schemas-tab failed-compile gating test.
 */
export function proposedPolicyDraftBroken(): Record<string, unknown> {
	const policy = procurementInvoicePolicy() as Record<string, unknown>;
	const read = policy.read as { allow: Array<Record<string, unknown>> };
	read.allow = [
		...read.allow,
		{
			name: 'broken-unknown-subject',
			when: { subject: 'unknown_role', eq: 'nope' },
		},
	];
	return policy;
}

/**
 * Probe the persisted `access_control` JSON for a collection over GraphQL
 * to assert it is unchanged after a refused activation.
 */
export async function fetchPersistedAccessControl(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
): Promise<unknown> {
	const response = await request.post(graphqlPath(db), {
		data: {
			query: `query($name: String!) {
				collection(name: $name) {
					schema
				}
			}`,
			variables: { name: collection },
		},
	});
	const body = (await response.json()) as {
		data?: { collection?: { schema?: { access_control?: unknown } } };
		errors?: unknown;
	};
	expect(response.ok(), `${response.status()} ${JSON.stringify(body)}`).toBe(true);
	expect(body.errors ?? null, JSON.stringify(body.errors)).toBeNull();
	return body.data?.collection?.schema?.access_control ?? null;
}

/**
 * Activate a proposed access_control via GraphQL `putSchema`. Used by the
 * intent-audit-lineage spec to drive the audit-evidence assertion without
 * relying on the UI.
 */
export async function activateProposedPolicy(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	proposedAccessControl: Record<string, unknown>,
	options: { actor?: string } = {},
): Promise<{ schema: { version: number } }> {
	// Fetch the current schema so we can preserve everything except access_control.
	const fetched = await request.post(graphqlPath(db), {
		data: {
			query: `query($name: String!) { collection(name: $name) { schema } }`,
			variables: { name: collection },
		},
	});
	const fetchedBody = (await fetched.json()) as {
		data?: { collection?: { schema?: Record<string, unknown> } };
	};
	const current = fetchedBody.data?.collection?.schema;
	expect(current, 'collection schema should exist').toBeTruthy();
	const proposedSchema = {
		...(current ?? {}),
		access_control: proposedAccessControl,
		version: ((current?.version as number | undefined) ?? 1) + 1,
	};
	const response = await request.post(graphqlPath(db), {
		...(options.actor ? { headers: { 'x-axon-actor': options.actor } } : {}),
		data: {
			query: `mutation($collection: String!, $schema: JSON!) {
				putSchema(input: { collection: $collection, schema: $schema }) {
					schema
				}
			}`,
			variables: { collection, schema: proposedSchema },
		},
	});
	const body = (await response.json()) as {
		data?: { putSchema?: { schema?: { version?: number } } };
		errors?: unknown;
	};
	expect(response.ok(), `${response.status()} ${JSON.stringify(body)}`).toBe(true);
	expect(body.errors ?? null, JSON.stringify(body.errors)).toBeNull();
	return body.data?.putSchema as { schema: { version: number } };
}

export async function seedIntentStates(
	request: APIRequestContext,
	db: TestDatabase,
): Promise<SeededIntentIds> {
	const intents = await seedScn017IntentFixtures(request, db);
	return {
		pending: intents.pending.intentId,
		approved: intents.approved.intentId,
		rejected: intents.rejected.intentId,
		expired: intents.expired.intentId,
		committed: intents.committed.intentId,
		approveTarget: intents.approveTarget.intentId,
		rejectTarget: intents.rejectTarget.intentId,
	};
}
