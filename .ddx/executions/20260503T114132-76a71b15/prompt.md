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
    <notes>
REVIEW:BLOCK

Diff contains only ddx execution metadata (manifest.json, result.json) for the attempt. No source changes to axon-schema, axon-cypher, or tests are present, so none of AC1–AC6 can be evaluated as implemented.
    </notes>
    <labels>helix, feat-009, feat-002, area:schema, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T113138-9dd29676/manifest.json</file>
    <file>.ddx/executions/20260503T113138-9dd29676/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="6d3c21048cf8b14cddba2dcef322e54543dd6357">
diff --git a/.ddx/executions/20260503T113138-9dd29676/manifest.json b/.ddx/executions/20260503T113138-9dd29676/manifest.json
new file mode 100644
index 0000000..113e351
--- /dev/null
+++ b/.ddx/executions/20260503T113138-9dd29676/manifest.json
@@ -0,0 +1,129 @@
+{
+  "attempt_id": "20260503T113138-9dd29676",
+  "bead_id": "axon-9a4f3b72",
+  "base_rev": "c8a08017a55db91a4490e77b37772bb981ec6d09",
+  "created_at": "2026-05-03T11:31:39.021751615Z",
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
+      "claimed-at": "2026-05-03T11:31:38Z",
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
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"codex\",\"fallback_chain\":[],\"requested_harness\":\"codex\"}",
+          "created_at": "2026-05-03T04:26:16.409196179Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260503T040101-1f366984\",\"harness\":\"codex\",\"input_tokens\":13988759,\"output_tokens\":29482,\"total_tokens\":14018241,\"cost_usd\":0,\"duration_ms\":1514332,\"exit_code\":0}",
+          "created_at": "2026-05-03T04:26:16.519520614Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=14018241"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"escalation_count\":0,\"fallback_chain\":[],\"final_tier\":\"\",\"requested_profile\":\"\",\"requested_tier\":\"\",\"resolved_model\":\"\",\"resolved_provider\":\"codex\",\"resolved_tier\":\"\"}",
+          "created_at": "2026-05-03T04:26:26.59566702Z",
+          "kind": "routing",
+          "source": "ddx agent execute-loop",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "Diff contains only ddx execution metadata (manifest.json, result.json) for the attempt. No source changes to axon-schema, axon-cypher, or tests are present, so none of AC1–AC6 can be evaluated as implemented.\nharness=claude\nmodel=opus\ninput_bytes=9266\noutput_bytes=1303\nelapsed_ms=14614",
+          "created_at": "2026-05-03T04:26:43.3906185Z",
+          "kind": "review",
+          "source": "ddx agent execute-loop",
+          "summary": "BLOCK"
+        },
+        {
+          "actor": "",
+          "body": "",
+          "created_at": "2026-05-03T04:26:43.474203904Z",
+          "kind": "reopen",
+          "source": "",
+          "summary": "review: BLOCK"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"action\":\"re_attempt_with_context\",\"mode\":\"review_block\"}",
+          "created_at": "2026-05-03T04:26:43.593643232Z",
+          "kind": "triage-decision",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block: re_attempt_with_context"
+        },
+        {
+          "actor": "ddx",
+          "body": "post-merge review: BLOCK (flagged for human)\nDiff contains only ddx execution metadata (manifest.json, result.json) for the attempt. No source changes to axon-schema, axon-cypher, or tests are present, so none of AC1–AC6 can be evaluated as implemented.\nresult_rev=dc02ecb41e8e235e69f1bc287bcc045d90234640\nbase_rev=71079c6a2ddbf9c09a85df61f54e762492762daa",
+          "created_at": "2026-05-03T04:26:43.725942767Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T11:31:38.451456538Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T113138-9dd29676",
+    "prompt": ".ddx/executions/20260503T113138-9dd29676/prompt.md",
+    "manifest": ".ddx/executions/20260503T113138-9dd29676/manifest.json",
+    "result": ".ddx/executions/20260503T113138-9dd29676/result.json",
+    "checks": ".ddx/executions/20260503T113138-9dd29676/checks.json",
+    "usage": ".ddx/executions/20260503T113138-9dd29676/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-9a4f3b72-20260503T113138-9dd29676"
+  },
+  "prompt_sha": "5f3cf9f6430d532c98542d44523f83671ae81e80c1362de810b1f7a24954abe5"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T113138-9dd29676/result.json b/.ddx/executions/20260503T113138-9dd29676/result.json
new file mode 100644
index 0000000..eb4cab7
--- /dev/null
+++ b/.ddx/executions/20260503T113138-9dd29676/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-9a4f3b72",
+  "attempt_id": "20260503T113138-9dd29676",
+  "base_rev": "c8a08017a55db91a4490e77b37772bb981ec6d09",
+  "result_rev": "41fc4a10cd50f9f04a6369f58e70a0f92162b8de",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-40fdc11f",
+  "duration_ms": 573787,
+  "tokens": 3549260,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T113138-9dd29676",
+  "prompt_file": ".ddx/executions/20260503T113138-9dd29676/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T113138-9dd29676/manifest.json",
+  "result_file": ".ddx/executions/20260503T113138-9dd29676/result.json",
+  "usage_file": ".ddx/executions/20260503T113138-9dd29676/usage.json",
+  "started_at": "2026-05-03T11:31:39.022792693Z",
+  "finished_at": "2026-05-03T11:41:12.810628459Z"
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
