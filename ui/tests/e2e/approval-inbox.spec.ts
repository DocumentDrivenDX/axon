import { type APIRequestContext, type Page, expect, test } from '@playwright/test';
import {
	type TestDatabase,
	createTestCollection,
	createTestDatabase,
	createTestEntity,
	createTestTenant,
} from './helpers';

const TASK_COLLECTION = 'task';
const EXPENSE_COLLECTION = 'expense';

type PreviewedIntent = {
	intentId: string;
	token: string;
};

type SeededIntentIds = {
	pending: string;
	approved: string;
	rejected: string;
	expired: string;
	committed: string;
	approveTarget: string;
	rejectTarget: string;
};

function dbIntentsUrl(db: TestDatabase): string {
	return `/ui/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/intents`;
}

function dbIntentUrl(db: TestDatabase, intentId: string): string {
	return `${dbIntentsUrl(db)}/${encodeURIComponent(intentId)}`;
}

function graphqlPath(db: TestDatabase): string {
	return `/tenants/${encodeURIComponent(db.tenant.db_name)}/databases/${encodeURIComponent(db.name)}/graphql`;
}

function taskSchema() {
	return {
		type: 'object',
		properties: {
			title: { type: 'string' },
			budget_cents: { type: 'integer' },
			status: { type: 'string' },
		},
	};
}

function userSchema() {
	return {
		type: 'object',
		required: ['user_id', 'approval_role'],
		properties: {
			user_id: { type: 'string' },
			approval_role: { type: 'string' },
		},
	};
}

function approvalPolicy() {
	return {
		identity: {
			user_id: 'subject.user_id',
			role: 'subject.attributes.approval_role',
			attributes: {
				approval_role: {
					from: 'collection',
					collection: 'users',
					key_field: 'user_id',
					key_subject: 'user_id',
					value_field: 'approval_role',
				},
			},
		},
		read: { allow: [{ name: 'fixture-read' }] },
		create: { allow: [{ name: 'fixture-create' }] },
		update: {
			allow: [
				{
					name: 'finance-update',
					when: {
						subject: 'user_id',
						in: ['finance-agent', 'finance-approver', 'finance-bot'],
					},
				},
			],
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
						role: 'finance_approver',
						reason_required: true,
						deadline_seconds: 86400,
						separation_of_duties: true,
					},
				},
			],
		},
	};
}

