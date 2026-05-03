<bead-review>
  <bead id="axon-79d1df96" iter=1>
    <title>build(feat-030): audit approval and intent lineage</title>
    <description>
Record preview, approval, rejection, expiry, stale, and committed intent lineage with actor, agent/tool identity when present, policy version, approver reason, intent ID, and pre/post images with FEAT-029 redaction on reads.
    </description>
    <acceptance>
Audit tests prove lineage can be queried by intent ID and entity ID, approvals/rejections include required metadata, and redacted audit reads match entity read redaction.
    </acceptance>
    <notes>
REVIEW:BLOCK

Diff contains only an execution metadata file (result.json) with no code or test changes. No audit lineage implementation, no queries by intent/entity ID, no approval/rejection metadata, and no redacted audit reads are present to evaluate against the acceptance criteria.

REVIEW:BLOCK

Diff contains only execution metadata files (manifest.json and result.json) under .ddx/executions/. No source code, tests, or audit lineage implementation are present to evaluate against any acceptance criterion.

REVIEW:BLOCK

Diff contains only execution metadata files (manifest.json, result.json) under .ddx/executions/. No source code or tests implementing audit lineage, intent/entity ID queries, approval/rejection metadata, or redacted audit reads are present to evaluate against the acceptance criteria.
    </notes>
    <labels>helix, phase:build, kind:implementation, area:audit, feat-030, needs_human</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T224008-95193a59/manifest.json</file>
    <file>.ddx/executions/20260503T224008-95193a59/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="310e4e7e1e6ebb1e027961525ea6d2b332b4a506">
