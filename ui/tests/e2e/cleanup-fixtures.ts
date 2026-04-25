import { request as playwrightRequest } from '@playwright/test';
import { E2E_FIXTURE_PREFIX } from './helpers';

const baseURL = process.env.AXON_E2E_BASE_URL ?? 'http://localhost:4170';

type Tenant = {
	id: string;
	name?: string;
	db_name?: string;
	dbName?: string;
};

type User = {
	id: string;
	display_name?: string;
	displayName?: string;
};

function tenantMatches(tenant: Tenant): boolean {
	const values = [tenant.name, tenant.db_name, tenant.dbName].filter(Boolean) as string[];
	return values.some((value) => value.startsWith(E2E_FIXTURE_PREFIX));
}

function userMatches(user: User): boolean {
	const displayName = user.display_name ?? user.displayName ?? '';
	return displayName.startsWith(E2E_FIXTURE_PREFIX);
}

async function cleanupFixtures() {
	const context = await playwrightRequest.newContext({ baseURL, ignoreHTTPSErrors: true });
	try {
		const tenantsResponse = await context.get('/control/tenants');
		if (tenantsResponse.ok()) {
			const body = await tenantsResponse.json();
			const tenants = (body.tenants ?? []) as Tenant[];
			for (const tenant of tenants.filter(tenantMatches)) {
				const response = await context.delete(`/control/tenants/${encodeURIComponent(tenant.id)}`);
				if (!response.ok()) {
					throw new Error(
						`failed to delete E2E tenant ${tenant.id}: ${response.status()} ${await response.text()}`,
					);
				}
			}
		}

		const usersResponse = await context.get('/control/users/list');
		if (usersResponse.ok()) {
			const body = await usersResponse.json();
			const users = (body.users ?? []) as User[];
			for (const user of users.filter(userMatches)) {
				const response = await context.delete(
					`/control/users/suspend/${encodeURIComponent(user.id)}`,
				);
				if (!response.ok()) {
					throw new Error(
						`failed to suspend E2E user ${user.id}: ${response.status()} ${await response.text()}`,
					);
				}
			}
		}

		const verifyResponse = await context.get('/control/tenants');
		if (verifyResponse.ok()) {
			const body = await verifyResponse.json();
			const leftovers = ((body.tenants ?? []) as Tenant[]).filter(tenantMatches);
			if (leftovers.length > 0) {
				throw new Error(
					`E2E tenant cleanup left ${leftovers.length} tenant(s): ${leftovers
						.map((tenant) => `${tenant.id}:${tenant.name ?? tenant.db_name ?? tenant.dbName ?? ''}`)
						.join(', ')}`,
				);
			}
		}
	} finally {
		await context.dispose();
	}
}

export default cleanupFixtures;
