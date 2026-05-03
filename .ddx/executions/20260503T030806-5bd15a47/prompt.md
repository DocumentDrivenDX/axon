<bead-review>
  <bead id="axon-79d1df96" iter=1>
    <title>build(feat-030): audit approval and intent lineage</title>
    <description>
Record preview, approval, rejection, expiry, stale, and committed intent lineage with actor, agent/tool identity when present, policy version, approver reason, intent ID, and pre/post images with FEAT-029 redaction on reads.
    </description>
    <acceptance>
Audit tests prove lineage can be queried by intent ID and entity ID, approvals/rejections include required metadata, and redacted audit reads match entity read redaction.
    </acceptance>
    <labels>helix, phase:build, kind:implementation, area:audit, feat-030</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T024425-64b08244/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="8782a31e4d4434900ecbe5498551f376972ee419">
diff --git a/.ddx/executions/20260503T024425-64b08244/result.json b/.ddx/executions/20260503T024425-64b08244/result.json
new file mode 100644
index 0000000..a005e6b
--- /dev/null
+++ b/.ddx/executions/20260503T024425-64b08244/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-79d1df96",
+  "attempt_id": "20260503T024425-64b08244",
+  "base_rev": "4df979430f61bd4deae5f9fbf79a1ed8859c5bbc",
+  "result_rev": "b18d81f279fd274a83053ec1ec0e8927bf11c025",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-f03f4bf3",
+  "duration_ms": 1405054,
+  "tokens": 15227693,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T024425-64b08244",
+  "prompt_file": ".ddx/executions/20260503T024425-64b08244/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T024425-64b08244/manifest.json",
+  "result_file": ".ddx/executions/20260503T024425-64b08244/result.json",
+  "usage_file": ".ddx/executions/20260503T024425-64b08244/usage.json",
+  "started_at": "2026-05-03T02:44:26.579875194Z",
+  "finished_at": "2026-05-03T03:07:51.634479295Z"
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
