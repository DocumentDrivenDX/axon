<bead-review>
  <bead id="axon-9a4f3b72" iter=1>
    <title>build(feat-009): ESF queries: block — schema-time named query registration</title>
    <description>
Extend axon-schema to accept a queries: block in collection schemas per FEAT-009 US-075 and ADR-021. Each named query has description, cypher (string), and parameters (list of {name, type, required}).

At put_schema time, each named query is: (1) parsed via axon-cypher::parse, (2) validated via axon-cypher::validate against the active schema, (3) index-validated — unindexed scans on large collections rejected, (4) policy-validated per ADR-019 — bypass-required queries rejected with policy_required_bypass.

Out of scope: GraphQL field generation, MCP tool generation, subscriptions (separate beads).
    </description>
    <acceptance>
AC1. axon-schema accepts queries: block per FEAT-009 US-075. AC2. put_schema returns CompileReport with per-query diagnostics (ok/parse_error/unknown_identifier/unsupported_query_plan/policy_required_bypass). AC3. put_schema --dry-run returns diagnostics without activating. AC4. Activated schemas expose named-query metadata via existing schema-introspection paths. AC5. cargo test -p axon-schema passes; clippy clean. AC6. ≥8 tests: valid query activates, parse error rejected, unknown label/property/relationship rejected, unindexed-plan rejected, policy-bypass rejected, parameterized query, multiple named queries, dry-run path.
    </acceptance>
    <labels>helix, feat-009, feat-002, area:schema, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T040101-1f366984/manifest.json</file>
    <file>.ddx/executions/20260503T040101-1f366984/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="dc02ecb41e8e235e69f1bc287bcc045d90234640">
diff --git a/.ddx/executions/20260503T040101-1f366984/manifest.json b/.ddx/executions/20260503T040101-1f366984/manifest.json
new file mode 100644
index 0000000..4956149
--- /dev/null
+++ b/.ddx/executions/20260503T040101-1f366984/manifest.json
@@ -0,0 +1,73 @@
+{
+  "attempt_id": "20260503T040101-1f366984",
+  "bead_id": "axon-9a4f3b72",
+  "base_rev": "71079c6a2ddbf9c09a85df61f54e762492762daa",
+  "created_at": "2026-05-03T04:01:02.068957126Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-9a4f3b72",
+    "title": "build(feat-009): ESF queries: block — schema-time named query registration",
+    "description": "Extend axon-schema to accept a queries: block in collection schemas per FEAT-009 US-075 and ADR-021. Each named query has description, cypher (string), and parameters (list of {name, type, required}).\n\nAt put_schema time, each named query is: (1) parsed via axon-cypher::parse, (2) validated via axon-cypher::validate against the active schema, (3) index-validated — unindexed scans on large collections rejected, (4) policy-validated per ADR-019 — bypass-required queries rejected with policy_required_bypass.\n\nOut of scope: GraphQL field generation, MCP tool generation, subscriptions (separate beads).",
+    "acceptance": "AC1. axon-schema accepts queries: block per FEAT-009 US-075. AC2. put_schema returns CompileReport with per-query diagnostics (ok/parse_error/unknown_identifier/unsupported_query_plan/policy_required_bypass). AC3. put_schema --dry-run returns diagnostics without activating. AC4. Activated schemas expose named-query metadata via existing schema-introspection paths. AC5. cargo test -p axon-schema passes; clippy clean. AC6. ≥8 tests: valid query activates, parse error rejected, unknown label/property/relationship rejected, unindexed-plan rejected, policy-bypass rejected, parameterized query, multiple named queries, dry-run path.",
+    "labels": [
+      "helix",
+      "feat-009",
+      "feat-002",
+      "area:schema",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T04:01:01Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "events": [
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"vidar\",\"resolved_model\":\"MiniMax-M2.5-MLX-4bit\",\"fallback_chain\":[],\"requested_model\":\"MiniMax-M2.5-MLX-4bit\"}",
+          "created_at": "2026-05-02T20:54:06.548831674Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=vidar model=MiniMax-M2.5-MLX-4bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"http://vidar:1235/v1/chat/completions\": 507 Insufficient Storage {\"message\":\"Model 'MiniMax-M2.5-MLX-4bit' (125.82GB) exceeds max-model-memory (64.00GB)\",\"type\":\"server_error\",\"param\":null,\"code\":null}\nresult_rev=5841301e64af1cb4db663bfdefa31d99c17902c7\nbase_rev=5841301e64af1cb4db663bfdefa31d99c17902c7\nretry_after=2026-05-03T02:54:06Z",
+          "created_at": "2026-05-02T20:54:06.988705683Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"\",\"fallback_chain\":[]}",
+          "created_at": "2026-05-03T02:42:51.459126375Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider="
+        },
+        {
+          "actor": "ddx",
+          "body": "ResolveRoute: no viable routing candidate: 4 candidates rejected\nresult_rev=3faf7290a133a58b05341450d26e42ca358ce179\nbase_rev=3faf7290a133a58b05341450d26e42ca358ce179\nretry_after=2026-05-03T08:42:51Z",
+          "created_at": "2026-05-03T02:42:51.869053581Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T04:01:01.36950733Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T040101-1f366984",
+    "prompt": ".ddx/executions/20260503T040101-1f366984/prompt.md",
+    "manifest": ".ddx/executions/20260503T040101-1f366984/manifest.json",
+    "result": ".ddx/executions/20260503T040101-1f366984/result.json",
+    "checks": ".ddx/executions/20260503T040101-1f366984/checks.json",
+    "usage": ".ddx/executions/20260503T040101-1f366984/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-9a4f3b72-20260503T040101-1f366984"
+  },
+  "prompt_sha": "69ebb62f74240bfb743198d9bc03a0f7d3bbb97404facd7abbd412e23cb9e159"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T040101-1f366984/result.json b/.ddx/executions/20260503T040101-1f366984/result.json
new file mode 100644
index 0000000..f083b77
--- /dev/null
+++ b/.ddx/executions/20260503T040101-1f366984/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-9a4f3b72",
+  "attempt_id": "20260503T040101-1f366984",
+  "base_rev": "71079c6a2ddbf9c09a85df61f54e762492762daa",
+  "result_rev": "9fd009f17f6feb974e6d948fb20186bde68bc7ac",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-a5957829",
+  "duration_ms": 1514332,
+  "tokens": 14018241,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T040101-1f366984",
+  "prompt_file": ".ddx/executions/20260503T040101-1f366984/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T040101-1f366984/manifest.json",
+  "result_file": ".ddx/executions/20260503T040101-1f366984/result.json",
+  "usage_file": ".ddx/executions/20260503T040101-1f366984/usage.json",
+  "started_at": "2026-05-03T04:01:02.070154529Z",
+  "finished_at": "2026-05-03T04:26:16.402405953Z"
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
