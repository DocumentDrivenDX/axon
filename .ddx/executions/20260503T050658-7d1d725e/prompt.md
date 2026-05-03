<bead-review>
  <bead id="axon-648120e4" iter=1>
    <title>build(feat-030): preview audit threading Pattern A — create_preview_record takes audit log</title>
    <description>
preview-audit-threading.md picked Pattern A. Implement the signature change:

create_preview_record&lt;S: StorageAdapter, A: AuditLog&gt;(
    &amp;self, storage: &amp;mut S, audit: &amp;mut A, intent: MutationIntent,
) -&gt; Result&lt;MutationIntentPreviewRecord, MutationIntentLifecycleError&gt;

Emit operational event with field shape from preview-audit-threading.md: operation=mutation_intent.preview, actor=intent subject, collection=__mutation_intents, entity_id=intent_id, metadata={decision, schema_version, policy_version, operation_hash, expires_at}, before=null, after=review_summary only.

Update GraphQL handler (crates/axon-graphql/src/dynamic.rs:4817-4910) and MCP handler (crates/axon-mcp/src/handlers.rs:674-724) to pass audit. Update test callers throughout crates/axon-api/src/intent.rs.

Add integration test asserting auditLog(collection: '__mutation_intents', entityId: $intentId) returns preview event for both GraphQL- and MCP-originated previews.

Closes axon-e21cad01 (decision-doc bead).
    </description>
    <acceptance>
AC1. create_preview_record signature includes &amp;mut AuditLog. AC2. Operational event emitted on every successful preview-record creation per the field shape above. AC3. GraphQL preview endpoint passes audit; MCP preview endpoint passes audit. AC4. auditLog query integration test passes for both surfaces. AC5. cargo test --workspace passes; clippy clean. AC6. axon-e21cad01 closes as superseded. AC7. axon-ab2e52e0 (parent) progresses or closes per its AC.
    </acceptance>
    <labels>helix, feat-030, area:api, area:audit, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T044613-ef9f4ac1/manifest.json</file>
    <file>.ddx/executions/20260503T044613-ef9f4ac1/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="e8d6e27c80ecb51db92786502b6dacfe7671b158">
