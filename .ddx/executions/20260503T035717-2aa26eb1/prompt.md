<bead-review>
  <bead id="axon-51d884f0" iter=1>
    <title>build(feat-009): Cypher planner — AST + schema → execution plan</title>
    <description>
Implement the planner module in crates/axon-cypher per ADR-021 §Compilation strategy. Parser and validator already exist (crates/axon-cypher/src/{parser,validator,schema}.rs). The planner takes a parsed/validated Query plus a SchemaSnapshot and produces an ExecutionPlan operator tree (Scan, IndexLookup, Expand, Filter, Project, Sort, Skip, Limit, ExistsCheck).

Index selection rules per ADR-021 §Compilation/Index usage: (1) label+property predicate uses FEAT-013 secondary index. (2) Relationship traversal uses links table PK/target index. (3) EXISTS subquery uses links-table index probe. (4) ORDER BY covered by index uses index scan order. (5) Otherwise full scan with predicate pushdown, subject to cost budget.

Cost budget per ADR-021 §Cost and depth limits: variable-length paths require explicit bounds, default depth cap 10. Cardinality estimates from index stats; reject plans above 1M intermediate rows for ad-hoc, named queries can override.

Out of scope: actual execution (separate bead), real schema integration (uses SchemaSnapshot fixture), GraphQL/MCP exposure.
    </description>
    <acceptance>
AC1. crates/axon-cypher/src/planner.rs has plan(query, schema) -&gt; Result&lt;ExecutionPlan, CypherError&gt;. AC2. ExecutionPlan operator tree types serialize to JSON for observability. AC3. The DDx ready/blocked query (FEAT-009 US-074) compiles using the secondary index on DdxBead.status for both outer match and EXISTS subquery. AC4. Plans requiring unindexed scans on collections above the configured threshold (default 1000) return CypherError::UnsupportedQueryPlan with a missing-index diagnostic. AC5. cargo test -p axon-cypher passes; cargo clippy -p axon-cypher --all-targets -- -D warnings clean. AC6. ≥8 unit tests covering: index hit, range scan, sort-via-index, EXISTS via index probe, OPTIONAL MATCH, fallback scan rejection, depth cap, cardinality budget rejection.
    </acceptance>
    <labels>helix, feat-009, area:cypher, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T034127-c287e4dd/manifest.json</file>
    <file>.ddx/executions/20260503T034127-c287e4dd/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="87849a5dcdfc88410d4dc99072b7516f0e5daa66">
