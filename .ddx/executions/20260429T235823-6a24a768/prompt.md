<bead-review>
  <bead id="axon-db7a6d0a" iter=1>
    <title>feature: application audit-write API — first-class 'emit audit event' contract for downstream consumers</title>
    <description>
&lt;context&gt;
Filed on behalf of nexiq (/home/erik/Projects/nexiq). nexiq's FEAT-010 plan references writing audit events in multiple places:
  - access_denied events when a user hits a forbidden route (plan line 554, auth-design.md)
  - invoice approved / engagement status transition audit entries (plan line 104, line 486, line 497)
  - first-run user bootstrap events
  - dashboard views / PII exposures that policy wants recorded

Axon's current api-contracts.md (2026-04-19 state) covers audit READS at L269-L302 but does not document a consumer-facing audit WRITE path. Entity mutations already leave audit footprints in Axon's internal audit log (per ADR), but application-level events that are NOT tied to a specific entity mutation (e.g. 'user attempted to access /invoices but was denied by RBAC — no entity touched') need a first-class write contract.

Codex fresh-eyes review of nexiq's plan 2026-04-19 flagged this as a handwaved assumption that blocks Phase 2 implementation.
&lt;/context&gt;

&lt;what-nexiq-needs&gt;

1. **Event kinds used by nexiq** — in priority order:
   -  (route, attempted_action, caller_identity, reason)
   -  (already event-tied; could piggyback on the mutation)
   -  /  (piggyback on mutation)
   -  (no entity mutation; separate write)
   -  (no mutation; write before resolving identity)
   -  (user viewed rate_card, contractor rates, etc.)
   -  (user exported a report — compliance event)

   Most are piggyback-candidates; a few (access_denied, login_attempted, first_run) have no natural entity mutation to ride on.

2. **Shape options**:
   - A) Extend  to accept an optional  field alongside , so an app-level event can travel with an entity mutation OR alone (empty ops array + one event).
   - B) Separate endpoint  with idempotency semantics.
   - C) GraphQL mutation  once the GraphQL mutation layer ships.

   Downstream preference: (A) — matches the transaction-is-the-unit-of-change mental model, keeps idempotency uniform, and lets a route-denied event be written atomically with a downgrade operation if both are wanted.

3. **Schema**: same rationale as entity audit rows (actor from auth context, at from server time, ref is polymorphic entity reference or null, metadata is JSON). Nexiq doesn't need Axon to define application event_kinds ahead of time — a curated list is brittle; a free-form string is fine if Axon records what the consumer asserts.

4. **Query parity**: these events need to be readable via the same audit-read API so a nexiq dashboard can show 'all access_denied events on engagement X this week' regardless of whether the event came from an entity mutation or a standalone emit.
&lt;/what-nexiq-needs&gt;

&lt;interaction-with-axon-c5cc071a&gt;

The RBAC feature ask (axon-c5cc071a) includes an access_denied denial shape. If Axon's RBAC layer itself emits audit entries on denial, that might cover the 'access_denied' event kind automatically — in which case this bead narrows to the other event kinds above. Confirm whether RBAC denials go to audit automatically.
&lt;/interaction-with-axon-c5cc071a&gt;

&lt;priority-rationale&gt;

P1, not P0. Nexiq can ship Phase 2 without this by piggybacking on entity mutations for most cases and suppressing access_denied audit in the interim. But the workaround is cumbersome (every route component carries its own audit-write path) and creates a gap in compliance stories; filing now so Axon can scope it into the work-breakdown alongside RBAC enforcement.
&lt;/priority-rationale&gt;
    </description>
    <acceptance>
docs/helix/02-design/api-contracts.md adds an 'Audit Writes' section (alongside the existing 'Audit Reads' coverage at L269-L302) pinning: (a) the shape of a standalone audit entry a consumer can emit outside of a mutation (e.g. 'user viewed X', 'access_denied at route Y', 'login attempted from origin Z'); (b) whether audit entries are a separate endpoint/GraphQL mutation OR always piggyback on a transaction; (c) the event schema (event_kind, actor, subject_ref, payload_json, origin) and whether consumers can define new event_kind values or must use a curated list; (d) idempotency/replay semantics (do duplicate audit writes collapse?); (e) retention / queryability guarantees relative to entity audit rows; (f) the response shape when Axon rejects an audit write (rate limit, unknown event_kind, missing actor). A reference invocation example from a browser client and an integration worker is included. Downstream (nexiq) can then write access_denied events, user-facing login-attempt events, and business-level 'invoice approved' markers that are not coupled to a single entity mutation.
    </acceptance>
    <notes>
