<bead-review>
  <bead id="axon-4b264f4e" iter=1>
    <title>build(feat-031): impact-matrix proposed-policy delta module (A3/4 of axon-ff92fed7)</title>
    <description>
Subdivision 3 of 4 of axon-ff92fed7. Depends on axon-80979cb8 (A1) only — independent of A2 because this is a pure module + tests, no UI changes.

NEW FILE: ui/src/lib/policy-impact-delta.ts
NEW TEST: ui/src/lib/policy-impact-delta.test.ts (bun test)

Implement and export:

  import type { ImpactCell } from './policy-evaluator';

  export type CellDelta = {
    decisionChanged: boolean;       // active.decision !== proposed.decision
    redactedFieldsChanged: boolean; // sets of strings differ
    deniedFieldsChanged: boolean;   // sets of strings differ
    approvalRoleChanged: boolean;   // active.approvalRole !== proposed.approvalRole
    diagnosticCodeChanged: boolean; // active.diagnostic?.code vs proposed.diagnostic?.code (specifically tracks policy_filter_unindexed remediation)
    isUnchanged: boolean;           // true iff none of the above changed AND both cells exist
    onlyActive: boolean;            // proposed cell missing → schema has no draft, or proposed fetch errored
    onlyProposed: boolean;          // active cell missing (rare, but possible if active fetch errored)
  };

  export function computeCellDelta(
    active: ImpactCell | null,
    proposed: ImpactCell | null,
  ): CellDelta;

Helper: a stable set-equality predicate for redactedFields/deniedFields (treat order as insignificant, deduplicate, case-sensitive). Do NOT mutate inputs.

Unit tests in policy-impact-delta.test.ts must cover:

  1. unchanged: identical cells → isUnchanged=true, all *Changed=false.
  2. decision allow→deny: decisionChanged=true.
  3. decision deny→allow: decisionChanged=true.
  4. decision deny→needs_approval: decisionChanged=true.
  5. redactedFields added: ['amount_cents'] → ['amount_cents', 'commercial_terms'] → redactedFieldsChanged=true.
  6. redactedFields removed: ['amount_cents'] → [] → redactedFieldsChanged=true.
  7. redactedFields reordered (no semantic change): ['a','b'] → ['b','a'] → redactedFieldsChanged=false.
  8. deniedFields added/removed: same pattern as redactedFields.
  9. approvalRole null→'reviewer': approvalRoleChanged=true.
  10. diagnostic code policy_filter_unindexed→null: diagnosticCodeChanged=true (remediation accepted).
  11. diagnostic code null→policy_filter_unindexed: diagnosticCodeChanged=true (remediation regression).
  12. proposed=null, active=non-null: onlyActive=true, isUnchanged=false.
  13. active=null, proposed=non-null: onlyProposed=true.
  14. both null: isUnchanged=false, onlyActive=false, onlyProposed=false (degenerate state).

Run: bun test src/lib/policy-impact-delta.test.ts must be green. The function will be wired into the render block in A4.
    </description>
    <acceptance>
AC1. ui/src/lib/policy-impact-delta.ts exports computeCellDelta and CellDelta type with the shape documented above.

AC2. ui/src/lib/policy-impact-delta.test.ts has at least 14 tests covering the cases enumerated in the description; bun test src/lib/policy-impact-delta.test.ts is green.

AC3. computeCellDelta does not mutate its inputs (verify with frozen-input test).

AC4. Set-equality for redactedFields/deniedFields treats element-order as insignificant (test 7 above proves this).

AC5. bun run typecheck and bun run lint pass.
    </acceptance>
    <labels>helix, feat-031, decomp</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T231744-19b63103/manifest.json</file>
    <file>.ddx/executions/20260503T231744-19b63103/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="ce5be35529d74a61f933322ae4f3e9914cc0a7ef">
