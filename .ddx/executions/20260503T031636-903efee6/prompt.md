<bead-review>
  <bead id="axon-cf99b8a4" iter=1>
    <title>build(feat-031): GraphQL policyOverride support for explainPolicy and effectivePolicy (backend half of axon-80979cb8)</title>
    <description>
Codex review of axon-80979cb8 flagged this hidden gap: the UI side now sends `policyOverride` arguments in AxonUiEffectivePolicy and explain queries (api.ts:1502-1565), but the GraphQL resolvers in crates/axon-graphql/src/dynamic.rs do NOT accept or apply policyOverride. Verified: `grep policyOverride crates/axon-graphql/src/` returns nothing. The UI work landed with a silent no-op — proposed-matrix state fetched against the active access_control regardless of caller intent.

Scope:

A. Add an optional `policyOverride: JSON` argument to two field resolvers in crates/axon-graphql/src/dynamic.rs:
   - effectivePolicy(collection, entityId, policyOverride)
   - explainPolicy(input, policyOverride) — and the dry-run variant explainPolicyDryRun if it exists

B. Plumb the override through the policy-evaluator path so that when policyOverride is non-null, the evaluation runs against the supplied access_control instead of the schema's persisted active access_control. The schema's draft.access_control is the canonical shape.

C. Validate the override at parse time; reject malformed policy with a typed error code (e.g. `invalid_policy_override` with field-level diagnostics matching the policy-validation error shape used elsewhere).

D. Ensure existing callers that pass null/omitted policyOverride get exactly the previous behavior (active policy).

E. Rust unit tests in crates/axon-graphql/tests/policy_dryrun_test.rs (or extending an existing test file) covering: null override → active policy result; valid override → override applied; malformed override → typed error; override that flips a decision allow→deny → reflected in result.

F. UI integration sanity check: run the existing tests/e2e/policy-authoring.spec.ts — should pass without changes (no UI changes needed; UI already sends the field). If a fixture needs updating to exercise the new path, do that minimally.

Out of scope: UI render of proposed column (axon-93fcc7f7), delta module (axon-4b264f4e), Playwright delta assertions (axon-1fb17586). This bead is server-only.
    </description>
    <acceptance>
AC1. effectivePolicy and explainPolicy GraphQL fields accept optional policyOverride: JSON argument.

AC2. When policyOverride is non-null, the evaluation uses the override's access_control; when null/omitted, behavior is unchanged.

AC3. Malformed policyOverride returns a typed error (invalid_policy_override) with a field-level diagnostic, not a 500.

AC4. Rust unit tests cover the four scenarios above and pass: cargo test -p axon-graphql.

AC5. cargo clippy --workspace -- -D warnings clean.

AC6. tests/e2e/policy-authoring.spec.ts passes (no regression in active-policy path).
    </acceptance>
    <labels>helix, feat-031, decomp, area:graphql</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T025509-d65d4ac2/manifest.json</file>
    <file>.ddx/executions/20260503T025509-d65d4ac2/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="48e72cc6297df1772c93c1992fbc156f3d06f0dc">
