<bead-review>
  <bead id="axon-15de5e84" iter=1>
    <title>build(feat-009): GraphQL field generation for schema-declared named queries</title>
    <description>
After a schema with a queries: block activates (see ESF queries bead axon-9a4f3b72), each named query surfaces as a typed GraphQL Query field with connection pagination per FEAT-009 US-072 and ADR-021 §GraphQL surfacing.

For ddx_beads.ready_beads:
type Query { ready_beads(first: Int, after: String): DdxBeadConnection! }

Parameterized queries surface their parameters as field arguments. Result type follows the named query's RETURN projection. Connection contract follows FEAT-015 (edges, pageInfo, totalCount, all policy-filtered).

Out of scope: ad-hoc axonQuery resolver, MCP exposure, subscriptions (separate beads).
    </description>
    <acceptance>
AC1. axon-graphql generates a Query field per named query with the schema-declared parameter shape. AC2. Generated field returns a Connection following FEAT-015. AC3. Results policy-filtered per FEAT-029 — hidden rows omitted, redacted fields rendered as null. AC4. The DDx ready_beads query is callable via GraphQL; cargo test -p axon-graphql passes. AC5. cargo clippy --workspace -- -D warnings clean.
    </acceptance>
    <labels>helix, feat-009, feat-015, area:graphql, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260504T000840-23a7c0f6/manifest.json</file>
    <file>.ddx/executions/20260504T000840-23a7c0f6/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="f14836ed4a9b34faec36abea5aea77679df7a965">
diff --git a/.ddx/executions/20260504T000840-23a7c0f6/manifest.json b/.ddx/executions/20260504T000840-23a7c0f6/manifest.json
new file mode 100644
index 0000000..a89407d
--- /dev/null
+++ b/.ddx/executions/20260504T000840-23a7c0f6/manifest.json
@@ -0,0 +1,39 @@
+{
+  "attempt_id": "20260504T000840-23a7c0f6",
+  "bead_id": "axon-15de5e84",
+  "base_rev": "94f0ab200e1b1ce5b49ef445c23adf6e58d67042",
+  "created_at": "2026-05-04T00:08:40.962539537Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-15de5e84",
+    "title": "build(feat-009): GraphQL field generation for schema-declared named queries",
+    "description": "After a schema with a queries: block activates (see ESF queries bead axon-9a4f3b72), each named query surfaces as a typed GraphQL Query field with connection pagination per FEAT-009 US-072 and ADR-021 §GraphQL surfacing.\n\nFor ddx_beads.ready_beads:\ntype Query { ready_beads(first: Int, after: String): DdxBeadConnection! }\n\nParameterized queries surface their parameters as field arguments. Result type follows the named query's RETURN projection. Connection contract follows FEAT-015 (edges, pageInfo, totalCount, all policy-filtered).\n\nOut of scope: ad-hoc axonQuery resolver, MCP exposure, subscriptions (separate beads).",
+    "acceptance": "AC1. axon-graphql generates a Query field per named query with the schema-declared parameter shape. AC2. Generated field returns a Connection following FEAT-015. AC3. Results policy-filtered per FEAT-029 — hidden rows omitted, redacted fields rendered as null. AC4. The DDx ready_beads query is callable via GraphQL; cargo test -p axon-graphql passes. AC5. cargo clippy --workspace -- -D warnings clean.",
+    "labels": [
+      "helix",
+      "feat-009",
+      "feat-015",
+      "area:graphql",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-04T00:08:40Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "execute-loop-heartbeat-at": "2026-05-04T00:08:40.2743385Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260504T000840-23a7c0f6",
+    "prompt": ".ddx/executions/20260504T000840-23a7c0f6/prompt.md",
+    "manifest": ".ddx/executions/20260504T000840-23a7c0f6/manifest.json",
+    "result": ".ddx/executions/20260504T000840-23a7c0f6/result.json",
+    "checks": ".ddx/executions/20260504T000840-23a7c0f6/checks.json",
+    "usage": ".ddx/executions/20260504T000840-23a7c0f6/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-15de5e84-20260504T000840-23a7c0f6"
+  },
+  "prompt_sha": "8546b38b080b46ccb6fd36469966c96b33e558e6035496ae74a2cc1bc33074c5"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260504T000840-23a7c0f6/result.json b/.ddx/executions/20260504T000840-23a7c0f6/result.json
new file mode 100644
index 0000000..ff0d355
--- /dev/null
+++ b/.ddx/executions/20260504T000840-23a7c0f6/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-15de5e84",
+  "attempt_id": "20260504T000840-23a7c0f6",
+  "base_rev": "94f0ab200e1b1ce5b49ef445c23adf6e58d67042",
+  "result_rev": "22d84d8e27afe063aeebd278797e557f0b8ee5eb",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-f20ba062",
+  "duration_ms": 1337969,
+  "tokens": 24137760,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260504T000840-23a7c0f6",
+  "prompt_file": ".ddx/executions/20260504T000840-23a7c0f6/prompt.md",
+  "manifest_file": ".ddx/executions/20260504T000840-23a7c0f6/manifest.json",
+  "result_file": ".ddx/executions/20260504T000840-23a7c0f6/result.json",
+  "usage_file": ".ddx/executions/20260504T000840-23a7c0f6/usage.json",
+  "started_at": "2026-05-04T00:08:40.963514846Z",
+  "finished_at": "2026-05-04T00:30:58.932913581Z"
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