REVIEW:BLOCK

The change only adds an execution metadata file. None of the required API contract documentation for audit writes, schema, idempotency, rejection behavior, retention/queryability, or examples is present, so the bead is not implemented.

REVIEW:BLOCK

The diff only adds execution metadata. None of the required audit-write API contract documentation or examples are present, so the bead is not implemented.
    </notes>
    <labels>helix, phase:frame, kind:feature-request, area:api, area:audit, downstream:nexiq, cross-repo</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260429T235406-20eeaf2c/manifest.json</file>
    <file>.ddx/executions/20260429T235406-20eeaf2c/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="b1b4a5b585f3f41f0f2ebd8f84568a17190aad79">
diff --git a/.ddx/executions/20260429T235406-20eeaf2c/manifest.json b/.ddx/executions/20260429T235406-20eeaf2c/manifest.json
new file mode 100644
index 0000000..003706f
--- /dev/null
+++ b/.ddx/executions/20260429T235406-20eeaf2c/manifest.json
@@ -0,0 +1,231 @@
+{
+  "attempt_id": "20260429T235406-20eeaf2c",
+  "bead_id": "axon-db7a6d0a",
+  "base_rev": "337831991956adc3ef2f2aa838e8a31cff157735",
+  "created_at": "2026-04-29T23:54:07.667133828Z",
+  "requested": {
+    "harness": "claude",
+    "model": "claude-sonnet-4-6",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-db7a6d0a",
+    "title": "feature: application audit-write API — first-class 'emit audit event' contract for downstream consumers",
+    "description": "\u003ccontext\u003e\nFiled on behalf of nexiq (/home/erik/Projects/nexiq). nexiq's FEAT-010 plan references writing audit events in multiple places:\n  - access_denied events when a user hits a forbidden route (plan line 554, auth-design.md)\n  - invoice approved / engagement status transition audit entries (plan line 104, line 486, line 497)\n  - first-run user bootstrap events\n  - dashboard views / PII exposures that policy wants recorded\n\nAxon's current api-contracts.md (2026-04-19 state) covers audit READS at L269-L302 but does not document a consumer-facing audit WRITE path. Entity mutations already leave audit footprints in Axon's internal audit log (per ADR), but application-level events that are NOT tied to a specific entity mutation (e.g. 'user attempted to access /invoices but was denied by RBAC — no entity touched') need a first-class write contract.\n\nCodex fresh-eyes review of nexiq's plan 2026-04-19 flagged this as a handwaved assumption that blocks Phase 2 implementation.\n\u003c/context\u003e\n\n\u003cwhat-nexiq-needs\u003e\n\n1. **Event kinds used by nexiq** — in priority order:\n   -  (route, attempted_action, caller_identity, reason)\n   -  (already event-tied; could piggyback on the mutation)\n   -  /  (piggyback on mutation)\n   -  (no entity mutation; separate write)\n   -  (no mutation; write before resolving identity)\n   -  (user viewed rate_card, contractor rates, etc.)\n   -  (user exported a report — compliance event)\n\n   Most are piggyback-candidates; a few (access_denied, login_attempted, first_run) have no natural entity mutation to ride on.\n\n2. **Shape options**:\n   - A) Extend  to accept an optional  field alongside , so an app-level event can travel with an entity mutation OR alone (empty ops array + one event).\n   - B) Separate endpoint  with idempotency semantics.\n   - C) GraphQL mutation  once the GraphQL mutation layer ships.\n\n   Downstream preference: (A) — matches the transaction-is-the-unit-of-change mental model, keeps idempotency uniform, and lets a route-denied event be written atomically with a downgrade operation if both are wanted.\n\n3. **Schema**: same rationale as entity audit rows (actor from auth context, at from server time, ref is polymorphic entity reference or null, metadata is JSON). Nexiq doesn't need Axon to define application event_kinds ahead of time — a curated list is brittle; a free-form string is fine if Axon records what the consumer asserts.\n\n4. **Query parity**: these events need to be readable via the same audit-read API so a nexiq dashboard can show 'all access_denied events on engagement X this week' regardless of whether the event came from an entity mutation or a standalone emit.\n\u003c/what-nexiq-needs\u003e\n\n\u003cinteraction-with-axon-c5cc071a\u003e\n\nThe RBAC feature ask (axon-c5cc071a) includes an access_denied denial shape. If Axon's RBAC layer itself emits audit entries on denial, that might cover the 'access_denied' event kind automatically — in which case this bead narrows to the other event kinds above. Confirm whether RBAC denials go to audit automatically.\n\u003c/interaction-with-axon-c5cc071a\u003e\n\n\u003cpriority-rationale\u003e\n\nP1, not P0. Nexiq can ship Phase 2 without this by piggybacking on entity mutations for most cases and suppressing access_denied audit in the interim. But the workaround is cumbersome (every route component carries its own audit-write path) and creates a gap in compliance stories; filing now so Axon can scope it into the work-breakdown alongside RBAC enforcement.\n\u003c/priority-rationale\u003e",
+    "acceptance": "docs/helix/02-design/api-contracts.md adds an 'Audit Writes' section (alongside the existing 'Audit Reads' coverage at L269-L302) pinning: (a) the shape of a standalone audit entry a consumer can emit outside of a mutation (e.g. 'user viewed X', 'access_denied at route Y', 'login attempted from origin Z'); (b) whether audit entries are a separate endpoint/GraphQL mutation OR always piggyback on a transaction; (c) the event schema (event_kind, actor, subject_ref, payload_json, origin) and whether consumers can define new event_kind values or must use a curated list; (d) idempotency/replay semantics (do duplicate audit writes collapse?); (e) retention / queryability guarantees relative to entity audit rows; (f) the response shape when Axon rejects an audit write (rate limit, unknown event_kind, missing actor). A reference invocation example from a browser client and an integration worker is included. Downstream (nexiq) can then write access_denied events, user-facing login-attempt events, and business-level 'invoice approved' markers that are not coupled to a single entity mutation.",
+    "labels": [
+      "helix",
+      "phase:frame",
+      "kind:feature-request",
+      "area:api",
+      "area:audit",
+      "downstream:nexiq",
+      "cross-repo"
+    ],
+    "metadata": {
+      "claimed-at": "2026-04-29T23:54:06Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3580983",
+      "events": [
+        {
+          "actor": "erik",
+          "body": "tier=cheap harness= model= probe=no viable provider\nno viable harness found",
+          "created_at": "2026-04-29T03:02:46.551135762Z",
+          "kind": "tier-attempt",
+          "source": "ddx agent execute-loop",
+          "summary": "skipped"
+        },
+        {
+          "actor": "erik",
+          "body": "tier=standard harness= model= probe=no viable provider\nno viable harness found",
+          "created_at": "2026-04-29T03:02:48.783590142Z",
+          "kind": "tier-attempt",
+          "source": "ddx agent execute-loop",
+          "summary": "skipped"
+        },
+        {
+          "actor": "erik",
+          "body": "tier=smart harness= model= probe=no viable provider\nno viable harness found",
+          "created_at": "2026-04-29T03:02:51.033537696Z",
+          "kind": "tier-attempt",
+          "source": "ddx agent execute-loop",
+          "summary": "skipped"
+        },
+        {
+          "actor": "erik",
+          "body": "{\"tiers_attempted\":[{\"tier\":\"cheap\",\"status\":\"skipped\",\"cost_usd\":0,\"duration_ms\":0},{\"tier\":\"standard\",\"status\":\"skipped\",\"cost_usd\":0,\"duration_ms\":0},{\"tier\":\"smart\",\"status\":\"skipped\",\"cost_usd\":0,\"duration_ms\":0}],\"winning_tier\":\"exhausted\",\"total_cost_usd\":0,\"wasted_cost_usd\":0}",
+          "created_at": "2026-04-29T03:02:51.116233823Z",
+          "kind": "escalation-summary",
+          "source": "ddx agent execute-loop",
+          "summary": "winning_tier=exhausted attempts=3 total_cost_usd=0.0000 wasted_cost_usd=0.0000"
+        },
+        {
+          "actor": "erik",
+          "body": "execute-loop: all tiers exhausted — no viable provider found",
+          "created_at": "2026-04-29T03:02:51.279814414Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"bragi\",\"resolved_model\":\"qwen3.6\",\"fallback_chain\":[]}",
+          "created_at": "2026-04-29T03:37:10.663854416Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=bragi model=qwen3.6"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"bragi\",\"resolved_model\":\"qwen/qwen3.6-35b-a3b\",\"fallback_chain\":[]}",
+          "created_at": "2026-04-29T04:07:12.773902372Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=bragi model=qwen/qwen3.6-35b-a3b"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260429T033750-f9b149c5\",\"harness\":\"agent\",\"provider\":\"bragi\",\"model\":\"qwen/qwen3.6-35b-a3b\",\"input_tokens\":309650,\"output_tokens\":4859,\"total_tokens\":314509,\"cost_usd\":0,\"duration_ms\":1762081,\"exit_code\":0}",
+          "created_at": "2026-04-29T04:07:12.856047182Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=314509 model=qwen/qwen3.6-35b-a3b"
+        },
+        {
+          "actor": "erik",
+          "body": "tier=cheap harness=agent model=qwen/qwen3.6-35b-a3b probe=ok\nno_changes",
+          "created_at": "2026-04-29T04:07:12.974916702Z",
+          "kind": "tier-attempt",
+          "source": "ddx agent execute-loop",
+          "summary": "no_changes"
+        },
+        {
+          "actor": "erik",
+          "body": "{\"tiers_attempted\":[{\"tier\":\"cheap\",\"harness\":\"agent\",\"model\":\"qwen/qwen3.6-35b-a3b\",\"status\":\"no_changes\",\"cost_usd\":0,\"duration_ms\":1762081}],\"winning_tier\":\"exhausted\",\"total_cost_usd\":0,\"wasted_cost_usd\":0}",
+          "created_at": "2026-04-29T04:07:13.058844296Z",
+          "kind": "escalation-summary",
+          "source": "ddx agent execute-loop",
+          "summary": "winning_tier=exhausted attempts=1 total_cost_usd=0.0000 wasted_cost_usd=0.0000"
+        },
+        {
+          "actor": "erik",
+          "body": "no_changes\ntier=cheap\nprobe_result=ok\nresult_rev=59821a1864ed745965dc9d18bee96b5488c6e4c4\nbase_rev=59821a1864ed745965dc9d18bee96b5488c6e4c4\nretry_after=2026-04-29T10:07:13Z",
+          "created_at": "2026-04-29T04:07:13.413430653Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "no_changes"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"openrouter\",\"resolved_model\":\"openai/gpt-5.4-mini\",\"fallback_chain\":[]}",
+          "created_at": "2026-04-29T19:14:20.122768818Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=openrouter model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260429T190646-5aa5b46e\",\"harness\":\"agent\",\"provider\":\"openrouter\",\"model\":\"openai/gpt-5.4-mini\",\"input_tokens\":214497,\"output_tokens\":3515,\"total_tokens\":218012,\"cost_usd\":0.058149450000000005,\"duration_ms\":453380,\"exit_code\":0}",
+          "created_at": "2026-04-29T19:14:20.214872167Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=218012 cost_usd=0.0581 model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"escalation_count\":0,\"fallback_chain\":[],\"final_tier\":\"\",\"requested_profile\":\"\",\"requested_tier\":\"\",\"resolved_model\":\"openai/gpt-5.4-mini\",\"resolved_provider\":\"openrouter\",\"resolved_tier\":\"\"}",
+          "created_at": "2026-04-29T19:14:28.991242618Z",
+          "kind": "routing",
+          "source": "ddx agent execute-loop",
+          "summary": "provider=openrouter model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "erik",
+          "body": "The change only adds an execution metadata file. None of the required API contract documentation for audit writes, schema, idempotency, rejection behavior, retention/queryability, or examples is present, so the bead is not implemented.\nharness=codex\nmodel=gpt-5.4\ninput_bytes=7969\noutput_bytes=1060\nelapsed_ms=20119",
+          "created_at": "2026-04-29T19:14:49.308903516Z",
+          "kind": "review",
+          "source": "ddx agent execute-loop",
+          "summary": "BLOCK"
+        },
+        {
+          "actor": "",
+          "body": "",
+          "created_at": "2026-04-29T19:14:49.397118865Z",
+          "kind": "reopen",
+          "source": "",
+          "summary": "review: BLOCK"
+        },
+        {
+          "actor": "erik",
+          "body": "post-merge review: BLOCK (flagged for human)\nThe change only adds an execution metadata file. None of the required API contract documentation for audit writes, schema, idempotency, rejection behavior, retention/queryability, or examples is present, so the bead is not implemented.\nresult_rev=2d60be4cc173b0974a6aec51bc71f5ba57111e9b\nbase_rev=6b9d86b9a5fbc96f91d7442e66fa49efbf894e8b",
+          "created_at": "2026-04-29T19:14:49.485311813Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"openrouter\",\"resolved_model\":\"openai/gpt-5.4-mini\",\"fallback_chain\":[]}",
+          "created_at": "2026-04-29T23:52:40.953671154Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=openrouter model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"attempt_id\":\"20260429T235214-c5346b75\",\"harness\":\"agent\",\"provider\":\"openrouter\",\"model\":\"openai/gpt-5.4-mini\",\"input_tokens\":140194,\"output_tokens\":2527,\"total_tokens\":142721,\"cost_usd\":0.035646599999999994,\"duration_ms\":25513,\"exit_code\":0}",
+          "created_at": "2026-04-29T23:52:41.104092296Z",
+          "kind": "cost",
+          "source": "ddx agent execute-bead",
+          "summary": "tokens=142721 cost_usd=0.0356 model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"escalation_count\":0,\"fallback_chain\":[],\"final_tier\":\"\",\"requested_profile\":\"\",\"requested_tier\":\"\",\"resolved_model\":\"openai/gpt-5.4-mini\",\"resolved_provider\":\"openrouter\",\"resolved_tier\":\"\"}",
+          "created_at": "2026-04-29T23:52:44.28906517Z",
+          "kind": "routing",
+          "source": "ddx agent execute-loop",
+          "summary": "provider=openrouter model=openai/gpt-5.4-mini"
+        },
+        {
+          "actor": "erik",
+          "body": "The diff only adds execution metadata. None of the required audit-write API contract documentation or examples are present, so the bead is not implemented.\nharness=codex\nmodel=gpt-5.4\ninput_bytes=8243\noutput_bytes=885\nelapsed_ms=18709",
+          "created_at": "2026-04-29T23:53:03.201836761Z",
+          "kind": "review",
+          "source": "ddx agent execute-loop",
+          "summary": "BLOCK"
+        },
+        {
+          "actor": "",
+          "body": "",
+          "created_at": "2026-04-29T23:53:03.344296738Z",
+          "kind": "reopen",
+          "source": "",
+          "summary": "review: BLOCK"
+        },
+        {
+          "actor": "erik",
+          "body": "post-merge review: BLOCK (flagged for human)\nThe diff only adds execution metadata. None of the required audit-write API contract documentation or examples are present, so the bead is not implemented.\nresult_rev=f2f9fef466299a6342230815f83dd7c6b605ac87\nbase_rev=9e560d413a501061df1142df2b936f8167091fd8",
+          "created_at": "2026-04-29T23:53:03.478110766Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "review_block"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-04-29T23:54:06.393178879Z",
+      "execute-loop-last-detail": "no_changes",
+      "execute-loop-last-status": "no_changes",
+      "execute-loop-no-changes-count": 1,
+      "execute-loop-retry-after": "2026-04-29T10:07:13Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260429T235406-20eeaf2c",
+    "prompt": ".ddx/executions/20260429T235406-20eeaf2c/prompt.md",
+    "manifest": ".ddx/executions/20260429T235406-20eeaf2c/manifest.json",
+    "result": ".ddx/executions/20260429T235406-20eeaf2c/result.json",
+    "checks": ".ddx/executions/20260429T235406-20eeaf2c/checks.json",
+    "usage": ".ddx/executions/20260429T235406-20eeaf2c/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-db7a6d0a-20260429T235406-20eeaf2c"
+  }
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260429T235406-20eeaf2c/result.json b/.ddx/executions/20260429T235406-20eeaf2c/result.json
new file mode 100644
index 0000000..cc0c0bf
--- /dev/null
+++ b/.ddx/executions/20260429T235406-20eeaf2c/result.json
@@ -0,0 +1,23 @@
+{
+  "bead_id": "axon-db7a6d0a",
+  "attempt_id": "20260429T235406-20eeaf2c",
+  "base_rev": "337831991956adc3ef2f2aa838e8a31cff157735",
+  "result_rev": "4cb152558e1709f8ca197161349db7cbc75ac8ec",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "claude",
+  "model": "sonnet",
+  "session_id": "eb-c3810df4",
+  "duration_ms": 253321,
+  "tokens": 14779,
+  "cost_usd": 0.5719165,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260429T235406-20eeaf2c",
+  "prompt_file": ".ddx/executions/20260429T235406-20eeaf2c/prompt.md",
+  "manifest_file": ".ddx/executions/20260429T235406-20eeaf2c/manifest.json",
+  "result_file": ".ddx/executions/20260429T235406-20eeaf2c/result.json",
+  "usage_file": ".ddx/executions/20260429T235406-20eeaf2c/usage.json",
+  "started_at": "2026-04-29T23:54:07.670174892Z",
+  "finished_at": "2026-04-29T23:58:20.99201871Z"
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
