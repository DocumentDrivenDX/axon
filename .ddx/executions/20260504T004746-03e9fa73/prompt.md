<bead-review>
  <bead id="axon-d3fbd10a" iter=1>
    <title>build(feat-009): axonQuery ad-hoc GraphQL resolver</title>
    <description>
Implement the generic axonQuery field per FEAT-009 US-076 and ADR-021 §GraphQL surfacing/Ad-hoc queries.

type Query { axonQuery(cypher: String!, parameters: JSON): AxonQueryResult! }
type AxonQueryResult { rows: [JSON!]!, schema: AxonQuerySchema!, metadata: AxonQueryMetadata! }

Same parser/validator/planner/executor/policy-enforcement path as named queries. Parsing rejects references to identifiers not in the active schema. Cardinality budget rejection. 30-second wall-clock timeout. Stable error codes per CypherError::code().

Out of scope: ad-hoc subscriptions (deferred per ADR-021), MCP equivalent (separate bead).
    </description>
    <acceptance>
AC1. axonQuery field exists with cypher + parameters arguments. AC2. Result includes rows (JSON), schema (column types), metadata (plan info, index usage). AC3. Errors carry stable codes from CypherError. AC4. Policy enforced identically to named queries. AC5. Tests cover: valid query, parse_error, unknown_identifier, unsupported_query_plan, query_too_large, policy_required_bypass.
    </acceptance>
    <labels>helix, feat-009, feat-015, area:graphql, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260504T003240-bc4bc145/manifest.json</file>
    <file>.ddx/executions/20260504T003240-bc4bc145/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="8bae5929c7786680d02e738e961e71b4dc882b33">
diff --git a/.ddx/executions/20260504T003240-bc4bc145/manifest.json b/.ddx/executions/20260504T003240-bc4bc145/manifest.json
new file mode 100644
index 0000000..431b957
--- /dev/null
+++ b/.ddx/executions/20260504T003240-bc4bc145/manifest.json
@@ -0,0 +1,39 @@
+{
+  "attempt_id": "20260504T003240-bc4bc145",
+  "bead_id": "axon-d3fbd10a",
+  "base_rev": "fbef69eb27f97e1c036ef23b5a603d1a7f605ae3",
+  "created_at": "2026-05-04T00:32:41.535235295Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-d3fbd10a",
+    "title": "build(feat-009): axonQuery ad-hoc GraphQL resolver",
+    "description": "Implement the generic axonQuery field per FEAT-009 US-076 and ADR-021 §GraphQL surfacing/Ad-hoc queries.\n\ntype Query { axonQuery(cypher: String!, parameters: JSON): AxonQueryResult! }\ntype AxonQueryResult { rows: [JSON!]!, schema: AxonQuerySchema!, metadata: AxonQueryMetadata! }\n\nSame parser/validator/planner/executor/policy-enforcement path as named queries. Parsing rejects references to identifiers not in the active schema. Cardinality budget rejection. 30-second wall-clock timeout. Stable error codes per CypherError::code().\n\nOut of scope: ad-hoc subscriptions (deferred per ADR-021), MCP equivalent (separate bead).",
+    "acceptance": "AC1. axonQuery field exists with cypher + parameters arguments. AC2. Result includes rows (JSON), schema (column types), metadata (plan info, index usage). AC3. Errors carry stable codes from CypherError. AC4. Policy enforced identically to named queries. AC5. Tests cover: valid query, parse_error, unknown_identifier, unsupported_query_plan, query_too_large, policy_required_bypass.",
+    "labels": [
+      "helix",
+      "feat-009",
+      "feat-015",
+      "area:graphql",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-04T00:32:40Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "execute-loop-heartbeat-at": "2026-05-04T00:32:40.765119021Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260504T003240-bc4bc145",
+    "prompt": ".ddx/executions/20260504T003240-bc4bc145/prompt.md",
+    "manifest": ".ddx/executions/20260504T003240-bc4bc145/manifest.json",
+    "result": ".ddx/executions/20260504T003240-bc4bc145/result.json",
+    "checks": ".ddx/executions/20260504T003240-bc4bc145/checks.json",
+    "usage": ".ddx/executions/20260504T003240-bc4bc145/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-d3fbd10a-20260504T003240-bc4bc145"
+  },
+  "prompt_sha": "4b859ac977677b0b6613555772599a028658777266693e623847c239393dc124"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260504T003240-bc4bc145/result.json b/.ddx/executions/20260504T003240-bc4bc145/result.json
new file mode 100644
index 0000000..a874678
--- /dev/null
+++ b/.ddx/executions/20260504T003240-bc4bc145/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-d3fbd10a",
+  "attempt_id": "20260504T003240-bc4bc145",
+  "base_rev": "fbef69eb27f97e1c036ef23b5a603d1a7f605ae3",
+  "result_rev": "005abd7b550e0f9cb623f0bfb77c091db34a9009",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-baa6b69d",
+  "duration_ms": 894375,
+  "tokens": 6482994,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260504T003240-bc4bc145",
+  "prompt_file": ".ddx/executions/20260504T003240-bc4bc145/prompt.md",
+  "manifest_file": ".ddx/executions/20260504T003240-bc4bc145/manifest.json",
+  "result_file": ".ddx/executions/20260504T003240-bc4bc145/result.json",
+  "usage_file": ".ddx/executions/20260504T003240-bc4bc145/usage.json",
+  "started_at": "2026-05-04T00:32:41.536179178Z",
+  "finished_at": "2026-05-04T00:47:35.911657227Z"
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
