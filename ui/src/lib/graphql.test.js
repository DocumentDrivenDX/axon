import { expect, test } from 'bun:test';

import { fetchCollections, fetchEntities, fetchEntity, gqlQuery } from './graphql.js';

test('graphql client exports the UI query helpers', () => {
	expect(typeof gqlQuery).toBe('function');
	expect(typeof fetchCollections).toBe('function');
	expect(typeof fetchEntities).toBe('function');
	expect(typeof fetchEntity).toBe('function');
});