async function gqlAs(
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

async function routeGraphqlAs(page: Page, actor: string) {
	await page.route('**/graphql', async (route) => {
		await route.continue({
			headers: {
				...route.request().headers(),
				'x-axon-actor': actor,
			},
		});
	});
}

async function seedApprovalCollections(
	request: APIRequestContext,
	prefix: string,
): Promise<TestDatabase> {
	const tenant = await createTestTenant(request, prefix);
	const db = await createTestDatabase(request, tenant);
	await createTestCollection(request, db, 'users', { entity_schema: userSchema() });
	await createTestCollection(request, db, TASK_COLLECTION, {
		entity_schema: taskSchema(),
		access_control: approvalPolicy(),
	});
	await createTestCollection(request, db, EXPENSE_COLLECTION, {
		entity_schema: taskSchema(),
		access_control: approvalPolicy(),
	});
	for (const [id, role] of [
		['finance-agent', 'finance_agent'],
		['finance-approver', 'finance_approver'],
		['finance-bot', 'finance_agent'],
	] as const) {
		await createTestEntity(request, db, 'users', id, {
			user_id: id,
			approval_role: role,
		});
	}
	return db;
}

async function createBudgetRecord(
	request: APIRequestContext,
	db: TestDatabase,
	collection: string,
	id: string,
) {
	await createTestEntity(request, db, collection, id, {
		title: id,
		budget_cents: 5000,
		status: 'draft',
	});
}

async function previewBudgetIntent(
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
		actor = 'finance-agent',
		agentId,
		credentialId = `cred-${actor}`,
		delegatedBy,
		grantVersion = 7,
		tenantRole = actor === 'finance-approver' ? 'finance_approver' : 'finance_agent',
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

async function approveIntent(request: APIRequestContext, db: TestDatabase, intentId: string) {
	await gqlAs(
		request,
		db,
		'finance-approver',
		`mutation {
			approveMutationIntent(input: {
				intentId: "${intentId}"
				reason: "approved"
			}) { id approvalState }
		}`,
	);
}

async function rejectIntent(request: APIRequestContext, db: TestDatabase, intentId: string) {
	await gqlAs(
		request,
		db,
		'finance-approver',
		`mutation {
			rejectMutationIntent(input: {
				intentId: "${intentId}"
				reason: "rejected"
			}) { id approvalState }
		}`,
	);
}

async function commitIntent(request: APIRequestContext, db: TestDatabase, intent: PreviewedIntent) {
	await gqlAs(
		request,
		db,
		'finance-agent',
		`mutation {
			commitMutationIntent(input: {
				intentToken: "${intent.token}"
				intentId: "${intent.intentId}"
			}) { committed intent { id approvalState } }
		}`,
	);
}

async function seedIntentStates(
	request: APIRequestContext,
	db: TestDatabase,
): Promise<SeededIntentIds> {
	for (const id of [
		'task-pending',
		'task-approved',
		'task-rejected',
		'task-expired',
		'task-committed',
		'task-ui-approve',
	]) {
		await createBudgetRecord(request, db, TASK_COLLECTION, id);
	}
	await createBudgetRecord(request, db, EXPENSE_COLLECTION, 'expense-ui-reject');

	const pending = await previewBudgetIntent(request, db, 'task-pending', 20_000, {
		agentId: 'mcp.finance-cli',
		grantVersion: 9,
	});
	const approved = await previewBudgetIntent(request, db, 'task-approved', 21_000, {
		agentId: 'mcp.finance-cli',
		grantVersion: 11,
	});
	await approveIntent(request, db, approved.intentId);
	const rejected = await previewBudgetIntent(request, db, 'task-rejected', 22_000, {
		agentId: 'tool.bulk-review',
	});
	await rejectIntent(request, db, rejected.intentId);
	const expired = await previewBudgetIntent(request, db, 'task-expired', 6000, {
		agentId: 'tool.expiring',
		expiresInSeconds: 0,
	});
	const committed = await previewBudgetIntent(request, db, 'task-committed', 6000, {
		agentId: 'tool.commit',
	});
	await commitIntent(request, db, committed);
	const approveTarget = await previewBudgetIntent(request, db, 'task-ui-approve', 23_000, {
		actor: 'finance-bot',
		agentId: 'tool.review-console',
		delegatedBy: 'finance-agent',
		grantVersion: 13,
	});
	const rejectTarget = await previewBudgetIntent(request, db, 'expense-ui-reject', 24_000, {
		collection: EXPENSE_COLLECTION,
		agentId: 'mcp.gateway',
		grantVersion: 15,
	});

	await new Promise((resolve) => setTimeout(resolve, 5));

	return {
		pending: pending.intentId,
		approved: approved.intentId,
		rejected: rejected.intentId,
		expired: expired.intentId,
		committed: committed.intentId,
		approveTarget: approveTarget.intentId,
		rejectTarget: rejectTarget.intentId,
	};
}

async function selectStatus(page: Page, status: string) {
	await page.getByRole('tab', { name: status }).click();
}

test.describe('Approval inbox', () => {
	test('lists scoped intents across review states and opens detail', async ({ page, request }) => {
		const db = await seedApprovalCollections(request, 'approval-inbox');
		const ids = await seedIntentStates(request, db);
		const foreignDb = await seedApprovalCollections(request, 'approval-foreign');
		await createBudgetRecord(request, foreignDb, TASK_COLLECTION, 'task-foreign');
		const foreign = await previewBudgetIntent(request, foreignDb, 'task-foreign', 20_000);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentsUrl(db));

		await expect(page.getByRole('heading', { name: 'Mutation Intents' })).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.pending}`)).toBeVisible();
		await expect(page.getByText(foreign.intentId)).toHaveCount(0);
		await selectStatus(page, 'history');
		await expect(page.getByTestId(`intent-row-${ids.approved}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.rejected}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.expired}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.committed}`)).toBeVisible();

		for (const [status, intentId] of [
			['approved', ids.approved],
			['rejected', ids.rejected],
			['expired', ids.expired],
			['committed', ids.committed],
		] as const) {
			await selectStatus(page, status);
			await expect(page.getByTestId(`intent-row-${intentId}`)).toBeVisible();
		}

		await selectStatus(page, 'approved');
		await page.getByTestId(`intent-row-${ids.approved}`).click();
		await page.getByTestId('intent-open-detail').click();
		const detail = page.getByTestId('intent-detail');
		await expect(detail).toContainText(ids.approved);
		await expect(detail).toContainText('approved');
		await expect(detail).toContainText('large-budget-needs-finance-approval');
		await expect(detail).toContainText('task/task-approved');
		await expect(page.getByTestId('intent-bindings')).toContainText('cred-finance-agent');
		await expect(page.getByTestId('intent-bindings')).toContainText('11');
		await expect(page.getByTestId('intent-diff')).toContainText('budget_cents');
		await expect(page.getByTestId('intent-audit-trail')).toContainText('intent.approve');
		await expect(page.getByTestId('intent-audit-trail')).toContainText('approved');
		await expect(page.getByTestId('intent-deep-links')).toContainText('Open audit log');
	});

	test('supports dense filters, keyboard selection, and inline review without leaving inbox', async ({
		page,
		request,
	}) => {
		const db = await seedApprovalCollections(request, 'approval-filters');
		const ids = await seedIntentStates(request, db);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentsUrl(db));

		await page.getByTestId('intent-filter-requester').fill('finance-bot');
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.pending}`)).toHaveCount(0);

		await page.getByTestId('intent-filter-requester').fill('');
		await page.getByTestId('intent-filter-subject').fill('expense-ui-reject');
		await page.getByTestId('intent-filter-role').selectOption('finance_approver');
		await page.getByTestId('intent-filter-risk').fill('large-budget-needs-finance-approval');
		await page.getByTestId('intent-filter-collection').selectOption(EXPENSE_COLLECTION);
		await page.getByTestId('intent-filter-origin').selectOption('mcp.gateway');
		await expect(page.getByTestId(`intent-row-${ids.rejectTarget}`)).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toHaveCount(0);

		await page.getByTestId('intent-filter-age').selectOption('older_than_day');
		await expect(page.getByTestId('intent-empty')).toBeVisible();
		await page.getByTestId('intent-filter-age').selectOption('last_hour');
		await expect(page.getByTestId(`intent-row-${ids.rejectTarget}`)).toBeVisible();
		await page.getByRole('button', { name: 'Clear filters' }).click();

		await page.getByTestId('intent-inbox-grid').focus();
		await page.keyboard.press('Home');
		await page.keyboard.press('ArrowDown');
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toHaveAttribute(
			'aria-selected',
			'true',
		);
		await page.getByTestId('intent-inline-reason').fill('approved in inbox');
		await page.getByTestId('intent-inline-approve').click();
		await expect(page.getByTestId('intent-inline-message')).toContainText('Intent approved.');
		await expect(page).toHaveURL(dbIntentsUrl(db));
		await expect(page.getByTestId(`intent-row-${ids.approveTarget}`)).toHaveCount(0);

		await page.getByTestId(`intent-row-${ids.rejectTarget}`).click();
		await page.getByTestId('intent-inline-reason').fill('not enough context');
		await page.getByTestId('intent-inline-reject').click();
		await expect(page.getByTestId('intent-inline-message')).toContainText('Intent rejected.');
		await expect(page).toHaveURL(dbIntentsUrl(db));
		await expect(page.getByTestId(`intent-row-${ids.rejectTarget}`)).toHaveCount(0);
	});

	test('approves and rejects pending intents from the detail route', async ({ page, request }) => {
		const db = await seedApprovalCollections(request, 'approval-actions');
		const ids = await seedIntentStates(request, db);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentUrl(db, ids.approveTarget));
		await page.getByTestId('intent-reason').fill('approved in inbox');
		await page.getByTestId('intent-approve').click();
		await expect(page.getByText('Intent approved.')).toBeVisible({ timeout: 10_000 });
		await expect(page.getByTestId('intent-detail')).toContainText('approved');

		await page.goto(dbIntentUrl(db, ids.rejectTarget));
		await page.getByTestId('intent-reason').fill('not enough context');
		await page.getByTestId('intent-reject').click();
		await expect(page.getByText('Intent rejected.')).toBeVisible({ timeout: 10_000 });
		await expect(page.getByTestId('intent-detail')).toContainText('rejected');
	});

	test('distinguishes loading, empty, and error states', async ({ page, request }) => {
		const db = await seedApprovalCollections(request, 'approval-states');
		const ids = await seedIntentStates(request, db);
		let releaseLoad!: () => void;
		const loadGate = new Promise<void>((resolve) => {
			releaseLoad = resolve;
		});

		await page.route('**/graphql', async (route) => {
			const body = route.request().postDataJSON() as { query?: string };
			if (body.query?.includes('pendingMutationIntents')) {
				await loadGate;
			}
			await route.continue({
				headers: {
					...route.request().headers(),
					'x-axon-actor': 'finance-approver',
				},
			});
		});

		const navigation = page.goto(dbIntentsUrl(db));
		await expect(page.getByTestId('intent-loading')).toBeVisible();
		releaseLoad();
		await navigation;
		await expect(page.getByTestId(`intent-row-${ids.pending}`)).toBeVisible();

		await page.getByTestId('intent-filter-subject').fill('does-not-exist');
		await expect(page.getByTestId('intent-empty')).toBeVisible();

		await page.unroute('**/graphql');
		await page.route('**/graphql', async (route) => {
			const body = route.request().postDataJSON() as { query?: string };
			if (body.query?.includes('pendingMutationIntents')) {
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({
						errors: [{ message: 'forced inbox error' }],
					}),
				});
				return;
			}
			await route.continue({
				headers: {
					...route.request().headers(),
					'x-axon-actor': 'finance-approver',
				},
			});
		});

		await page.reload();
		await expect(page.getByTestId('intent-error')).toContainText('forced inbox error');
	});
});
