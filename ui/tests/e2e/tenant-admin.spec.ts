import { expect, test } from '@playwright/test';
import {
	addTestTenantMember,
	createTestDatabase,
	createTestTenant,
	createTestUser,
	tenantUrl,
	type TestTenant,
	type TestUser,
} from './helpers';

/**
 * Gap-coverage specs for tenant administration flows that the initial
 * smoke test only touched at the surface: credential issue modal, member
 * role editing, tenant/database deletion, and the global user ACL.
 */

test.describe('Credentials — issue and display JWT', () => {
	let tenant: TestTenant;
	let testUser: TestUser;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'cred');
		await createTestDatabase(request, tenant);
		testUser = await createTestUser(request, 'cred-test-user');
		await addTestTenantMember(request, tenant, testUser, 'read');
	});

	test('Issue Credential modal mints a JWT and shows it once', async ({ page }) => {
		const controlRequests: string[] = [];
		page.on('request', (request) => {
			const path = new URL(request.url()).pathname;
			if (path.startsWith('/control/')) {
				controlRequests.push(path);
			}
		});

		await page.goto(tenantUrl(tenant, 'credentials'));
		await page.getByRole('button', { name: 'Issue Credential' }).click();

		// Fill the issue form — use UUID paste mode since the user picker needs
		// the user to exist in the list returned by /control/users/list.
		await page.getByRole('button', { name: 'UUID' }).click();
		await page.getByPlaceholder('Paste user UUID…').fill(testUser.id);
		await page.getByRole('button', { name: 'Use' }).click();
		// TTL default is fine; grants are pre-populated with one empty row.
		const grantRow = page.locator('.grant-row').first();
		await grantRow.getByPlaceholder('Database name').fill('default');
		await grantRow.getByRole('checkbox', { name: 'read' }).check();

		await page.getByRole('button', { name: 'Issue', exact: true }).click();

		// JWT modal appears.
		await expect(page.getByRole('heading', { name: 'Credential Issued' })).toBeVisible({
			timeout: 5_000,
		});
		const issuedJti = (await page.locator('.modal-body code').textContent())?.trim();
		expect(issuedJti).toBeTruthy();
		const issuedJtiPrefix = issuedJti?.slice(0, 12) ?? '';
		const jwtField = page.locator('textarea.jwt-display');
		await expect(jwtField).toBeVisible();
		const jwt = await jwtField.inputValue();
		expect(jwt.length).toBeGreaterThan(50);
		// JWTs have three dot-separated sections.
		expect(jwt.split('.').length).toBe(3);

		// Close the JWT dialog and verify the credential appears in the list.
		await page.getByRole('button', { name: 'Close' }).click();
		await expect(page.getByRole('heading', { name: 'Credentials' }).first()).toBeVisible();
		await expect(page.getByText('Loading credentials…')).toHaveCount(0, { timeout: 15_000 });

		const row = page.locator('tr', { hasText: issuedJtiPrefix }).first();
		await expect(row.getByText('active')).toBeVisible({ timeout: 10_000 });
		await row.getByRole('button', { name: 'Revoke' }).click();
		await row.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.locator('table').getByText('revoked').first()).toBeVisible({
			timeout: 5_000,
		});

		expect(controlRequests).toContain('/control/graphql');
		expect(
			controlRequests.filter((path) => path !== '/control/graphql'),
			'Credential UI should use control GraphQL rather than REST control routes',
		).toEqual([]);
	});
});

test.describe('Tenant members — add, change role, remove', () => {
	let tenant: TestTenant;
	let testUser: TestUser;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'mbr');
		testUser = await createTestUser(request, 'mbr-test-user');
	});

	test('add a member, change role via dropdown, then remove', async ({ page }) => {
		const controlRequests: string[] = [];
		page.on('request', (request) => {
			const path = new URL(request.url()).pathname;
			if (path.startsWith('/control/')) {
				controlRequests.push(path);
			}
		});

		await page.goto(tenantUrl(tenant, 'members'));

		// Add via UUID paste mode (the user picker's fallback for direct UUID entry).
		await page.getByRole('button', { name: 'UUID' }).click();
		await page.getByPlaceholder('Paste user UUID…').fill(testUser.id);
		await page.getByRole('button', { name: 'Use' }).click();
		await page.getByRole('button', { name: 'Add' }).click();
		await expect(page.locator('table').getByText(testUser.id.slice(0, 8))).toBeVisible({
			timeout: 5_000,
		});

		// Change role via dropdown: find the row, change the select to 'admin'.
		const idPrefix = testUser.id.slice(0, 8);
		const row = page.locator('tr', { hasText: idPrefix });
		await row.locator('select').selectOption('admin');

		// Reload and verify the role stuck.
		await page.reload();
		await expect(page.locator('tr', { hasText: idPrefix }).locator('select')).toHaveValue('admin');

		// Remove.
		await page.locator('tr', { hasText: idPrefix }).getByRole('button', { name: 'Remove' }).click();
		await page.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.locator('table').getByText(idPrefix)).toHaveCount(0);

		expect(controlRequests).toContain('/control/graphql');
		expect(
			controlRequests.filter((path) => path !== '/control/graphql'),
			'Member UI should use control GraphQL rather than REST control routes',
		).toEqual([]);
	});
});

