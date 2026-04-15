import { error } from '@sveltejs/kit';
import { fetchTenantDatabases, fetchTenants } from '$lib/api';
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

	return { tenant, databases };
};
