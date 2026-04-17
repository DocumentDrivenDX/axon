import { fetchTenantDatabases, fetchTenantMembers, fetchTenants, listCredentials } from '$lib/api';
import { error } from '@sveltejs/kit';
import type { LayoutLoad } from './$types';

export const load: LayoutLoad = async ({ params }) => {
	const tenants = await fetchTenants();
	const tenant = tenants.find((t) => t.db_name === params.tenant);
	if (!tenant) {
		error(404, `Tenant "${params.tenant}" not found`);
	}

	// Databases are needed for both the tenant overview and the child database
	// layout, so load them once here and cache them on the layout data.
	const databases = await fetchTenantDatabases(tenant.id);

	// Fetch member and credential counts for the tenant home page.
	let membersCount = 0;
	try {
		const members = await fetchTenantMembers(tenant.id);
		membersCount = members.length;
	} catch {
		// If the member endpoint is not available, default to 0.
	}

	let credentialsCount = 0;
	try {
		const credentials = await listCredentials(tenant.id);
		credentialsCount = credentials.length;
	} catch {
		// If the credential endpoint is not available, default to 0.
	}

	return { tenant, databases, membersCount, credentialsCount };
};
