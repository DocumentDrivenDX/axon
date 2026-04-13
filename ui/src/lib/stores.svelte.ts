/**
 * Shared reactive state for cross-component coordination.
 *
 * Uses Svelte 5 runes (`$state`) at module scope via `.svelte.ts` files.
 * Components import the getter/setter pair rather than the raw rune, which
 * keeps the mutable variable local to this module.
 */

import type { Tenant } from './api';

// ---------------------------------------------------------------------------
// Selected tenant
// ---------------------------------------------------------------------------

let selectedTenant: Tenant | null = $state(null);

/** Return the currently-selected tenant (or null if none is selected). */
export function getSelectedTenant(): Tenant | null {
	return selectedTenant;
}

/** Set the active tenant.  Pass `null` to deselect. */
export function setSelectedTenant(tenant: Tenant | null): void {
	selectedTenant = tenant;
}
