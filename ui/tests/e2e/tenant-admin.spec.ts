import { expect, test } from '@playwright/test';
import {
	createTestDatabase,
	createTestTenant,
	tenantUrl,
	type TestTenant,
} from './helpers';

/**
 * Gap-coverage specs for tenant administration flows that the initial
 * smoke test only touched at the surface: credential issue modal, member
 * role editing, tenant/database deletion, and the global user ACL.
 */

test.describe('Credentials — issue and display JWT', () => {
	let tenant: TestTenant;

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'cred');
		await createTestDatabase(request, tenant);
	});

	// Skipped: issuing a credential requires the target_user to be a member of
	// the tenant, which requires a user row in `users`, which requires OIDC
	// federation (there's no REST path to provision a user). Same gap as the
	// "tenant members" test. Tracked as a DDx bead.
	test.skip('Issue Credential modal mints a JWT and shows it once', async ({ page }) => {
		await page.goto(tenantUrl(tenant, 'credentials'));
		await page.getByRole('button', { name: 'Issue Credential' }).click();

		// Fill the issue form.
		await page.getByPlaceholder('UUID').fill('00000000-0000-0000-0000-000000000042');
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
	// A throwaway UUID string to use as a user_id.
	const userId = '11111111-1111-1111-1111-111111111111';

	test.beforeAll(async ({ request }) => {
		tenant = await createTestTenant(request, 'mbr');
	});

	// Skipped until the backend exposes a user-provisioning endpoint.
	// Adding a member currently fails with a FOREIGN KEY constraint because
	// tenant_users.user_id → users.id and there's no REST path that inserts
	// a bare user row (only upsert_user_identity via OIDC federation).
	// Tracked as a DDx bead.
	test.skip('add a member, change role via dropdown, then remove', async ({ page }) => {
		await page.goto(tenantUrl(tenant, 'members'));

		// Add.
		await page.getByPlaceholder('User ID (UUID)').fill(userId);
		await page.getByRole('button', { name: 'Add' }).click();
		await expect(page.locator('table').getByText(userId)).toBeVisible({ timeout: 5_000 });

		// Change role via dropdown: find the row, change the select to 'admin'.
		const row = page.locator('tr', { hasText: userId });
		await row.locator('select').selectOption('admin');

		// Reload and verify the role stuck.
		await page.reload();
		await expect(
			page.locator('tr', { hasText: userId }).locator('select'),
		).toHaveValue('admin');

		// Remove.
		await page.locator('tr', { hasText: userId }).getByRole('button', { name: 'Remove' }).click();
		await page.getByRole('button', { name: 'Confirm' }).click();
		await expect(page.locator('table').getByText(userId)).toHaveCount(0);
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
