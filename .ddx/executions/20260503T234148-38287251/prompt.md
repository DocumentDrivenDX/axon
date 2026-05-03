<bead-review>
  <bead id="axon-1fb17586" iter=1>
    <title>build(feat-031): impact-matrix delta highlighting and Playwright assertions (A4/4 of axon-ff92fed7)</title>
    <description>
Subdivision 4 of 4 of axon-ff92fed7. Depends on axon-80979cb8 (A1: state), axon-93fcc7f7 (A2: side-by-side render), axon-4b264f4e (A3: computeCellDelta module). This bead wires delta visualization into the cell render and adds the Playwright assertions that close the parent bead's AC.

PART 1 — render: ui/src/routes/tenants/[tenant]/databases/[database]/policies/+page.svelte

Inside each cell wrapper from A2 (between the existing -active sub-section and the -proposed sub-section), add a delta-summary div driven by computeCellDelta(active, proposed):

  Imports at the top of the &lt;script&gt;:
    import { computeCellDelta, type CellDelta } from '$lib/policy-impact-delta';

  Inside the {#each ... operations} loop, after the {@const cell = ...} line (around line 1101-1106), add:
    {@const proposedCell = proposedImpactMatrix.find((c) =&gt; c.entityId === entity.id &amp;&amp; c.subjectId === subject.id &amp;&amp; c.operation === operation) ?? null}
    {@const delta = computeCellDelta(cell ?? null, proposedCell)}

  Render rules inside the cell (between active and proposed sub-sections):
    - When delta.isUnchanged → show &lt;span data-testid='policy-impact-matrix-cell-unchanged'&gt;unchanged&lt;/span&gt; (suppress the -proposed sub-section, since it would duplicate active).
    - When delta.onlyActive (no proposed entry) → omit the -proposed sub-section, no delta marker.
    - When any *Changed flag is true → render a delta-summary div: &lt;div data-testid='policy-impact-matrix-cell-delta' data-decision-changed={delta.decisionChanged} data-redacted-changed={delta.redactedFieldsChanged} data-denied-changed={delta.deniedFieldsChanged} data-approval-changed={delta.approvalRoleChanged} data-diagnostic-changed={delta.diagnosticCodeChanged}&gt;changed&lt;/div&gt;
    - When entity.collection is the transactions table (out of scope for this bead per parent ff92fed7's notes) → render a 'transaction delta unavailable' affordance: &lt;div data-testid='policy-impact-matrix-cell-transaction-unavailable'&gt;transaction delta unavailable&lt;a href='#follow-up-bead'&gt;follow-up&lt;/a&gt;&lt;/div&gt;. The follow-up bead reference is informational only — the parent ff92fed7 description says transaction-row support is queued as a separate bead (does not need to exist yet).

PART 2 — Playwright spec: ui/tests/e2e/policy-authoring.spec.ts

Add a new test in the existing 'Policy authoring (impact matrix)' describe block titled 'surfaces active-vs-proposed deltas across read|create|update|patch|delete fixture rows'. Use the existing seedScn017PolicyUiFixture helper (which already supplies proposed-policy drafts via proposedPolicyDraftDenyHigh from helpers.ts). Walk through:

  1. Set the schema's draft access_control to proposedPolicyDraftDenyHigh (UI flow already covered by the existing 'editor accepts a proposed policy' test, reuse it).
  2. Render the impact matrix.
  3. For each operation in [read, create, update, patch, delete] (the 5 ops added in A1) and each fixture subject:
     - If decision changes vs active → expect data-testid='policy-impact-matrix-cell-delta' with data-decision-changed='true'.
     - If redactedFields differ → expect data-redacted-changed='true'.
     - If active and proposed are byte-identical → expect data-testid='policy-impact-matrix-cell-unchanged' visible.
  4. For at least one fixture row, set proposed to a state that introduces a policy_filter_unindexed diagnostic; assert data-diagnostic-changed='true'.
  5. Confirm the (existing) 'transactions' collection row, if rendered, shows data-testid='policy-impact-matrix-cell-transaction-unavailable'.

Use seeds proposedPolicyDraftBroken (introduces unindexed diag) and proposedPolicyDraftDenyHigh (denies high-amount invoices) — both already in helpers.ts.
    </description>
    <acceptance>
AC1. Each impact-matrix cell renders one of: data-testid='policy-impact-matrix-cell-unchanged' (when delta.isUnchanged), data-testid='policy-impact-matrix-cell-delta' with the appropriate data-*-changed booleans (when any flag is true), data-testid='policy-impact-matrix-cell-transaction-unavailable' (for transaction-collection rows), or just the active sub-section (when no proposed exists).

AC2. ui/tests/e2e/policy-authoring.spec.ts has a new test that uses proposedPolicyDraftDenyHigh and proposedPolicyDraftBroken to assert the matrix surfaces decision changes, redacted/denied field deltas, and policy_filter_unindexed remediation between active and proposed for read|create|update|patch|delete fixture rows. Cells where decision is unchanged render 'unchanged' literally. Transaction-row delta cells show the transaction-delta-unavailable affordance.

AC3. bash scripts/test-ui-e2e-docker.sh -- tests/e2e/policy-authoring.spec.ts is green; capture the exact output (passed/skipped/failed counts) in the bead notes when closing.

AC4. bun run typecheck, bun run lint, and bun test src/lib/policy-impact-delta.test.ts (from A3) all pass.

AC5. After this bead closes, the parent ff92fed7 acceptance is fully satisfied. Closing this bead may close ff92fed7 once the dependency chain reports green.
    </acceptance>
    <notes>
Verification evidence (commit 62aad0b):
- bash scripts/test-ui-e2e-docker.sh -- tests/e2e/policy-authoring.spec.ts: 7 passed (17.0s), 0 failed, 0 skipped.
- bun run typecheck: svelte-check found 0 errors and 0 warnings.
- bun run lint: Checked 50 files; no fixes applied.
- bun test src/lib/policy-impact-delta.test.ts: 16 pass, 0 fail, 31 expect() calls.
- cargo fmt --check: pass.
- cargo check: pass.
- cargo test: pass.
- cargo clippy -- -D warnings: pass.
    </notes>
    <labels>helix, feat-031, decomp</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T232153-80212686/manifest.json</file>
    <file>.ddx/executions/20260503T232153-80212686/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="4b499ddbabc3e75a48284c40d43ac5bc9ba6780b">
diff --git a/.ddx/executions/20260503T232153-80212686/manifest.json b/.ddx/executions/20260503T232153-80212686/manifest.json
new file mode 100644
index 0000000..89dcc1e
--- /dev/null
+++ b/.ddx/executions/20260503T232153-80212686/manifest.json
@@ -0,0 +1,38 @@
+{
+  "attempt_id": "20260503T232153-80212686",
+  "bead_id": "axon-1fb17586",
+  "base_rev": "e3e171e8b86084554e8b2815dba670547baa1d47",
+  "created_at": "2026-05-03T23:21:54.165447234Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-1fb17586",
+    "title": "build(feat-031): impact-matrix delta highlighting and Playwright assertions (A4/4 of axon-ff92fed7)",
+    "description": "Subdivision 4 of 4 of axon-ff92fed7. Depends on axon-80979cb8 (A1: state), axon-93fcc7f7 (A2: side-by-side render), axon-4b264f4e (A3: computeCellDelta module). This bead wires delta visualization into the cell render and adds the Playwright assertions that close the parent bead's AC.\n\nPART 1 — render: ui/src/routes/tenants/[tenant]/databases/[database]/policies/+page.svelte\n\nInside each cell wrapper from A2 (between the existing -active sub-section and the -proposed sub-section), add a delta-summary div driven by computeCellDelta(active, proposed):\n\n  Imports at the top of the \u003cscript\u003e:\n    import { computeCellDelta, type CellDelta } from '$lib/policy-impact-delta';\n\n  Inside the {#each ... operations} loop, after the {@const cell = ...} line (around line 1101-1106), add:\n    {@const proposedCell = proposedImpactMatrix.find((c) =\u003e c.entityId === entity.id \u0026\u0026 c.subjectId === subject.id \u0026\u0026 c.operation === operation) ?? null}\n    {@const delta = computeCellDelta(cell ?? null, proposedCell)}\n\n  Render rules inside the cell (between active and proposed sub-sections):\n    - When delta.isUnchanged → show \u003cspan data-testid='policy-impact-matrix-cell-unchanged'\u003eunchanged\u003c/span\u003e (suppress the -proposed sub-section, since it would duplicate active).\n    - When delta.onlyActive (no proposed entry) → omit the -proposed sub-section, no delta marker.\n    - When any *Changed flag is true → render a delta-summary div: \u003cdiv data-testid='policy-impact-matrix-cell-delta' data-decision-changed={delta.decisionChanged} data-redacted-changed={delta.redactedFieldsChanged} data-denied-changed={delta.deniedFieldsChanged} data-approval-changed={delta.approvalRoleChanged} data-diagnostic-changed={delta.diagnosticCodeChanged}\u003echanged\u003c/div\u003e\n    - When entity.collection is the transactions table (out of scope for this bead per parent ff92fed7's notes) → render a 'transaction delta unavailable' affordance: \u003cdiv data-testid='policy-impact-matrix-cell-transaction-unavailable'\u003etransaction delta unavailable\u003ca href='#follow-up-bead'\u003efollow-up\u003c/a\u003e\u003c/div\u003e. The follow-up bead reference is informational only — the parent ff92fed7 description says transaction-row support is queued as a separate bead (does not need to exist yet).\n\nPART 2 — Playwright spec: ui/tests/e2e/policy-authoring.spec.ts\n\nAdd a new test in the existing 'Policy authoring (impact matrix)' describe block titled 'surfaces active-vs-proposed deltas across read|create|update|patch|delete fixture rows'. Use the existing seedScn017PolicyUiFixture helper (which already supplies proposed-policy drafts via proposedPolicyDraftDenyHigh from helpers.ts). Walk through:\n\n  1. Set the schema's draft access_control to proposedPolicyDraftDenyHigh (UI flow already covered by the existing 'editor accepts a proposed policy' test, reuse it).\n  2. Render the impact matrix.\n  3. For each operation in [read, create, update, patch, delete] (the 5 ops added in A1) and each fixture subject:\n     - If decision changes vs active → expect data-testid='policy-impact-matrix-cell-delta' with data-decision-changed='true'.\n     - If redactedFields differ → expect data-redacted-changed='true'.\n     - If active and proposed are byte-identical → expect data-testid='policy-impact-matrix-cell-unchanged' visible.\n  4. For at least one fixture row, set proposed to a state that introduces a policy_filter_unindexed diagnostic; assert data-diagnostic-changed='true'.\n  5. Confirm the (existing) 'transactions' collection row, if rendered, shows data-testid='policy-impact-matrix-cell-transaction-unavailable'.\n\nUse seeds proposedPolicyDraftBroken (introduces unindexed diag) and proposedPolicyDraftDenyHigh (denies high-amount invoices) — both already in helpers.ts.",
+    "acceptance": "AC1. Each impact-matrix cell renders one of: data-testid='policy-impact-matrix-cell-unchanged' (when delta.isUnchanged), data-testid='policy-impact-matrix-cell-delta' with the appropriate data-*-changed booleans (when any flag is true), data-testid='policy-impact-matrix-cell-transaction-unavailable' (for transaction-collection rows), or just the active sub-section (when no proposed exists).\n\nAC2. ui/tests/e2e/policy-authoring.spec.ts has a new test that uses proposedPolicyDraftDenyHigh and proposedPolicyDraftBroken to assert the matrix surfaces decision changes, redacted/denied field deltas, and policy_filter_unindexed remediation between active and proposed for read|create|update|patch|delete fixture rows. Cells where decision is unchanged render 'unchanged' literally. Transaction-row delta cells show the transaction-delta-unavailable affordance.\n\nAC3. bash scripts/test-ui-e2e-docker.sh -- tests/e2e/policy-authoring.spec.ts is green; capture the exact output (passed/skipped/failed counts) in the bead notes when closing.\n\nAC4. bun run typecheck, bun run lint, and bun test src/lib/policy-impact-delta.test.ts (from A3) all pass.\n\nAC5. After this bead closes, the parent ff92fed7 acceptance is fully satisfied. Closing this bead may close ff92fed7 once the dependency chain reports green.",
+    "parent": "axon-ff92fed7",
+    "labels": [
+      "helix",
+      "feat-031",
+      "decomp"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T23:21:53Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "execute-loop-heartbeat-at": "2026-05-03T23:21:53.300815924Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T232153-80212686",
+    "prompt": ".ddx/executions/20260503T232153-80212686/prompt.md",
+    "manifest": ".ddx/executions/20260503T232153-80212686/manifest.json",
+    "result": ".ddx/executions/20260503T232153-80212686/result.json",
+    "checks": ".ddx/executions/20260503T232153-80212686/checks.json",
+    "usage": ".ddx/executions/20260503T232153-80212686/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-1fb17586-20260503T232153-80212686"
+  },
+  "prompt_sha": "cfedbcfa61ff16df43d924f7ce1db46caf48b61330a3f1d0ca4ee015e5e86efd"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T232153-80212686/result.json b/.ddx/executions/20260503T232153-80212686/result.json
new file mode 100644
index 0000000..0c306eb
--- /dev/null
+++ b/.ddx/executions/20260503T232153-80212686/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-1fb17586",
+  "attempt_id": "20260503T232153-80212686",
+  "base_rev": "e3e171e8b86084554e8b2815dba670547baa1d47",
+  "result_rev": "62aad0b05e4eabc553240e19feca973dccb07412",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-769b3ef8",
+  "duration_ms": 1190297,
+  "tokens": 8248423,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T232153-80212686",
+  "prompt_file": ".ddx/executions/20260503T232153-80212686/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T232153-80212686/manifest.json",
+  "result_file": ".ddx/executions/20260503T232153-80212686/result.json",
+  "usage_file": ".ddx/executions/20260503T232153-80212686/usage.json",
+  "started_at": "2026-05-03T23:21:54.166796101Z",
+  "finished_at": "2026-05-03T23:41:44.464721601Z"
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
