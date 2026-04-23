import { type APIRequestContext, type Page, expect, test } from '@playwright/test';
import {
	type TestDatabase,
	createTestCollection,
	createTestDatabase,
	createTestEntity,
	createTestTenant,
} from './helpers';

const TASK_COLLECTION = 'task';

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
					when: { subject: 'user_id', in: ['finance-agent', 'finance-approver'] },
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
	for (const [id, role] of [
		['finance-agent', 'finance_agent'],
		['finance-approver', 'finance_approver'],
	]) {
		await createTestEntity(request, db, 'users', id, {
			user_id: id,
			approval_role: role,
		});
	}
	return db;
}

async function createTask(request: APIRequestContext, db: TestDatabase, id: string) {
	await createTestEntity(request, db, TASK_COLLECTION, id, {
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
	expiresInSeconds = 600,
): Promise<PreviewedIntent> {
	const data = await gqlAs(
		request,
		db,
		'finance-agent',
		`mutation {
			previewMutation(input: {
				operation: {
					operationKind: "patch_entity"
					operation: {
						collection: "${TASK_COLLECTION}"
						id: "${entityId}"
						expected_version: 1
						patch: { budget_cents: ${budgetCents} }
					}
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
		'task-ui-reject',
	]) {
		await createTask(request, db, id);
	}

	const pending = await previewBudgetIntent(request, db, 'task-pending', 20_000);
	const approved = await previewBudgetIntent(request, db, 'task-approved', 21_000);
	await approveIntent(request, db, approved.intentId);
	const rejected = await previewBudgetIntent(request, db, 'task-rejected', 22_000);
	await rejectIntent(request, db, rejected.intentId);
	const expired = await previewBudgetIntent(request, db, 'task-expired', 6000, 0);
	const committed = await previewBudgetIntent(request, db, 'task-committed', 6000);
	await commitIntent(request, db, committed);
	const approveTarget = await previewBudgetIntent(request, db, 'task-ui-approve', 23_000);
	const rejectTarget = await previewBudgetIntent(request, db, 'task-ui-reject', 24_000);

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
		await createTask(request, foreignDb, 'task-foreign');
		const foreign = await previewBudgetIntent(request, foreignDb, 'task-foreign', 20_000);

		await routeGraphqlAs(page, 'finance-approver');
		await page.goto(dbIntentsUrl(db));

		await expect(page.getByRole('heading', { name: 'Mutation Intents' })).toBeVisible();
		await expect(page.getByTestId(`intent-row-${ids.pending}`)).toBeVisible();
		await expect(page.getByText(foreign.intentId)).toHaveCount(0);

		for (const [status, intentId] of [
			['approved', ids.approved],
			['rejected', ids.rejected],
			['expired', ids.expired],
			['committed', ids.committed],
		]) {
			await selectStatus(page, status);
			await expect(page.getByTestId(`intent-row-${intentId}`)).toBeVisible();
		}

		await selectStatus(page, 'approved');
		await page.getByTestId(`intent-row-${ids.approved}`).getByRole('link').click();
		const detail = page.getByTestId('intent-detail');
		await expect(detail).toContainText(ids.approved);
		await expect(detail).toContainText('approved');
		await expect(detail).toContainText('large-budget-needs-finance-approval');
		await expect(detail).toContainText('task/task-approved');
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
});
