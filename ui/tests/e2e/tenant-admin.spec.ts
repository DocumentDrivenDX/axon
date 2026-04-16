import { expect, test } from '@playwright/test';
import {
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
	});

	test('Issue Credential modal mints a JWT and shows it once', async ({ page }) => {
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
		const jwtField = page.locator('textarea.jwt-display');
		await expect(jwtField).toBeVisible();
		const jwt = await jwtField.inputValue();
		expect(jwt.length).toBeGreaterThan(50);
		// JWTs have three dot-separated sections.
		expect(jwt.split('.').length).toBe(3);

		// Close the JWT dialog and verify the credential appears in the list.
		await page.getByRole('button', { name: 'Close' }).click();
		await expect(page.getByRole('heading', { name: 'Credentials' }).first()).toBeVisible();
		// Expect at least one row with 'active' status.
		await expect(page.locator('table').getByText('active').first()).toBeVisible();
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
		await expect(
			page.locator('tr', { hasText: idPrefix }).locator('select'),
		).toHaveValue('admin');

		// Remove.
		await page.locator('tr', { hasText: idPrefix }).getByRole('button', { name: 'Remove' }).click();
		await page.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.locator('table').getByText(idPrefix)).toHaveCount(0);
	});
});

test.describe('Global users ACL — add, change, remove', () => {
	const login = `test-user-${Date.now().toString(36)}`;

	test('add a user, change role, remove', async ({ page }) => {
		await page.goto('/ui/users');

		// Add as "read".
		await page.getByPlaceholder('Login (principal)').fill(login);
		await page.getByRole('button', { name: 'Assign' }).click();

		const row = page.locator('tr', { hasText: login });
		await expect(row).toBeVisible({ timeout: 5_000 });

		// Change to admin.
		await row.locator('select').selectOption('admin');
		await page.reload();
		await expect(page.locator('tr', { hasText: login }).locator('select')).toHaveValue('admin');

		// Remove.
		await page.locator('tr', { hasText: login }).getByRole('button', { name: 'Remove' }).click();
		await page.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.locator('table').getByText(login)).toHaveCount(0);
	});
});

test.describe('Database create + delete', () => {
	let tenant: TestTenant;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'dbcd');
	});

	test('create a database via UI, then delete it', async ({ page }) => {
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
	});
});
