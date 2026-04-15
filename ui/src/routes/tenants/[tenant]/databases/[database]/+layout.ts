import { error } from '@sveltejs/kit';
import type { LayoutLoad } from './$types';

export const load: LayoutLoad = async ({ params, parent }) => {
	const parentData = await parent();
	const database = parentData.databases.find((d) => d.name === params.database);
	if (!database) {
		error(404, `Database "${params.database}" not found in tenant "${params.tenant}"`);
	}
	return {
		database,
		scope: { tenant: params.tenant, database: params.database },
	};
};
