<bead-review>
  <bead id="axon-84088cbe" iter=1>
    <title>build(feat-015): JSON-LD content negotiation for GraphQL entity payloads</title>
    <description>
Per FEAT-015 US-078 and ADR-020 §RDF concept adoption.

Add Accept: application/ld+json content-negotiation path to GraphQL responses. Generated @context derived from active ESF schema; @id set to canonical entity URL; @type derived from collection. Linked entities render as nested @id-bearing nodes.

Field-name collisions with JSON-LD reserved keywords (@id, @type, @graph, @context) are remapped via @context aliases; FEAT-002 schema validator emits warning at schema-write time when collision detected.

Default Accept: application/json behavior unchanged (no perf regression).
    </description>
    <acceptance>
AC1. Accept: application/ld+json returns JSON-LD body with @context, @id, @type. AC2. Default Accept returns plain JSON unchanged. AC3. @context generated from ESF schema; reserved-keyword collisions remapped. AC4. Validates against jsonld.js or pyld. AC5. cargo test passes; clippy clean.
    </acceptance>
    <notes>
REVIEW:BLOCK

Diff under review contains only execution metadata (manifest.json, result.json) — no code changes for JSON-LD content negotiation. None of AC1–AC5 can be evaluated against the provided diff.
    </notes>
    <labels>helix, feat-015, area:graphql, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260507T015135-5b3d716e/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="e700f3b35698a0acccc039bf621f4116a9bbc817">
<untrusted-data>
diff --git a/.ddx/executions/20260507T015135-5b3d716e/result.json b/.ddx/executions/20260507T015135-5b3d716e/result.json
new file mode 100644
index 0000000..6a652b3
--- /dev/null
+++ b/.ddx/executions/20260507T015135-5b3d716e/result.json
@@ -0,0 +1,23 @@
+{
+  "bead_id": "axon-84088cbe",
+  "attempt_id": "20260507T015135-5b3d716e",
+  "base_rev": "7913c1c47bc4f0ef70fc74b13984c47d5507f596",
+  "result_rev": "0dc6d4efa895e305fde5ef0334b30731032eb11d",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "claude",
+  "model": "sonnet",
+  "session_id": "eb-3ce2223c",
+  "duration_ms": 856815,
+  "tokens": 15358,
+  "cost_usd": 1.7951456499999998,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260507T015135-5b3d716e",
+  "prompt_file": ".ddx/executions/20260507T015135-5b3d716e/prompt.md",
+  "manifest_file": ".ddx/executions/20260507T015135-5b3d716e/manifest.json",
+  "result_file": ".ddx/executions/20260507T015135-5b3d716e/result.json",
+  "usage_file": ".ddx/executions/20260507T015135-5b3d716e/usage.json",
+  "started_at": "2026-05-07T01:51:36.148462355Z",
+  "finished_at": "2026-05-07T02:05:52.964256018Z"
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