diff --git a/.ddx/executions/20260503T034127-c287e4dd/manifest.json b/.ddx/executions/20260503T034127-c287e4dd/manifest.json
new file mode 100644
index 0000000..14a6e2d
--- /dev/null
+++ b/.ddx/executions/20260503T034127-c287e4dd/manifest.json
@@ -0,0 +1,72 @@
+{
+  "attempt_id": "20260503T034127-c287e4dd",
+  "bead_id": "axon-51d884f0",
+  "base_rev": "78bdd9efb06051f62da6b22ed1a6fe525f866eb0",
+  "created_at": "2026-05-03T03:41:29.411238586Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-51d884f0",
+    "title": "build(feat-009): Cypher planner — AST + schema → execution plan",
+    "description": "Implement the planner module in crates/axon-cypher per ADR-021 §Compilation strategy. Parser and validator already exist (crates/axon-cypher/src/{parser,validator,schema}.rs). The planner takes a parsed/validated Query plus a SchemaSnapshot and produces an ExecutionPlan operator tree (Scan, IndexLookup, Expand, Filter, Project, Sort, Skip, Limit, ExistsCheck).\n\nIndex selection rules per ADR-021 §Compilation/Index usage: (1) label+property predicate uses FEAT-013 secondary index. (2) Relationship traversal uses links table PK/target index. (3) EXISTS subquery uses links-table index probe. (4) ORDER BY covered by index uses index scan order. (5) Otherwise full scan with predicate pushdown, subject to cost budget.\n\nCost budget per ADR-021 §Cost and depth limits: variable-length paths require explicit bounds, default depth cap 10. Cardinality estimates from index stats; reject plans above 1M intermediate rows for ad-hoc, named queries can override.\n\nOut of scope: actual execution (separate bead), real schema integration (uses SchemaSnapshot fixture), GraphQL/MCP exposure.",
+    "acceptance": "AC1. crates/axon-cypher/src/planner.rs has plan(query, schema) -\u003e Result\u003cExecutionPlan, CypherError\u003e. AC2. ExecutionPlan operator tree types serialize to JSON for observability. AC3. The DDx ready/blocked query (FEAT-009 US-074) compiles using the secondary index on DdxBead.status for both outer match and EXISTS subquery. AC4. Plans requiring unindexed scans on collections above the configured threshold (default 1000) return CypherError::UnsupportedQueryPlan with a missing-index diagnostic. AC5. cargo test -p axon-cypher passes; cargo clippy -p axon-cypher --all-targets -- -D warnings clean. AC6. ≥8 unit tests covering: index hit, range scan, sort-via-index, EXISTS via index probe, OPTIONAL MATCH, fallback scan rejection, depth cap, cardinality budget rejection.",
+    "labels": [
+      "helix",
+      "feat-009",
+      "area:cypher",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T03:41:27Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "events": [
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"vidar\",\"resolved_model\":\"MiniMax-M2.5-MLX-4bit\",\"fallback_chain\":[],\"requested_model\":\"MiniMax-M2.5-MLX-4bit\"}",
+          "created_at": "2026-05-02T20:53:30.797833466Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=vidar model=MiniMax-M2.5-MLX-4bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"http://vidar:1235/v1/chat/completions\": 507 Insufficient Storage {\"message\":\"Model 'MiniMax-M2.5-MLX-4bit' (125.82GB) exceeds max-model-memory (64.00GB)\",\"type\":\"server_error\",\"param\":null,\"code\":null}\nresult_rev=6a8661470c0b9ff82b33de19aed4cceedb61b1c6\nbase_rev=6a8661470c0b9ff82b33de19aed4cceedb61b1c6\nretry_after=2026-05-03T02:53:31Z",
+          "created_at": "2026-05-02T20:53:31.172732176Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"\",\"fallback_chain\":[]}",
+          "created_at": "2026-05-03T02:42:45.989357174Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider="
+        },
+        {
+          "actor": "ddx",
+          "body": "ResolveRoute: no viable routing candidate: 4 candidates rejected\nresult_rev=404bdc5680bb966f0e4dd39be32299bc03d24059\nbase_rev=404bdc5680bb966f0e4dd39be32299bc03d24059\nretry_after=2026-05-03T08:42:46Z",
+          "created_at": "2026-05-03T02:42:46.385363246Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T03:41:27.782719172Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T034127-c287e4dd",
+    "prompt": ".ddx/executions/20260503T034127-c287e4dd/prompt.md",
+    "manifest": ".ddx/executions/20260503T034127-c287e4dd/manifest.json",
+    "result": ".ddx/executions/20260503T034127-c287e4dd/result.json",
+    "checks": ".ddx/executions/20260503T034127-c287e4dd/checks.json",
+    "usage": ".ddx/executions/20260503T034127-c287e4dd/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-51d884f0-20260503T034127-c287e4dd"
+  },
+  "prompt_sha": "33ef7c75c936475ea7566691c118c7635d61b267f8c8cbc0b299f2270be0fb4a"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T034127-c287e4dd/result.json b/.ddx/executions/20260503T034127-c287e4dd/result.json
new file mode 100644
index 0000000..f1f4817
--- /dev/null
+++ b/.ddx/executions/20260503T034127-c287e4dd/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-51d884f0",
+  "attempt_id": "20260503T034127-c287e4dd",
+  "base_rev": "78bdd9efb06051f62da6b22ed1a6fe525f866eb0",
+  "result_rev": "2251159d0801ce71bed91b3106efd616d85bc9ac",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-95282c8e",
+  "duration_ms": 936687,
+  "tokens": 8141334,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T034127-c287e4dd",
+  "prompt_file": ".ddx/executions/20260503T034127-c287e4dd/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T034127-c287e4dd/manifest.json",
+  "result_file": ".ddx/executions/20260503T034127-c287e4dd/result.json",
+  "usage_file": ".ddx/executions/20260503T034127-c287e4dd/usage.json",
+  "started_at": "2026-05-03T03:41:29.412579501Z",
+  "finished_at": "2026-05-03T03:57:06.100278594Z"
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