diff --git a/.ddx/executions/20260503T224008-95193a59/manifest.json b/.ddx/executions/20260503T224008-95193a59/manifest.json
new file mode 100644
index 0000000..2c5addd
--- /dev/null
+++ b/.ddx/executions/20260503T224008-95193a59/manifest.json
@@ -0,0 +1,260 @@
+{
+  "attempt_id": "20260503T224008-95193a59",
+  "bead_id": "axon-79d1df96",
+  "base_rev": "9da583b6b4468f1bb65131b743a1b895ece2ed17",
+  "created_at": "2026-05-03T22:40:09.227962101Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-79d1df96",
+    "title": "build(feat-030): audit approval and intent lineage",
+    "description": "Record preview, approval, rejection, expiry, stale, and committed intent lineage with actor, agent/tool identity when present, policy version, approver reason, intent ID, and pre/post images with FEAT-029 redaction on reads.",
+    "acceptance": "Audit tests prove lineage can be queried by intent ID and entity ID, approvals/rejections include required metadata, and redacted audit reads match entity read redaction.",
+    "parent": "axon-c7111156",
+    "labels": [
+      "helix",
+      "phase:build",
+      "kind:implementation",
+      "area:audit",
+      "feat-030",
+      "needs_human"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T22:40:08Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "events": [
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"openrouter\",\"resolved_model\":\"qwen/qwen3.6-35b-a3b\",\"fallback_chain\":[],\"requested_model\":\"qwen/qwen3.6-35b-a3b\"}",
+          "created_at": "2026-05-02T20:52:20.586388511Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=openrouter model=qwen/qwen3.6-35b-a3b"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"https://openrouter.ai/api/v1/chat/completions\": 401 Unauthorized {\"message\":\"Missing Authentication header\",\"code\":401}\nresult_rev=6039f1ecb3b8591a25a85edbd1ec6423b7512136\nbase_rev=6039f1ecb3b8591a25a85edbd1ec6423b7512136\nretry_after=2026-05-03T02:52:20Z",
+          "created_at": "2026-05-02T20:52:21.019233312Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"lmstudio-vidar-1234\",\"resolved_model\":\"Qwen3.6-27B-MLX-8bit\",\"fallback_chain\":[],\"requested_harness\":\"agent\",\"requested_model\":\"Qwen3.6-27B-MLX-8bit\"}",
+          "created_at": "2026-05-02T23:51:50.372360709Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=lmstudio-vidar-1234 model=Qwen3.6-27B-MLX-8bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"http://vidar:1234/v1/chat/completions\": 502 Bad Gateway \nresult_rev=e1428bae82fd7388d24f981e6fcbc007a527f2f5\nbase_rev=e1428bae82fd7388d24f981e6fcbc007a527f2f5\nretry_after=2026-05-03T05:51:50Z",
+          "created_at": "2026-05-02T23:51:50.739098225Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"\",\"fallback_chain\":[]}",
+          "created_at": "2026-05-03T02:42:33.82519413Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider="
+        },
+        {
+          "actor": "ddx",
+          "body": "ResolveRoute: no viable routing candidate: 4 candidates rejected\nresult_rev=c48a1e788f086216550f403124a2df21e384da20\nbase_rev=c48a1e788f086216550f403124a2df21e384da20\nretry_after=2026-05-03T08:42:35Z",
+          "created_at": "2026-05-03T02:42:35.289282094Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"codex\",\"fallback_chain\":[],\"requested_harness\":\"codex\"}",
+          "created_at": "2026-05-03T03:07:51.642771016Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260503T024425-64b08244\",\"harness\":\"codex\",\"input_tokens\":15208367,\"output_tokens\":19326,\"total_tokens\":15227693,\"cost_usd\":0,\"duration_ms\":1405054,\"exit_code\":0}",
+          "created_at": "2026-05-03T03:07:51.722935619Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=15227693"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"escalation_count\":0,\"fallback_chain\":[],\"final_tier\":\"\",\"requested_profile\":\"\",\"requested_tier\":\"\",\"resolved_model\":\"\",\"resolved_provider\":\"codex\",\"resolved_tier\":\"\"}",
+          "created_at": "2026-05-03T03:08:06.136665487Z",
+          "kind": "routing",
+          "source": "ddx agent execute-loop",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "Diff contains only an execution metadata file (result.json) with no code or test changes. No audit lineage implementation, no queries by intent/entity ID, no approval/rejection metadata, and no redacted audit reads are present to evaluate against the acceptance criteria.\nharness=claude\nmodel=opus\ninput_bytes=3552\noutput_bytes=1302\nelapsed_ms=17903",
+          "created_at": "2026-05-03T03:08:24.499700577Z",
+          "kind": "review",
+          "source": "ddx agent execute-loop",
+          "summary": "BLOCK"
+        },
+        {
+          "actor": "",
+          "body": "",
+          "created_at": "2026-05-03T03:08:24.606721209Z",
+          "kind": "reopen",
+          "source": "",
+          "summary": "review: BLOCK"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"action\":\"re_attempt_with_context\",\"mode\":\"review_block\"}",
+          "created_at": "2026-05-03T03:08:24.700104776Z",
+          "kind": "triage-decision",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block: re_attempt_with_context"
+        },
+        {
+          "actor": "ddx",
+          "body": "post-merge review: BLOCK (flagged for human)\nDiff contains only an execution metadata file (result.json) with no code or test changes. No audit lineage implementation, no queries by intent/entity ID, no approval/rejection metadata, and no redacted audit reads are present to evaluate against the acceptance criteria.\nresult_rev=8782a31e4d4434900ecbe5498551f376972ee419\nbase_rev=4df979430f61bd4deae5f9fbf79a1ed8859c5bbc",
+          "created_at": "2026-05-03T03:08:24.89195149Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"codex\",\"fallback_chain\":[],\"requested_harness\":\"codex\"}",
+          "created_at": "2026-05-03T03:35:55.428250354Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260503T031741-bab2b647\",\"harness\":\"codex\",\"input_tokens\":23753619,\"output_tokens\":19426,\"total_tokens\":23773045,\"cost_usd\":0,\"duration_ms\":1093065,\"exit_code\":0}",
+          "created_at": "2026-05-03T03:35:55.516673231Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=23773045"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"escalation_count\":0,\"fallback_chain\":[],\"final_tier\":\"\",\"requested_profile\":\"\",\"requested_tier\":\"\",\"resolved_model\":\"\",\"resolved_provider\":\"codex\",\"resolved_tier\":\"\"}",
+          "created_at": "2026-05-03T03:36:04.684547169Z",
+          "kind": "routing",
+          "source": "ddx agent execute-loop",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "Diff contains only execution metadata files (manifest.json and result.json) under .ddx/executions/. No source code, tests, or audit lineage implementation are present to evaluate against any acceptance criterion.\nharness=claude\nmodel=opus\ninput_bytes=11812\noutput_bytes=1138\nelapsed_ms=18887",
+          "created_at": "2026-05-03T03:36:23.765847907Z",
+          "kind": "review",
+          "source": "ddx agent execute-loop",
+          "summary": "BLOCK"
+        },
+        {
+          "actor": "",
+          "body": "",
+          "created_at": "2026-05-03T03:36:23.845653603Z",
+          "kind": "reopen",
+          "source": "",
+          "summary": "review: BLOCK"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"action\":\"escalate_tier\",\"mode\":\"review_block\",\"tier_hint\":\"standard\"}",
+          "created_at": "2026-05-03T03:36:23.963010558Z",
+          "kind": "triage-decision",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block: escalate_tier"
+        },
+        {
+          "actor": "ddx",
+          "body": "post-merge review: BLOCK (flagged for human)\nDiff contains only execution metadata files (manifest.json and result.json) under .ddx/executions/. No source code, tests, or audit lineage implementation are present to evaluate against any acceptance criterion.\nresult_rev=a705e392d354cfe5b9c4d5111e90318d9854a210\nbase_rev=3ec1ddb8ed94112a4a98fcc404638bd7f4ccf749",
+          "created_at": "2026-05-03T03:36:24.150404759Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"codex\",\"fallback_chain\":[],\"requested_harness\":\"codex\"}",
+          "created_at": "2026-05-03T11:25:39.585917619Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260503T110845-123129f1\",\"harness\":\"codex\",\"input_tokens\":8756921,\"output_tokens\":15434,\"total_tokens\":8772355,\"cost_usd\":0,\"duration_ms\":1013093,\"exit_code\":0}",
+          "created_at": "2026-05-03T11:25:39.706889659Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=8772355"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"escalation_count\":0,\"fallback_chain\":[],\"final_tier\":\"\",\"requested_profile\":\"\",\"requested_tier\":\"\",\"resolved_model\":\"\",\"resolved_provider\":\"codex\",\"resolved_tier\":\"\"}",
+          "created_at": "2026-05-03T11:25:49.819796098Z",
+          "kind": "routing",
+          "source": "ddx agent execute-loop",
+          "summary": "provider=codex"
+        },
+        {
+          "actor": "ddx",
+          "body": "Diff contains only execution metadata files (manifest.json, result.json) under .ddx/executions/. No source code or tests implementing audit lineage, intent/entity ID queries, approval/rejection metadata, or redacted audit reads are present to evaluate against the acceptance criteria.\nharness=claude\nmodel=opus\ninput_bytes=15020\noutput_bytes=1288\nelapsed_ms=17935",
+          "created_at": "2026-05-03T11:26:09.918841226Z",
+          "kind": "review",
+          "source": "ddx agent execute-loop",
+          "summary": "BLOCK"
+        },
+        {
+          "actor": "",
+          "body": "",
+          "created_at": "2026-05-03T11:26:09.994749997Z",
+          "kind": "reopen",
+          "source": "",
+          "summary": "review: BLOCK"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"action\":\"needs_human\",\"mode\":\"review_block\"}",
+          "created_at": "2026-05-03T11:26:10.127037433Z",
+          "kind": "triage-decision",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block: needs_human"
+        },
+        {
+          "actor": "ddx",
+          "body": "post-merge review: BLOCK (flagged for human)\nDiff contains only execution metadata files (manifest.json, result.json) under .ddx/executions/. No source code or tests implementing audit lineage, intent/entity ID queries, approval/rejection metadata, or redacted audit reads are present to evaluate against the acceptance criteria.\nresult_rev=162bbd72c658b4af4a9215d57a83c1b74a61e0a1\nbase_rev=40a552c7870930e336a44dc34318721c3d65c97b",
+          "created_at": "2026-05-03T11:26:10.330332629Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T22:40:08.486187169Z",
+      "triage.tier_hint": "standard"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T224008-95193a59",
+    "prompt": ".ddx/executions/20260503T224008-95193a59/prompt.md",
+    "manifest": ".ddx/executions/20260503T224008-95193a59/manifest.json",
+    "result": ".ddx/executions/20260503T224008-95193a59/result.json",
+    "checks": ".ddx/executions/20260503T224008-95193a59/checks.json",
+    "usage": ".ddx/executions/20260503T224008-95193a59/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-79d1df96-20260503T224008-95193a59"
+  },
+  "prompt_sha": "4f924d2cf4c5c9046c64db7fd46db831df2334a288766cdeaa649383f37a19a5"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T224008-95193a59/result.json b/.ddx/executions/20260503T224008-95193a59/result.json
new file mode 100644
index 0000000..bb98d95
--- /dev/null
+++ b/.ddx/executions/20260503T224008-95193a59/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-79d1df96",
+  "attempt_id": "20260503T224008-95193a59",
+  "base_rev": "9da583b6b4468f1bb65131b743a1b895ece2ed17",
+  "result_rev": "b6300324c7c53ac9b85676dae662db4be2db5efd",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-8a1e9dcb",
+  "duration_ms": 741491,
+  "tokens": 6527111,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T224008-95193a59",
+  "prompt_file": ".ddx/executions/20260503T224008-95193a59/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T224008-95193a59/manifest.json",
+  "result_file": ".ddx/executions/20260503T224008-95193a59/result.json",
+  "usage_file": ".ddx/executions/20260503T224008-95193a59/usage.json",
+  "started_at": "2026-05-03T22:40:09.229761778Z",
+  "finished_at": "2026-05-03T22:52:30.721736203Z"
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