diff --git a/.ddx/executions/20260503T025509-d65d4ac2/manifest.json b/.ddx/executions/20260503T025509-d65d4ac2/manifest.json
new file mode 100644
index 0000000..efc9bb3
--- /dev/null
+++ b/.ddx/executions/20260503T025509-d65d4ac2/manifest.json
@@ -0,0 +1,121 @@
+{
+  "attempt_id": "20260503T025509-d65d4ac2",
+  "bead_id": "axon-cf99b8a4",
+  "base_rev": "8295f9c9d24edaeff291402463f63c1d1ec17a2b",
+  "created_at": "2026-05-03T02:55:09.960385967Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-cf99b8a4",
+    "title": "build(feat-031): GraphQL policyOverride support for explainPolicy and effectivePolicy (backend half of axon-80979cb8)",
+    "description": "Codex review of axon-80979cb8 flagged this hidden gap: the UI side now sends `policyOverride` arguments in AxonUiEffectivePolicy and explain queries (api.ts:1502-1565), but the GraphQL resolvers in crates/axon-graphql/src/dynamic.rs do NOT accept or apply policyOverride. Verified: `grep policyOverride crates/axon-graphql/src/` returns nothing. The UI work landed with a silent no-op — proposed-matrix state fetched against the active access_control regardless of caller intent.\n\nScope:\n\nA. Add an optional `policyOverride: JSON` argument to two field resolvers in crates/axon-graphql/src/dynamic.rs:\n   - effectivePolicy(collection, entityId, policyOverride)\n   - explainPolicy(input, policyOverride) — and the dry-run variant explainPolicyDryRun if it exists\n\nB. Plumb the override through the policy-evaluator path so that when policyOverride is non-null, the evaluation runs against the supplied access_control instead of the schema's persisted active access_control. The schema's draft.access_control is the canonical shape.\n\nC. Validate the override at parse time; reject malformed policy with a typed error code (e.g. `invalid_policy_override` with field-level diagnostics matching the policy-validation error shape used elsewhere).\n\nD. Ensure existing callers that pass null/omitted policyOverride get exactly the previous behavior (active policy).\n\nE. Rust unit tests in crates/axon-graphql/tests/policy_dryrun_test.rs (or extending an existing test file) covering: null override → active policy result; valid override → override applied; malformed override → typed error; override that flips a decision allow→deny → reflected in result.\n\nF. UI integration sanity check: run the existing tests/e2e/policy-authoring.spec.ts — should pass without changes (no UI changes needed; UI already sends the field). If a fixture needs updating to exercise the new path, do that minimally.\n\nOut of scope: UI render of proposed column (axon-93fcc7f7), delta module (axon-4b264f4e), Playwright delta assertions (axon-1fb17586). This bead is server-only.",
+    "acceptance": "AC1. effectivePolicy and explainPolicy GraphQL fields accept optional policyOverride: JSON argument.\n\nAC2. When policyOverride is non-null, the evaluation uses the override's access_control; when null/omitted, behavior is unchanged.\n\nAC3. Malformed policyOverride returns a typed error (invalid_policy_override) with a field-level diagnostic, not a 500.\n\nAC4. Rust unit tests cover the four scenarios above and pass: cargo test -p axon-graphql.\n\nAC5. cargo clippy --workspace -- -D warnings clean.\n\nAC6. tests/e2e/policy-authoring.spec.ts passes (no regression in active-policy path).",
+    "parent": "axon-ff92fed7",
+    "labels": [
+      "helix",
+      "feat-031",
+      "decomp",
+      "area:graphql"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T02:55:09Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "events": [
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"openrouter\",\"resolved_model\":\"openai/gpt-5.4-mini\",\"fallback_chain\":[]}",
+          "created_at": "2026-04-30T00:55:29.11177998Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=openrouter model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260430T005449-e9673250\",\"harness\":\"agent\",\"provider\":\"openrouter\",\"model\":\"openai/gpt-5.4-mini\",\"input_tokens\":366423,\"output_tokens\":967,\"total_tokens\":367390,\"cost_usd\":0.05452875,\"duration_ms\":38461,\"exit_code\":1}",
+          "created_at": "2026-04-30T00:55:29.281501103Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=367390 cost_usd=0.0545 model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "erik",
+          "body": "stalled: no_progress_tools_exceeded\nresult_rev=e6e4553d5846f2b9fd94c40ddf65766e2c4a69ba\nbase_rev=e6e4553d5846f2b9fd94c40ddf65766e2c4a69ba\nretry_after=2026-04-30T06:55:31Z",
+          "created_at": "2026-04-30T00:55:32.019836434Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"vidar\",\"resolved_model\":\"MiniMax-M2.5-MLX-4bit\",\"fallback_chain\":[],\"requested_model\":\"MiniMax-M2.5-MLX-4bit\"}",
+          "created_at": "2026-05-02T20:53:13.811073244Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=vidar model=MiniMax-M2.5-MLX-4bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"http://vidar:1235/v1/chat/completions\": 507 Insufficient Storage {\"message\":\"Model 'MiniMax-M2.5-MLX-4bit' (125.82GB) exceeds max-model-memory (64.00GB)\",\"type\":\"server_error\",\"param\":null,\"code\":null}\nresult_rev=04a8f933fe90ab28dde344743639e68820924c46\nbase_rev=04a8f933fe90ab28dde344743639e68820924c46\nretry_after=2026-05-03T02:53:14Z",
+          "created_at": "2026-05-02T20:53:14.276542283Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"omlx-vidar-1235\",\"resolved_model\":\"Qwen3.6-27B-MLX-8bit\",\"fallback_chain\":[],\"requested_harness\":\"agent\",\"requested_model\":\"Qwen3.6-27B-MLX-8bit\"}",
+          "created_at": "2026-05-03T01:24:47.380301576Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=omlx-vidar-1235 model=Qwen3.6-27B-MLX-8bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260503T000249-fc0ea0b1\",\"harness\":\"agent\",\"provider\":\"omlx-vidar-1235\",\"model\":\"Qwen3.6-27B-MLX-8bit\",\"input_tokens\":316858,\"output_tokens\":2816,\"total_tokens\":319674,\"cost_usd\":0,\"duration_ms\":4915727,\"exit_code\":1}",
+          "created_at": "2026-05-03T01:24:47.489278669Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=319674 model=Qwen3.6-27B-MLX-8bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "stalled: no_progress_tools_exceeded\nresult_rev=84802faa47e422c4644fce99222c1f7c5ac66470\nbase_rev=84802faa47e422c4644fce99222c1f7c5ac66470\nretry_after=2026-05-03T07:24:48Z",
+          "created_at": "2026-05-03T01:24:48.499597125Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"\",\"fallback_chain\":[]}",
+          "created_at": "2026-05-03T02:42:40.510574301Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider="
+        },
+        {
+          "actor": "ddx",
+          "body": "ResolveRoute: no viable routing candidate: 4 candidates rejected\nresult_rev=5e202688452892cfaa097b39f0871a207ad6af5c\nbase_rev=5e202688452892cfaa097b39f0871a207ad6af5c\nretry_after=2026-05-03T08:42:40Z",
+          "created_at": "2026-05-03T02:42:40.894399082Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T02:55:09.211566384Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T025509-d65d4ac2",
+    "prompt": ".ddx/executions/20260503T025509-d65d4ac2/prompt.md",
+    "manifest": ".ddx/executions/20260503T025509-d65d4ac2/manifest.json",
+    "result": ".ddx/executions/20260503T025509-d65d4ac2/result.json",
+    "checks": ".ddx/executions/20260503T025509-d65d4ac2/checks.json",
+    "usage": ".ddx/executions/20260503T025509-d65d4ac2/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-cf99b8a4-20260503T025509-d65d4ac2"
+  },
+  "prompt_sha": "8863c51250335db7cba12deb4a592063902f74a9df3773542528362af8996dfb"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T025509-d65d4ac2/result.json b/.ddx/executions/20260503T025509-d65d4ac2/result.json
new file mode 100644
index 0000000..0326839
--- /dev/null
+++ b/.ddx/executions/20260503T025509-d65d4ac2/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-cf99b8a4",
+  "attempt_id": "20260503T025509-d65d4ac2",
+  "base_rev": "8295f9c9d24edaeff291402463f63c1d1ec17a2b",
+  "result_rev": "b60fc3090636898ccd7ec9096b3231cd5ab2dcd6",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-e89941a1",
+  "duration_ms": 1273963,
+  "tokens": 10984123,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T025509-d65d4ac2",
+  "prompt_file": ".ddx/executions/20260503T025509-d65d4ac2/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T025509-d65d4ac2/manifest.json",
+  "result_file": ".ddx/executions/20260503T025509-d65d4ac2/result.json",
+  "usage_file": ".ddx/executions/20260503T025509-d65d4ac2/usage.json",
+  "started_at": "2026-05-03T02:55:09.961529662Z",
+  "finished_at": "2026-05-03T03:16:23.925463131Z"
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
