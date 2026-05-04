<bead-review>
  <bead id="axon-2705d3ac" iter=1>
    <title>build(feat-009): MCP tools — axon.query and per-named-query</title>
    <description>
Per ADR-021 §MCP surfacing and FEAT-009 US-073/US-076. MCP exposes:

- axon.query(cypher, parameters) — generic ad-hoc query tool, mirrors GraphQL axonQuery.
- One tool per named query, named after the query (e.g. ddx_beads.ready_beads). Same parameter shape as the GraphQL field.

Tool descriptions surface the named query's documentation string. Both tools enforce identical policy and limits as the GraphQL surface.
    </description>
    <acceptance>
AC1. axon.query MCP tool with cypher + parameters. AC2. Each named query in the active schema generates a corresponding MCP tool. AC3. Tool descriptions include the named query's description: field. AC4. Tools enforce same policy/limits as GraphQL surface. AC5. cargo test -p axon-mcp passes; clippy clean.
    </acceptance>
    <labels>helix, feat-009, feat-016, area:mcp, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260504T004917-537d20b6/manifest.json</file>
    <file>.ddx/executions/20260504T004917-537d20b6/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="e08e0309590f1cedc0704a64865e8d6cf299d436">
diff --git a/.ddx/executions/20260504T004917-537d20b6/manifest.json b/.ddx/executions/20260504T004917-537d20b6/manifest.json
new file mode 100644
index 0000000..58ae0bc
--- /dev/null
+++ b/.ddx/executions/20260504T004917-537d20b6/manifest.json
@@ -0,0 +1,39 @@
+{
+  "attempt_id": "20260504T004917-537d20b6",
+  "bead_id": "axon-2705d3ac",
+  "base_rev": "3b5c8cd12261b83ee8946b3664bdb5514c73f489",
+  "created_at": "2026-05-04T00:49:18.757758794Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-2705d3ac",
+    "title": "build(feat-009): MCP tools — axon.query and per-named-query",
+    "description": "Per ADR-021 §MCP surfacing and FEAT-009 US-073/US-076. MCP exposes:\n\n- axon.query(cypher, parameters) — generic ad-hoc query tool, mirrors GraphQL axonQuery.\n- One tool per named query, named after the query (e.g. ddx_beads.ready_beads). Same parameter shape as the GraphQL field.\n\nTool descriptions surface the named query's documentation string. Both tools enforce identical policy and limits as the GraphQL surface.",
+    "acceptance": "AC1. axon.query MCP tool with cypher + parameters. AC2. Each named query in the active schema generates a corresponding MCP tool. AC3. Tool descriptions include the named query's description: field. AC4. Tools enforce same policy/limits as GraphQL surface. AC5. cargo test -p axon-mcp passes; clippy clean.",
+    "labels": [
+      "helix",
+      "feat-009",
+      "feat-016",
+      "area:mcp",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-04T00:49:17Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "execute-loop-heartbeat-at": "2026-05-04T00:49:17.839609454Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260504T004917-537d20b6",
+    "prompt": ".ddx/executions/20260504T004917-537d20b6/prompt.md",
+    "manifest": ".ddx/executions/20260504T004917-537d20b6/manifest.json",
+    "result": ".ddx/executions/20260504T004917-537d20b6/result.json",
+    "checks": ".ddx/executions/20260504T004917-537d20b6/checks.json",
+    "usage": ".ddx/executions/20260504T004917-537d20b6/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-2705d3ac-20260504T004917-537d20b6"
+  },
+  "prompt_sha": "b5e1293075676ac7f4930dded00f409c663f25e88f7cfe480f9e0aa42d78253a"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260504T004917-537d20b6/result.json b/.ddx/executions/20260504T004917-537d20b6/result.json
new file mode 100644
index 0000000..4db8a9e
--- /dev/null
+++ b/.ddx/executions/20260504T004917-537d20b6/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-2705d3ac",
+  "attempt_id": "20260504T004917-537d20b6",
+  "base_rev": "3b5c8cd12261b83ee8946b3664bdb5514c73f489",
+  "result_rev": "1c1db41eff472c7881bb85fd3387a40ecd6ac636",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-56d242ca",
+  "duration_ms": 1173122,
+  "tokens": 8486136,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260504T004917-537d20b6",
+  "prompt_file": ".ddx/executions/20260504T004917-537d20b6/prompt.md",
+  "manifest_file": ".ddx/executions/20260504T004917-537d20b6/manifest.json",
+  "result_file": ".ddx/executions/20260504T004917-537d20b6/result.json",
+  "usage_file": ".ddx/executions/20260504T004917-537d20b6/usage.json",
+  "started_at": "2026-05-04T00:49:18.759997147Z",
+  "finished_at": "2026-05-04T01:08:51.882013764Z"
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
