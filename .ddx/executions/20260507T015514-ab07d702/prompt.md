<bead-review>
  <bead id="axon-53c8d772" iter=1>
    <title>build(feat-009): Subscriptions on schema-declared named queries</title>
    <description>
Per FEAT-009 US-077. Each named query supports GraphQL subscription:

type Subscription { ready_beads: DdxBeadConnection! }

Re-evaluate the named query when the change-feed pipeline emits a relevant change (entity create/update/delete or link create/delete affecting the named query's collections/labels). Initial snapshot delivered on subscribe; clean teardown on disconnect.

Ad-hoc subscriptions NOT in scope (deferred per ADR-021).

Out of scope for V1: incremental query maintenance — full re-evaluation acceptable if it meets latency budgets.
    </description>
    <acceptance>
AC1. Each named query exposes a Subscription field. AC2. Initial snapshot delivered on subscribe. AC3. Updates policy-filtered per FEAT-029. AC4. Clean teardown on client disconnect; no leaked watchers. AC5. Tests: initial snapshot, entity-add update, status-change update, link-add update, policy-filter behavior, disconnect cleanup.
    </acceptance>
    <labels>helix, feat-009, feat-015, area:graphql, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260507T011143-b7141153/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="e2f6c28bc4f2a8900d2d9570f72130ea7601bde9">
<untrusted-data>
diff --git a/.ddx/executions/20260507T011143-b7141153/result.json b/.ddx/executions/20260507T011143-b7141153/result.json
new file mode 100644
index 0000000..031faf9
--- /dev/null
+++ b/.ddx/executions/20260507T011143-b7141153/result.json
@@ -0,0 +1,23 @@
+{
+  "bead_id": "axon-53c8d772",
+  "attempt_id": "20260507T011143-b7141153",
+  "base_rev": "97a05cc5e083791cff972c448ea1d2d41a70f104",
+  "result_rev": "fc729395bdb2ff66d0b7aba9afc2d5d270a3cf14",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "claude",
+  "model": "sonnet",
+  "session_id": "eb-54ec1322",
+  "duration_ms": 2596722,
+  "tokens": 49060,
+  "cost_usd": 4.090606649999999,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260507T011143-b7141153",
+  "prompt_file": ".ddx/executions/20260507T011143-b7141153/prompt.md",
+  "manifest_file": ".ddx/executions/20260507T011143-b7141153/manifest.json",
+  "result_file": ".ddx/executions/20260507T011143-b7141153/result.json",
+  "usage_file": ".ddx/executions/20260507T011143-b7141153/usage.json",
+  "started_at": "2026-05-07T01:11:44.558309353Z",
+  "finished_at": "2026-05-07T01:55:01.281198206Z"
+}
\ No newline at end of file
</untrusted-data>
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