diff --git a/.ddx/executions/20260503T044613-ef9f4ac1/manifest.json b/.ddx/executions/20260503T044613-ef9f4ac1/manifest.json
new file mode 100644
index 0000000..8ee0a15
--- /dev/null
+++ b/.ddx/executions/20260503T044613-ef9f4ac1/manifest.json
@@ -0,0 +1,73 @@
+{
+  "attempt_id": "20260503T044613-ef9f4ac1",
+  "bead_id": "axon-648120e4",
+  "base_rev": "4a789dcab871c566fc3cff82e5980968f9a6019d",
+  "created_at": "2026-05-03T04:46:13.667480147Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-648120e4",
+    "title": "build(feat-030): preview audit threading Pattern A — create_preview_record takes audit log",
+    "description": "preview-audit-threading.md picked Pattern A. Implement the signature change:\n\ncreate_preview_record\u003cS: StorageAdapter, A: AuditLog\u003e(\n    \u0026self, storage: \u0026mut S, audit: \u0026mut A, intent: MutationIntent,\n) -\u003e Result\u003cMutationIntentPreviewRecord, MutationIntentLifecycleError\u003e\n\nEmit operational event with field shape from preview-audit-threading.md: operation=mutation_intent.preview, actor=intent subject, collection=__mutation_intents, entity_id=intent_id, metadata={decision, schema_version, policy_version, operation_hash, expires_at}, before=null, after=review_summary only.\n\nUpdate GraphQL handler (crates/axon-graphql/src/dynamic.rs:4817-4910) and MCP handler (crates/axon-mcp/src/handlers.rs:674-724) to pass audit. Update test callers throughout crates/axon-api/src/intent.rs.\n\nAdd integration test asserting auditLog(collection: '__mutation_intents', entityId: $intentId) returns preview event for both GraphQL- and MCP-originated previews.\n\nCloses axon-e21cad01 (decision-doc bead).",
+    "acceptance": "AC1. create_preview_record signature includes \u0026mut AuditLog. AC2. Operational event emitted on every successful preview-record creation per the field shape above. AC3. GraphQL preview endpoint passes audit; MCP preview endpoint passes audit. AC4. auditLog query integration test passes for both surfaces. AC5. cargo test --workspace passes; clippy clean. AC6. axon-e21cad01 closes as superseded. AC7. axon-ab2e52e0 (parent) progresses or closes per its AC.",
+    "labels": [
+      "helix",
+      "feat-030",
+      "area:api",
+      "area:audit",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T04:46:12Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "events": [
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"vidar\",\"resolved_model\":\"MiniMax-M2.5-MLX-4bit\",\"fallback_chain\":[],\"requested_model\":\"MiniMax-M2.5-MLX-4bit\"}",
+          "created_at": "2026-05-02T20:55:19.880759431Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=vidar model=MiniMax-M2.5-MLX-4bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"http://vidar:1235/v1/chat/completions\": 507 Insufficient Storage {\"message\":\"Model 'MiniMax-M2.5-MLX-4bit' (125.82GB) exceeds max-model-memory (64.00GB)\",\"type\":\"server_error\",\"param\":null,\"code\":null}\nresult_rev=c6d20f1edd371deb7c3ecf6a23300afb81b3c5ce\nbase_rev=c6d20f1edd371deb7c3ecf6a23300afb81b3c5ce\nretry_after=2026-05-03T02:55:20Z",
+          "created_at": "2026-05-02T20:55:21.017274173Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"\",\"fallback_chain\":[]}",
+          "created_at": "2026-05-03T02:43:04.563834076Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider="
+        },
+        {
+          "actor": "ddx",
+          "body": "ResolveRoute: no viable routing candidate: 4 candidates rejected\nresult_rev=833db6b438ffb47d4b672d767b75e9987f772505\nbase_rev=833db6b438ffb47d4b672d767b75e9987f772505\nretry_after=2026-05-03T08:43:04Z",
+          "created_at": "2026-05-03T02:43:04.939171014Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T04:46:12.995526113Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T044613-ef9f4ac1",
+    "prompt": ".ddx/executions/20260503T044613-ef9f4ac1/prompt.md",
+    "manifest": ".ddx/executions/20260503T044613-ef9f4ac1/manifest.json",
+    "result": ".ddx/executions/20260503T044613-ef9f4ac1/result.json",
+    "checks": ".ddx/executions/20260503T044613-ef9f4ac1/checks.json",
+    "usage": ".ddx/executions/20260503T044613-ef9f4ac1/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-648120e4-20260503T044613-ef9f4ac1"
+  },
+  "prompt_sha": "afbe5971526c4bc8b1e8104e1387160725d889e5caebdf861594d886c4fed2a3"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T044613-ef9f4ac1/result.json b/.ddx/executions/20260503T044613-ef9f4ac1/result.json
new file mode 100644
index 0000000..f6a0d51
--- /dev/null
+++ b/.ddx/executions/20260503T044613-ef9f4ac1/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-648120e4",
+  "attempt_id": "20260503T044613-ef9f4ac1",
+  "base_rev": "4a789dcab871c566fc3cff82e5980968f9a6019d",
+  "result_rev": "348233cb92625a1a19a2855efc26935ca729870a",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-3067936f",
+  "duration_ms": 1232082,
+  "tokens": 20636834,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T044613-ef9f4ac1",
+  "prompt_file": ".ddx/executions/20260503T044613-ef9f4ac1/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T044613-ef9f4ac1/manifest.json",
+  "result_file": ".ddx/executions/20260503T044613-ef9f4ac1/result.json",
+  "usage_file": ".ddx/executions/20260503T044613-ef9f4ac1/usage.json",
+  "started_at": "2026-05-03T04:46:13.668773956Z",
+  "finished_at": "2026-05-03T05:06:45.751194104Z"
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
