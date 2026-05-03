# axon-80979cb8 Verification

The scoped FEAT-031 impact-matrix data-layer changes are present in the worktree:

- `ui/src/lib/policy-evaluator.ts:500` defines `IMPACT_MATRIX_OPERATIONS` as `['read', 'create', 'update', 'patch', 'delete']`.
- `ui/src/routes/tenants/[tenant]/databases/[database]/policies/+page.svelte:93` declares active matrix state, followed immediately by `proposedImpactMatrix`, `proposedImpactMatrixError`, and `loadingProposedImpactMatrix` at lines 98-100.
- `loadImpactMatrix` resets proposed state to `[]` and `null` when no draft policy is available at lines 434-440.
- When a draft `access_control` exists, `loadImpactMatrix` reuses the same request and effective-policy key sets, passes `policyOverride: schemaDraft` to `explainPolicyDetailed` and `fetchEffectivePolicy`, and maps the same `requests` array into `proposedImpactMatrix` at lines 446-480. This preserves the same `(subjectId, entityId, operation)` tuple set as `impactMatrix`.
- `ui/src/lib/api.ts:1505` and `ui/src/lib/api.ts:1554` accept `policyOverride?: AccessControlDraft` and send it as the GraphQL `policyOverride` variable.

Verification commands run:

- `cd ui && bun install --frozen-lockfile`
- `cd ui && bun run typecheck`
- `cd ui && bun run lint`
- `cargo check`
- `bash scripts/test-ui-e2e-docker.sh -- tests/e2e/policy-authoring.spec.ts`
- `cargo fmt --check`
- `cargo test`
- `cargo clippy -- -D warnings`

All verification commands passed. The e2e command reported 6 passed tests in `tests/e2e/policy-authoring.spec.ts`.
