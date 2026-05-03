import type { ImpactCell } from './policy-evaluator';

export type CellDelta = {
	decisionChanged: boolean;
	redactedFieldsChanged: boolean;
	deniedFieldsChanged: boolean;
	approvalRoleChanged: boolean;
	diagnosticCodeChanged: boolean;
	isUnchanged: boolean;
	onlyActive: boolean;
	onlyProposed: boolean;
};

function sameStringSet(active: string[], proposed: string[]): boolean {
	const activeSet = new Set(active);
	const proposedSet = new Set(proposed);
	if (activeSet.size !== proposedSet.size) return false;
	for (const field of activeSet) {
		if (!proposedSet.has(field)) return false;
	}
	return true;
}

export function computeCellDelta(
	active: ImpactCell | null,
	proposed: ImpactCell | null,
): CellDelta {
	const onlyActive = Boolean(active && !proposed);
	const onlyProposed = Boolean(!active && proposed);
	const emptyDelta = {
		decisionChanged: false,
		redactedFieldsChanged: false,
		deniedFieldsChanged: false,
		approvalRoleChanged: false,
		diagnosticCodeChanged: false,
		isUnchanged: false,
		onlyActive,
		onlyProposed,
	};

	if (!active || !proposed) return emptyDelta;

	const decisionChanged = active.decision !== proposed.decision;
	const redactedFieldsChanged = !sameStringSet(active.redactedFields, proposed.redactedFields);
	const deniedFieldsChanged = !sameStringSet(active.deniedFields, proposed.deniedFields);
	const approvalRoleChanged = active.approvalRole !== proposed.approvalRole;
	const diagnosticCodeChanged = active.diagnostic?.code !== proposed.diagnostic?.code;

	return {
		decisionChanged,
		redactedFieldsChanged,
		deniedFieldsChanged,
		approvalRoleChanged,
		diagnosticCodeChanged,
		isUnchanged:
			!decisionChanged &&
			!redactedFieldsChanged &&
			!deniedFieldsChanged &&
			!approvalRoleChanged &&
			!diagnosticCodeChanged,
		onlyActive: false,
		onlyProposed: false,
	};
}
