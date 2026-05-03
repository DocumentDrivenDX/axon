<bead-review>
  <bead id="axon-8b814cae" iter=1>
    <title>build(feat-017): lazy-read schema migration (US-062) — unblocks DDx GA</title>
    <description>
Per FEAT-017 US-062. Entities at schema_version=N remain readable after schema bumps to N+1, with declared on_read_defaults applied for added optional fields.

Schema declarations may include an on_read_defaults: {field: default} block per FEAT-017 US-062.

Read of schema_version=N entity against active schema N+1: (1) fields named in on_read_defaults populated from defaults, (2) fields added in N+1 without defaults returned as null, (3) required-without-default returned with field absent + structured warning (writes still rejected), (4) storage NOT modified — entity stays at version N until next write.

Unblocks DDx Use Case C per axon-82b6f7b2.
    </description>
    <acceptance>
AC1. Schema declares on_read_defaults block. AC2. Read of older entity applies defaults; read of newer entity unaffected. AC3. Required-without-default returns warning, not failure, on read. AC4. Storage unchanged on read; entity version persists. AC5. Eager revalidation (US-060) still available as opt-in. AC6. cargo test passes; clippy clean. AC7. DDx integration sanity test: worker reading older bead after schema bump sees defaults.
    </acceptance>
    <labels>helix, feat-017, area:schema, downstream:ddx, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T055309-58ffbdea/manifest.json</file>
    <file>.ddx/executions/20260503T055309-58ffbdea/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="b5284d847d1bc2c6c298b9b201816b0371dab4ad">
diff --git a/.ddx/executions/20260503T055309-58ffbdea/manifest.json b/.ddx/executions/20260503T055309-58ffbdea/manifest.json
new file mode 100644
index 0000000..dcf87e6
--- /dev/null
+++ b/.ddx/executions/20260503T055309-58ffbdea/manifest.json
@@ -0,0 +1,73 @@
+{
+  "attempt_id": "20260503T055309-58ffbdea",
+  "bead_id": "axon-8b814cae",
+  "base_rev": "a5ab0e6d1486e6b951ea4ae37e6d32a8fc86a4d0",
+  "created_at": "2026-05-03T05:53:09.922900088Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-8b814cae",
+    "title": "build(feat-017): lazy-read schema migration (US-062) — unblocks DDx GA",
+    "description": "Per FEAT-017 US-062. Entities at schema_version=N remain readable after schema bumps to N+1, with declared on_read_defaults applied for added optional fields.\n\nSchema declarations may include an on_read_defaults: {field: default} block per FEAT-017 US-062.\n\nRead of schema_version=N entity against active schema N+1: (1) fields named in on_read_defaults populated from defaults, (2) fields added in N+1 without defaults returned as null, (3) required-without-default returned with field absent + structured warning (writes still rejected), (4) storage NOT modified — entity stays at version N until next write.\n\nUnblocks DDx Use Case C per axon-82b6f7b2.",
+    "acceptance": "AC1. Schema declares on_read_defaults block. AC2. Read of older entity applies defaults; read of newer entity unaffected. AC3. Required-without-default returns warning, not failure, on read. AC4. Storage unchanged on read; entity version persists. AC5. Eager revalidation (US-060) still available as opt-in. AC6. cargo test passes; clippy clean. AC7. DDx integration sanity test: worker reading older bead after schema bump sees defaults.",
+    "labels": [
+      "helix",
+      "feat-017",
+      "area:schema",
+      "downstream:ddx",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T05:53:09Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "events": [
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"vidar\",\"resolved_model\":\"MiniMax-M2.5-MLX-4bit\",\"fallback_chain\":[],\"requested_model\":\"MiniMax-M2.5-MLX-4bit\"}",
+          "created_at": "2026-05-02T20:57:10.977699847Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=vidar model=MiniMax-M2.5-MLX-4bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"http://vidar:1235/v1/chat/completions\": 507 Insufficient Storage {\"message\":\"Model 'MiniMax-M2.5-MLX-4bit' (125.82GB) exceeds max-model-memory (64.00GB)\",\"type\":\"server_error\",\"param\":null,\"code\":null}\nresult_rev=34791011f04724caf6146a720ee2a3e03f72c5d8\nbase_rev=34791011f04724caf6146a720ee2a3e03f72c5d8\nretry_after=2026-05-03T02:57:11Z",
+          "created_at": "2026-05-02T20:57:11.363390505Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"\",\"fallback_chain\":[]}",
+          "created_at": "2026-05-03T02:43:22.11946669Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider="
+        },
+        {
+          "actor": "ddx",
+          "body": "ResolveRoute: no viable routing candidate: 4 candidates rejected\nresult_rev=e1f654a308f29593dd15e3255619216ee9d47b2f\nbase_rev=e1f654a308f29593dd15e3255619216ee9d47b2f\nretry_after=2026-05-03T08:43:22Z",
+          "created_at": "2026-05-03T02:43:22.632235427Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T05:53:09.270513515Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T055309-58ffbdea",
+    "prompt": ".ddx/executions/20260503T055309-58ffbdea/prompt.md",
+    "manifest": ".ddx/executions/20260503T055309-58ffbdea/manifest.json",
+    "result": ".ddx/executions/20260503T055309-58ffbdea/result.json",
+    "checks": ".ddx/executions/20260503T055309-58ffbdea/checks.json",
+    "usage": ".ddx/executions/20260503T055309-58ffbdea/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-8b814cae-20260503T055309-58ffbdea"
+  },
+  "prompt_sha": "e3741899d2d26dc59c1298c0f5d78992b643f9bb67b01da3b42352b14af625f5"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T055309-58ffbdea/result.json b/.ddx/executions/20260503T055309-58ffbdea/result.json
new file mode 100644
index 0000000..111901b
--- /dev/null
+++ b/.ddx/executions/20260503T055309-58ffbdea/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-8b814cae",
+  "attempt_id": "20260503T055309-58ffbdea",
+  "base_rev": "a5ab0e6d1486e6b951ea4ae37e6d32a8fc86a4d0",
+  "result_rev": "b825d0c789e16a7884154f314db63732025b412c",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-fd02ef55",
+  "duration_ms": 842191,
+  "tokens": 5764607,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T055309-58ffbdea",
+  "prompt_file": ".ddx/executions/20260503T055309-58ffbdea/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T055309-58ffbdea/manifest.json",
+  "result_file": ".ddx/executions/20260503T055309-58ffbdea/result.json",
+  "usage_file": ".ddx/executions/20260503T055309-58ffbdea/usage.json",
+  "started_at": "2026-05-03T05:53:09.924016079Z",
+  "finished_at": "2026-05-03T06:07:12.115753682Z"
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