diff --git a/.ddx/executions/20260503T231744-19b63103/manifest.json b/.ddx/executions/20260503T231744-19b63103/manifest.json
new file mode 100644
index 0000000..577b7fe
--- /dev/null
+++ b/.ddx/executions/20260503T231744-19b63103/manifest.json
@@ -0,0 +1,38 @@
+{
+  "attempt_id": "20260503T231744-19b63103",
+  "bead_id": "axon-4b264f4e",
+  "base_rev": "01bec416203561b72dcbe59e45b68aa1b549685a",
+  "created_at": "2026-05-03T23:17:45.584376687Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-4b264f4e",
+    "title": "build(feat-031): impact-matrix proposed-policy delta module (A3/4 of axon-ff92fed7)",
+    "description": "Subdivision 3 of 4 of axon-ff92fed7. Depends on axon-80979cb8 (A1) only — independent of A2 because this is a pure module + tests, no UI changes.\n\nNEW FILE: ui/src/lib/policy-impact-delta.ts\nNEW TEST: ui/src/lib/policy-impact-delta.test.ts (bun test)\n\nImplement and export:\n\n  import type { ImpactCell } from './policy-evaluator';\n\n  export type CellDelta = {\n    decisionChanged: boolean;       // active.decision !== proposed.decision\n    redactedFieldsChanged: boolean; // sets of strings differ\n    deniedFieldsChanged: boolean;   // sets of strings differ\n    approvalRoleChanged: boolean;   // active.approvalRole !== proposed.approvalRole\n    diagnosticCodeChanged: boolean; // active.diagnostic?.code vs proposed.diagnostic?.code (specifically tracks policy_filter_unindexed remediation)\n    isUnchanged: boolean;           // true iff none of the above changed AND both cells exist\n    onlyActive: boolean;            // proposed cell missing → schema has no draft, or proposed fetch errored\n    onlyProposed: boolean;          // active cell missing (rare, but possible if active fetch errored)\n  };\n\n  export function computeCellDelta(\n    active: ImpactCell | null,\n    proposed: ImpactCell | null,\n  ): CellDelta;\n\nHelper: a stable set-equality predicate for redactedFields/deniedFields (treat order as insignificant, deduplicate, case-sensitive). Do NOT mutate inputs.\n\nUnit tests in policy-impact-delta.test.ts must cover:\n\n  1. unchanged: identical cells → isUnchanged=true, all *Changed=false.\n  2. decision allow→deny: decisionChanged=true.\n  3. decision deny→allow: decisionChanged=true.\n  4. decision deny→needs_approval: decisionChanged=true.\n  5. redactedFields added: ['amount_cents'] → ['amount_cents', 'commercial_terms'] → redactedFieldsChanged=true.\n  6. redactedFields removed: ['amount_cents'] → [] → redactedFieldsChanged=true.\n  7. redactedFields reordered (no semantic change): ['a','b'] → ['b','a'] → redactedFieldsChanged=false.\n  8. deniedFields added/removed: same pattern as redactedFields.\n  9. approvalRole null→'reviewer': approvalRoleChanged=true.\n  10. diagnostic code policy_filter_unindexed→null: diagnosticCodeChanged=true (remediation accepted).\n  11. diagnostic code null→policy_filter_unindexed: diagnosticCodeChanged=true (remediation regression).\n  12. proposed=null, active=non-null: onlyActive=true, isUnchanged=false.\n  13. active=null, proposed=non-null: onlyProposed=true.\n  14. both null: isUnchanged=false, onlyActive=false, onlyProposed=false (degenerate state).\n\nRun: bun test src/lib/policy-impact-delta.test.ts must be green. The function will be wired into the render block in A4.",
+    "acceptance": "AC1. ui/src/lib/policy-impact-delta.ts exports computeCellDelta and CellDelta type with the shape documented above.\n\nAC2. ui/src/lib/policy-impact-delta.test.ts has at least 14 tests covering the cases enumerated in the description; bun test src/lib/policy-impact-delta.test.ts is green.\n\nAC3. computeCellDelta does not mutate its inputs (verify with frozen-input test).\n\nAC4. Set-equality for redactedFields/deniedFields treats element-order as insignificant (test 7 above proves this).\n\nAC5. bun run typecheck and bun run lint pass.",
+    "parent": "axon-ff92fed7",
+    "labels": [
+      "helix",
+      "feat-031",
+      "decomp"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T23:17:44Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "execute-loop-heartbeat-at": "2026-05-03T23:17:44.837907747Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T231744-19b63103",
+    "prompt": ".ddx/executions/20260503T231744-19b63103/prompt.md",
+    "manifest": ".ddx/executions/20260503T231744-19b63103/manifest.json",
+    "result": ".ddx/executions/20260503T231744-19b63103/result.json",
+    "checks": ".ddx/executions/20260503T231744-19b63103/checks.json",
+    "usage": ".ddx/executions/20260503T231744-19b63103/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-4b264f4e-20260503T231744-19b63103"
+  },
+  "prompt_sha": "1590cc54267b06ea0b9521166f35f9c21364dae80eb715e752538ea5fc68c480"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T231744-19b63103/result.json b/.ddx/executions/20260503T231744-19b63103/result.json
new file mode 100644
index 0000000..0798db9
--- /dev/null
+++ b/.ddx/executions/20260503T231744-19b63103/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-4b264f4e",
+  "attempt_id": "20260503T231744-19b63103",
+  "base_rev": "01bec416203561b72dcbe59e45b68aa1b549685a",
+  "result_rev": "0c2784c372b26ee35a849105846671abf563a86b",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-7ae4cb9a",
+  "duration_ms": 182518,
+  "tokens": 894798,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T231744-19b63103",
+  "prompt_file": ".ddx/executions/20260503T231744-19b63103/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T231744-19b63103/manifest.json",
+  "result_file": ".ddx/executions/20260503T231744-19b63103/result.json",
+  "usage_file": ".ddx/executions/20260503T231744-19b63103/usage.json",
+  "started_at": "2026-05-03T23:17:45.585545863Z",
+  "finished_at": "2026-05-03T23:20:48.103578048Z"
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
