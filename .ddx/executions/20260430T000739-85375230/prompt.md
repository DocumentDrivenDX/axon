<bead-review>
  <bead id="axon-80979cb8" iter=1>
    <title>build(feat-031): impact-matrix proposed-policy state and op set extension (A1/4 of axon-ff92fed7)</title>
    <description>
Subdivision 1 of 4 of axon-ff92fed7. Lay the data-layer groundwork for the proposed-policy column WITHOUT touching the render block. Two changes:

A. Extend IMPACT_MATRIX_OPERATIONS in ui/src/lib/policy-evaluator.ts:500 from
   ['read', 'patch', 'delete']
   to
   ['read', 'create', 'update', 'patch', 'delete']
   The acceptance criterion of the parent bead requires assertions across all five ops, so the matrix must surface them. The IMPACT_MATRIX_SUBJECT_LIMIT/ENTITY_LIMIT constants stay unchanged.

B. Add proposed-policy state slots adjacent to the existing active-policy state in ui/src/routes/tenants/[tenant]/databases/[database]/policies/+page.svelte (around lines 94-101 for declarations, 337-356 for resets, 358-423 for fetch):

   New $state declarations alongside lines 94-101:
     let proposedImpactMatrix = $state&lt;ImpactCell[]&gt;([]);
     let proposedImpactMatrixError = $state&lt;string | null&gt;(null);
     let loadingProposedImpactMatrix = $state(false);

   Existing ImpactCell shape (do not modify) from ui/src/lib/policy-evaluator.ts:480:
     type ImpactCell = {
       subjectId: string;
       operation: EvaluationOperation;
       entityId: string;
       decision: 'allowed' | 'denied' | 'needs_approval' | 'error';
       reason: string;
       redactedFields: string[];
       deniedFields: string[];
       approvalRole: string | null;
       diagnostic: WorkspaceDiagnostic | null;
       explainHref: string | null;
     };

   Run a SECOND Promise.allSettled in the fetch block (after the existing one at 365 finishes) that mirrors the same explainPolicyDetailed + fetchEffectivePolicy fanout but passes the schema's draft access_control as a policyOverride argument. If api.ts's explainPolicyDetailed/fetchEffectivePolicy do not yet accept a policyOverride, add an optional 4th argument { policyOverride?: AccessControlDraft } that — when set — sends the draft access_control along the GraphQL operation. The GraphQL backend already supports dry-run via the schemaDraft path used by the policy editor on the same page (search 'schema.draft' or 'access_control' in policies/+page.svelte for the existing shape).

   Populate proposedImpactMatrix using the same resolveImpactCell flow with the proposed explain/effective results.

   When the schema has no draft access_control set, leave proposedImpactMatrix = [] and proposedImpactMatrixError = null. The render block (out of scope for this bead) will collapse to active-only in that case.

OUT OF SCOPE for this bead: any UI render changes, any delta computation, any Playwright assertions. Existing policy-authoring.spec.ts must continue to pass unchanged.
    </description>
    <acceptance>
AC1. IMPACT_MATRIX_OPERATIONS in ui/src/lib/policy-evaluator.ts is ['read', 'create', 'update', 'patch', 'delete'].

AC2. proposedImpactMatrix, proposedImpactMatrixError, loadingProposedImpactMatrix state declarations exist in policies/+page.svelte adjacent to the active-policy state.

AC3. After loadImpactMatrix runs, when the schema has a draft access_control, proposedImpactMatrix.length === impactMatrix.length and each proposed cell has a matching (subjectId, entityId, operation) tuple in impactMatrix.

AC4. When the schema has no draft access_control, proposedImpactMatrix === [] and proposedImpactMatrixError === null.

AC5. bun run typecheck and bun run lint pass.

AC6. Existing policy-authoring.spec.ts continues to pass under bash scripts/test-ui-e2e-docker.sh -- tests/e2e/policy-authoring.spec.ts (no new assertions added in this bead).
    </acceptance>
    <notes>
REVIEW:BLOCK

The reviewed revision does not implement the required five-operation matrix, does not complete proposed-policy matrix loading/reset behavior, and introduces build-breaking regressions in the policies page and explain API.

REVIEW:BLOCK

The diff only adds an execution metadata file and does not modify the required UI or API code, so none of the acceptance criteria for the impact-matrix/proposed-policy implementation can be verified as implemented.
    </notes>
    <labels>helix, feat-031, decomp</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260429T235353-d14f6b91/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="529d466dee357cabec4aa2b03d0164e643f730c4">
diff --git a/.ddx/executions/20260429T235353-d14f6b91/result.json b/.ddx/executions/20260429T235353-d14f6b91/result.json
new file mode 100644
index 0000000..35034d1
--- /dev/null
+++ b/.ddx/executions/20260429T235353-d14f6b91/result.json
@@ -0,0 +1,23 @@
+{
+  "bead_id": "axon-80979cb8",
+  "attempt_id": "20260429T235353-d14f6b91",
+  "base_rev": "cf47ca378832c81f7bbe6fcfb04996ce5e4bad7a",
+  "result_rev": "32fa5c8dfe2099bc7aca28ec626230cc2e782c4b",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "claude",
+  "model": "sonnet",
+  "session_id": "eb-b522c143",
+  "duration_ms": 817588,
+  "tokens": 44256,
+  "cost_usd": 3.607105649999999,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260429T235353-d14f6b91",
+  "prompt_file": ".ddx/executions/20260429T235353-d14f6b91/prompt.md",
+  "manifest_file": ".ddx/executions/20260429T235353-d14f6b91/manifest.json",
+  "result_file": ".ddx/executions/20260429T235353-d14f6b91/result.json",
+  "usage_file": ".ddx/executions/20260429T235353-d14f6b91/usage.json",
+  "started_at": "2026-04-29T23:53:55.899788399Z",
+  "finished_at": "2026-04-30T00:07:33.488134089Z"
+}
\ No newline at end of file
  </diff>

  <instructions>
You are reviewing a bead implementation against its acceptance criteria.

For each acceptance-criteria (AC) item, decide whether it is implemented correctly, then assign one overall verdict:

- APPROVE — every AC item is fully and correctly implemented.
- REQUEST_CHANGES — some AC items are partial or have fixable minor issues.
- BLOCK — at least one AC item is not implemented or incorrectly implemented; or the diff is insufficient to evaluate.

## Required output format (schema_version: 1)

Respond with EXACTLY one JSON object as your final response, fenced as a single ```json … ``` code block. Do not include any prose outside the fenced block. The JSON must match this schema:

```json
{
  "schema_version": 1,
  "verdict": "APPROVE",
  "summary": "≤300 char human-readable verdict justification",
  "findings": [
    { "severity": "info", "summary": "what is wrong or notable", "location": "path/to/file.go:42" }
  ]
}
```

Rules:
- "verdict" must be exactly one of "APPROVE", "REQUEST_CHANGES", "BLOCK".
- "severity" must be exactly one of "info", "warn", "block".
- Output the JSON object inside ONE fenced ```json … ``` block. No additional prose, no extra fences, no markdown headings.
- Do not echo this template back. Do not write the words APPROVE, REQUEST_CHANGES, or BLOCK anywhere except as the JSON value of the verdict field.
  </instructions>
</bead-review>