test.describe('Global users ACL — add, change, remove', () => {
	const login = `test-user-${Date.now().toString(36)}`;
	const displayName = `Provisioned ${Date.now().toString(36)}`;
	const email = `${login}@example.test`;

	test('add a user, change role, remove', async ({ page }) => {
		const controlRequests: string[] = [];
		page.on('request', (request) => {
			const path = new URL(request.url()).pathname;
			if (path.startsWith('/control/')) {
				controlRequests.push(path);
			}
		});

		await page.goto('/ui/users');

		await page.getByPlaceholder('Display name (required)').fill(displayName);
		await page.getByPlaceholder('Email (optional)').fill(email);
		await page.getByRole('button', { name: 'Create User' }).click();

		const provisionedRow = page.locator('tr', { hasText: displayName });
		await expect(provisionedRow).toBeVisible({ timeout: 5_000 });
		await expect(provisionedRow.getByText('Active')).toBeVisible();
		await provisionedRow.getByRole('button', { name: 'Suspend' }).click();
		await provisionedRow.getByRole('button', { name: 'Confirm' }).click();
		await expect(provisionedRow.getByText('Suspended')).toBeVisible({ timeout: 5_000 });

		// Add as "read".
		await page.getByPlaceholder('Login (principal)').fill(login);
		await page.getByRole('button', { name: 'Assign' }).click();

		const row = page.locator('tr', { hasText: login });
		await expect(row).toBeVisible({ timeout: 5_000 });

		// Change to admin.
		const roleUpdate = page.waitForResponse((response) => {
			const postData = response.request().postData() ?? '';
			return (
				response.url().endsWith('/control/graphql') &&
				postData.includes('AxonUiSetUserRole') &&
				postData.includes(`"login":"${login}"`) &&
				postData.includes('"role":"admin"') &&
				response.ok()
			);
		});
		await row.locator('select').selectOption('admin');
		await roleUpdate;
		await page.reload();
		await expect(page.locator('tr', { hasText: login }).locator('select')).toHaveValue('admin');

		// Remove.
		await page.locator('tr', { hasText: login }).getByRole('button', { name: 'Remove' }).click();
		await page.getByRole('button', { name: 'Confirm' }).click();
		await expect(
			page.locator('section', { hasText: 'ACL Entries' }).locator('tr', { hasText: login }),
		).toHaveCount(0);

		expect(controlRequests).toContain('/control/graphql');
		expect(
			controlRequests.filter((path) => path !== '/control/graphql'),
			'Users UI should use control GraphQL rather than REST control routes',
		).toEqual([]);
	});
});

test.describe('Database create + delete', () => {
	let tenant: TestTenant;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'dbcd');
	});

	test('create a database via UI, then delete it', async ({ page }) => {
		const controlRequests: string[] = [];
		page.on('request', (request) => {
			const path = new URL(request.url()).pathname;
			if (path.startsWith('/control/')) {
				controlRequests.push(path);
			}
		});

		await page.goto(tenantUrl(tenant));
		await page.getByPlaceholder(/Database name/).fill('scratch');
		await page.getByRole('button', { name: 'Create', exact: true }).click();

		// Row appears with Open link + Delete.
		const row = page.locator('tr', { hasText: 'scratch' });
		await expect(row).toBeVisible({ timeout: 5_000 });

		// Delete with confirmation.
		await row.getByRole('button', { name: 'Delete' }).click();
		await row.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.locator('tr', { hasText: 'scratch' })).toHaveCount(0);

		expect(controlRequests).toContain('/control/graphql');
		expect(
			controlRequests.filter((path) => path !== '/control/graphql'),
			'Database UI should use control GraphQL rather than REST control routes',
		).toEqual([]);
	});
});
