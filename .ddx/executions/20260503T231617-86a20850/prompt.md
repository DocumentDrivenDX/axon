<bead-review>
  <bead id="axon-93fcc7f7" iter=1>
    <title>build(feat-031): impact-matrix proposed-policy column rendering (A2/4 of axon-ff92fed7)</title>
    <description>
Subdivision 2 of 4 of axon-ff92fed7. Depends on axon-80979cb8 (A1: state slots populated). This bead is rendering only — NO delta computation, NO highlighting, just side-by-side cells.

File: ui/src/routes/tenants/[tenant]/databases/[database]/policies/+page.svelte
Render block to modify: lines 1097-1172 (the inner &lt;td&gt; per (entity, subject, operation) cell).

Current structure inside each &lt;td data-testid='policy-impact-matrix-cell'&gt;:
  - &lt;div data-testid='policy-impact-matrix-decision'&gt;{cell.decision}&lt;/div&gt;
  - &lt;div data-testid='policy-impact-matrix-reason'&gt;{cell.reason}&lt;/div&gt;
  - {#if cell.approvalRole} &lt;div data-testid='policy-impact-matrix-approval-role'&gt;...&lt;/div&gt;
  - {#if cell.redactedFields.length} &lt;div data-testid='policy-impact-matrix-redacted-fields'&gt;...&lt;/div&gt;
  - {#if cell.deniedFields.length} &lt;div data-testid='policy-impact-matrix-denied-fields'&gt;...&lt;/div&gt;
  - {#if cell.diagnostic} &lt;div data-testid='policy-impact-matrix-diagnostic'&gt;...&lt;/div&gt;
  - {#if cell.explainHref} &lt;a data-testid='policy-impact-matrix-open-graphql'&gt;...&lt;/a&gt;

Wrap the existing cell content in a sub-section labeled 'active' and add a parallel sub-section labeled 'proposed' that uses proposedImpactMatrix.find(...) (same lookup pattern as line 1101-1106 currently). Required testIds:

  Outer cell: data-testid='policy-impact-matrix-cell' (UNCHANGED — selectors in existing tests still find the cell by entity/subject/operation)
  Active sub-section: data-testid='policy-impact-matrix-cell-active'
  Proposed sub-section: data-testid='policy-impact-matrix-cell-proposed'

Inside each sub-section, mirror the existing decision/reason/approvalRole/redactedFields/deniedFields/diagnostic/explainHref structure — same testIds suffixed with '-active' / '-proposed' (e.g. policy-impact-matrix-decision-active / -proposed). Reason: existing assertions in policy-authoring.spec.ts use unsuffixed testIds (active = current behavior). Add suffixed variants without breaking the existing testIds (which keep pointing at the active sub-section).

When proposedImpactMatrix is empty (no draft access_control on the schema), do NOT render the -proposed sub-section. The cell collapses to active-only and existing tests are unaffected.

OUT OF SCOPE: delta computation between active and proposed (no 'changed' badges, no 'unchanged' label, no transaction-unavailable affordance). Just render both side by side.
    </description>
    <acceptance>
AC1. When the schema has a draft access_control, every cell with both an active and a proposed entry renders two sub-sections: data-testid='policy-impact-matrix-cell-active' and data-testid='policy-impact-matrix-cell-proposed'.

AC2. Each sub-section contains the decision/reason/approvalRole/redactedFields/deniedFields/diagnostic/explainHref children with testIds suffixed by -active or -proposed respectively.

AC3. Existing testIds (policy-impact-matrix-decision, policy-impact-matrix-reason, etc. without suffix) continue to point at the same active-cell content they pointed at before A2 — verified by existing policy-authoring.spec.ts passing without changes.

AC4. When the schema has no draft access_control, no -proposed sub-section is rendered and the cell visually matches the pre-A2 layout.

AC5. bun run typecheck, bun run lint, and bash scripts/test-ui-e2e-docker.sh -- tests/e2e/policy-authoring.spec.ts all pass.
    </acceptance>
    <labels>helix, feat-031, decomp</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T230823-06262984/manifest.json</file>
    <file>.ddx/executions/20260503T230823-06262984/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="c53c39723d5582f5785ac15eb1b1b0b4ca45d16d">
diff --git a/.ddx/executions/20260503T230823-06262984/manifest.json b/.ddx/executions/20260503T230823-06262984/manifest.json
new file mode 100644
index 0000000..e2db595
--- /dev/null
+++ b/.ddx/executions/20260503T230823-06262984/manifest.json
@@ -0,0 +1,38 @@
+{
+  "attempt_id": "20260503T230823-06262984",
+  "bead_id": "axon-93fcc7f7",
+  "base_rev": "3f7c49b06142cf61c9e33f338744ab8df76e72f4",
+  "created_at": "2026-05-03T23:08:23.919963209Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-93fcc7f7",
+    "title": "build(feat-031): impact-matrix proposed-policy column rendering (A2/4 of axon-ff92fed7)",
+    "description": "Subdivision 2 of 4 of axon-ff92fed7. Depends on axon-80979cb8 (A1: state slots populated). This bead is rendering only — NO delta computation, NO highlighting, just side-by-side cells.\n\nFile: ui/src/routes/tenants/[tenant]/databases/[database]/policies/+page.svelte\nRender block to modify: lines 1097-1172 (the inner \u003ctd\u003e per (entity, subject, operation) cell).\n\nCurrent structure inside each \u003ctd data-testid='policy-impact-matrix-cell'\u003e:\n  - \u003cdiv data-testid='policy-impact-matrix-decision'\u003e{cell.decision}\u003c/div\u003e\n  - \u003cdiv data-testid='policy-impact-matrix-reason'\u003e{cell.reason}\u003c/div\u003e\n  - {#if cell.approvalRole} \u003cdiv data-testid='policy-impact-matrix-approval-role'\u003e...\u003c/div\u003e\n  - {#if cell.redactedFields.length} \u003cdiv data-testid='policy-impact-matrix-redacted-fields'\u003e...\u003c/div\u003e\n  - {#if cell.deniedFields.length} \u003cdiv data-testid='policy-impact-matrix-denied-fields'\u003e...\u003c/div\u003e\n  - {#if cell.diagnostic} \u003cdiv data-testid='policy-impact-matrix-diagnostic'\u003e...\u003c/div\u003e\n  - {#if cell.explainHref} \u003ca data-testid='policy-impact-matrix-open-graphql'\u003e...\u003c/a\u003e\n\nWrap the existing cell content in a sub-section labeled 'active' and add a parallel sub-section labeled 'proposed' that uses proposedImpactMatrix.find(...) (same lookup pattern as line 1101-1106 currently). Required testIds:\n\n  Outer cell: data-testid='policy-impact-matrix-cell' (UNCHANGED — selectors in existing tests still find the cell by entity/subject/operation)\n  Active sub-section: data-testid='policy-impact-matrix-cell-active'\n  Proposed sub-section: data-testid='policy-impact-matrix-cell-proposed'\n\nInside each sub-section, mirror the existing decision/reason/approvalRole/redactedFields/deniedFields/diagnostic/explainHref structure — same testIds suffixed with '-active' / '-proposed' (e.g. policy-impact-matrix-decision-active / -proposed). Reason: existing assertions in policy-authoring.spec.ts use unsuffixed testIds (active = current behavior). Add suffixed variants without breaking the existing testIds (which keep pointing at the active sub-section).\n\nWhen proposedImpactMatrix is empty (no draft access_control on the schema), do NOT render the -proposed sub-section. The cell collapses to active-only and existing tests are unaffected.\n\nOUT OF SCOPE: delta computation between active and proposed (no 'changed' badges, no 'unchanged' label, no transaction-unavailable affordance). Just render both side by side.",
+    "acceptance": "AC1. When the schema has a draft access_control, every cell with both an active and a proposed entry renders two sub-sections: data-testid='policy-impact-matrix-cell-active' and data-testid='policy-impact-matrix-cell-proposed'.\n\nAC2. Each sub-section contains the decision/reason/approvalRole/redactedFields/deniedFields/diagnostic/explainHref children with testIds suffixed by -active or -proposed respectively.\n\nAC3. Existing testIds (policy-impact-matrix-decision, policy-impact-matrix-reason, etc. without suffix) continue to point at the same active-cell content they pointed at before A2 — verified by existing policy-authoring.spec.ts passing without changes.\n\nAC4. When the schema has no draft access_control, no -proposed sub-section is rendered and the cell visually matches the pre-A2 layout.\n\nAC5. bun run typecheck, bun run lint, and bash scripts/test-ui-e2e-docker.sh -- tests/e2e/policy-authoring.spec.ts all pass.",
+    "parent": "axon-ff92fed7",
+    "labels": [
+      "helix",
+      "feat-031",
+      "decomp"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T23:08:23Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "execute-loop-heartbeat-at": "2026-05-03T23:08:23.242024785Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T230823-06262984",
+    "prompt": ".ddx/executions/20260503T230823-06262984/prompt.md",
+    "manifest": ".ddx/executions/20260503T230823-06262984/manifest.json",
+    "result": ".ddx/executions/20260503T230823-06262984/result.json",
+    "checks": ".ddx/executions/20260503T230823-06262984/checks.json",
+    "usage": ".ddx/executions/20260503T230823-06262984/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-93fcc7f7-20260503T230823-06262984"
+  },
+  "prompt_sha": "b2a88b8b36b2b459ae4014091672f6884b389ab0eb9159601f430fc4ed0c75cc"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T230823-06262984/result.json b/.ddx/executions/20260503T230823-06262984/result.json
new file mode 100644
index 0000000..d2e94a5
--- /dev/null
+++ b/.ddx/executions/20260503T230823-06262984/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-93fcc7f7",
+  "attempt_id": "20260503T230823-06262984",
+  "base_rev": "3f7c49b06142cf61c9e33f338744ab8df76e72f4",
+  "result_rev": "aa0c0c85f294182ff9b06024bb48722b6f57c0e6",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-e2c115ce",
+  "duration_ms": 470793,
+  "tokens": 1864937,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T230823-06262984",
+  "prompt_file": ".ddx/executions/20260503T230823-06262984/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T230823-06262984/manifest.json",
+  "result_file": ".ddx/executions/20260503T230823-06262984/result.json",
+  "usage_file": ".ddx/executions/20260503T230823-06262984/usage.json",
+  "started_at": "2026-05-03T23:08:23.920978804Z",
+  "finished_at": "2026-05-03T23:16:14.714633839Z"
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
