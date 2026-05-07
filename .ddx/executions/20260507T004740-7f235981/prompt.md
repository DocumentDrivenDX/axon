<bead-review>
  <bead id="axon-07430e8f" iter=1>
    <title>build(feat-031): /schemas activation gate behind matrix dry-run</title>
    <description>
Wire the schemas-tab Activate button (already gated on report.errors via axon-c3acde1f) to also require a successful proposed-policy delta-matrix dry-run from axon-ff92fed7. The gate keys on the access_control content hash so editing the policy invalidates the gate. The hash is computed CLIENT-SIDE on the proposed access_control JSON canonical form (sort keys, strip whitespace, sha256 first 16 hex chars) — the server is not extended for this bead. Surface the matrix dry-run as a deep-link from the Activate button so operators can run it inline.
    </description>
    <acceptance>
policy-authoring.spec.ts proves activation is blocked until a matrix dry-run for the current proposed access_control hash is recorded; activation succeeds when the dry-run is recorded; matrix dry-run results are tied to the hash so editing the policy text invalidates the gate. The client-side hashing helper is unit-tested via bun test src/.
    </acceptance>
    <labels>helix, phase:build, kind:implementation, area:ui, feat-031, component:policy-authoring, route:schemas</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260507T002932-4b5b9710/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="a85dd0ad9bf823ba269ea16d1ed6d4f481e20b81">
<untrusted-data>
diff --git a/.ddx/executions/20260507T002932-4b5b9710/result.json b/.ddx/executions/20260507T002932-4b5b9710/result.json
new file mode 100644
index 0000000..13b830a
--- /dev/null
+++ b/.ddx/executions/20260507T002932-4b5b9710/result.json
@@ -0,0 +1,23 @@
+{
+  "bead_id": "axon-07430e8f",
+  "attempt_id": "20260507T002932-4b5b9710",
+  "base_rev": "6b830b0ad74016f776d15f781fbd099fa05d8c6e",
+  "result_rev": "026b70b1a2cf00381758fd33b63fa464dc662e8d",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "claude",
+  "model": "sonnet",
+  "session_id": "eb-9b979272",
+  "duration_ms": 1045769,
+  "tokens": 52654,
+  "cost_usd": 3.0429930499999998,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260507T002932-4b5b9710",
+  "prompt_file": ".ddx/executions/20260507T002932-4b5b9710/prompt.md",
+  "manifest_file": ".ddx/executions/20260507T002932-4b5b9710/manifest.json",
+  "result_file": ".ddx/executions/20260507T002932-4b5b9710/result.json",
+  "usage_file": ".ddx/executions/20260507T002932-4b5b9710/usage.json",
+  "started_at": "2026-05-07T00:29:32.956855285Z",
+  "finished_at": "2026-05-07T00:46:58.726724203Z"
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
