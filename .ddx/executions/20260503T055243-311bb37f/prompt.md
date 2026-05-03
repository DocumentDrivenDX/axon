<bead-review>
  <bead id="axon-8cdc4e49" iter=1>
    <title>build(feat-003): PROV-O audit serialization (additive)</title>
    <description>
Per FEAT-003 US-010 and ADR-020 §RDF concept adoption.

Add additive PROV-O serialization to audit queries. Native JSON audit shape unchanged; PROV-O surfaces via Accept: application/ld+json with PROV @context, or via ?format=prov query parameter.

Mapping: operation→prov:Activity; affected entity/link→prov:Entity (note: PROV-O 'Entity' broader than Axon's, see FEAT-003 US-010 naming-clash note); actor→prov:Agent; before→prov:used; after→prov:wasGeneratedBy; actor↔activity→prov:wasAssociatedWith; transaction chain→prov:wasInformedBy; timestamp→prov:startedAtTime/endedAtTime.

Subject IRIs use canonical entity URLs per ADR-020.
    </description>
    <acceptance>
AC1. Audit query supports content negotiation (Accept: application/ld+json with PROV @context) and ?format=prov. AC2. Native JSON audit shape unchanged. AC3. PROV-O output validates against the official PROV-O ontology. AC4. Round-trip test: native JSON → PROV-O → re-import preserves all auditable facts. AC5. cargo test passes; clippy clean.
    </acceptance>
    <labels>helix, feat-003, area:audit, kind:feature</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260503T053312-7be85d71/manifest.json</file>
    <file>.ddx/executions/20260503T053312-7be85d71/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="f670ef3dd45e3a15dc603d30fe1627484df391e9">
diff --git a/.ddx/executions/20260503T053312-7be85d71/manifest.json b/.ddx/executions/20260503T053312-7be85d71/manifest.json
new file mode 100644
index 0000000..baac1b4
--- /dev/null
+++ b/.ddx/executions/20260503T053312-7be85d71/manifest.json
@@ -0,0 +1,72 @@
+{
+  "attempt_id": "20260503T053312-7be85d71",
+  "bead_id": "axon-8cdc4e49",
+  "base_rev": "164bb3a4aa4ce8b17fd5d7f85e95b103d1089278",
+  "created_at": "2026-05-03T05:33:12.910123767Z",
+  "requested": {
+    "harness": "codex",
+    "prompt": "synthesized"
+  },
+  "bead": {
+    "id": "axon-8cdc4e49",
+    "title": "build(feat-003): PROV-O audit serialization (additive)",
+    "description": "Per FEAT-003 US-010 and ADR-020 §RDF concept adoption.\n\nAdd additive PROV-O serialization to audit queries. Native JSON audit shape unchanged; PROV-O surfaces via Accept: application/ld+json with PROV @context, or via ?format=prov query parameter.\n\nMapping: operation→prov:Activity; affected entity/link→prov:Entity (note: PROV-O 'Entity' broader than Axon's, see FEAT-003 US-010 naming-clash note); actor→prov:Agent; before→prov:used; after→prov:wasGeneratedBy; actor↔activity→prov:wasAssociatedWith; transaction chain→prov:wasInformedBy; timestamp→prov:startedAtTime/endedAtTime.\n\nSubject IRIs use canonical entity URLs per ADR-020.",
+    "acceptance": "AC1. Audit query supports content negotiation (Accept: application/ld+json with PROV @context) and ?format=prov. AC2. Native JSON audit shape unchanged. AC3. PROV-O output validates against the official PROV-O ontology. AC4. Round-trip test: native JSON → PROV-O → re-import preserves all auditable facts. AC5. cargo test passes; clippy clean.",
+    "labels": [
+      "helix",
+      "feat-003",
+      "area:audit",
+      "kind:feature"
+    ],
+    "metadata": {
+      "claimed-at": "2026-05-03T05:33:12Z",
+      "claimed-machine": "sindri",
+      "claimed-pid": "3908734",
+      "events": [
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"vidar\",\"resolved_model\":\"MiniMax-M2.5-MLX-4bit\",\"fallback_chain\":[],\"requested_model\":\"MiniMax-M2.5-MLX-4bit\"}",
+          "created_at": "2026-05-02T20:56:33.375433076Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider=vidar model=MiniMax-M2.5-MLX-4bit"
+        },
+        {
+          "actor": "ddx",
+          "body": "agent: provider error: openai: POST \"http://vidar:1235/v1/chat/completions\": 507 Insufficient Storage {\"message\":\"Model 'MiniMax-M2.5-MLX-4bit' (125.82GB) exceeds max-model-memory (64.00GB)\",\"type\":\"server_error\",\"param\":null,\"code\":null}\nresult_rev=6d5dfbafb3db6968929dfcd416abd29a9eab1618\nbase_rev=6d5dfbafb3db6968929dfcd416abd29a9eab1618\nretry_after=2026-05-03T02:56:33Z",
+          "created_at": "2026-05-02T20:56:33.831511816Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        },
+        {
+          "actor": "ddx",
+          "body": "{\"resolved_provider\":\"\",\"fallback_chain\":[]}",
+          "created_at": "2026-05-03T02:43:16.674712163Z",
+          "kind": "routing",
+          "source": "ddx agent execute-bead",
+          "summary": "provider="
+        },
+        {
+          "actor": "ddx",
+          "body": "ResolveRoute: no viable routing candidate: 4 candidates rejected\nresult_rev=7cbb0c47522c2f2b5f2f0f1d9e9746faaf2fee25\nbase_rev=7cbb0c47522c2f2b5f2f0f1d9e9746faaf2fee25\nretry_after=2026-05-03T08:43:16Z",
+          "created_at": "2026-05-03T02:43:17.079518652Z",
+          "kind": "execute-bead",
+          "source": "ddx agent execute-loop",
+          "summary": "execution_failed"
+        }
+      ],
+      "execute-loop-heartbeat-at": "2026-05-03T05:33:12.301869125Z"
+    }
+  },
+  "paths": {
+    "dir": ".ddx/executions/20260503T053312-7be85d71",
+    "prompt": ".ddx/executions/20260503T053312-7be85d71/prompt.md",
+    "manifest": ".ddx/executions/20260503T053312-7be85d71/manifest.json",
+    "result": ".ddx/executions/20260503T053312-7be85d71/result.json",
+    "checks": ".ddx/executions/20260503T053312-7be85d71/checks.json",
+    "usage": ".ddx/executions/20260503T053312-7be85d71/usage.json",
+    "worktree": "tmp/ddx-exec-wt/.execute-bead-wt-axon-8cdc4e49-20260503T053312-7be85d71"
+  },
+  "prompt_sha": "ac1394c1e6fd30377aa1720d3ff63960d2e783d056676f78ca6e3fa53f4db122"
+}
\ No newline at end of file
diff --git a/.ddx/executions/20260503T053312-7be85d71/result.json b/.ddx/executions/20260503T053312-7be85d71/result.json
new file mode 100644
index 0000000..d2b7489
--- /dev/null
+++ b/.ddx/executions/20260503T053312-7be85d71/result.json
@@ -0,0 +1,21 @@
+{
+  "bead_id": "axon-8cdc4e49",
+  "attempt_id": "20260503T053312-7be85d71",
+  "base_rev": "164bb3a4aa4ce8b17fd5d7f85e95b103d1089278",
+  "result_rev": "25876f263c1cf0bd83761d364d6e41242f3f3d2f",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "codex",
+  "session_id": "eb-d5b31628",
+  "duration_ms": 1161199,
+  "tokens": 8448757,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260503T053312-7be85d71",
+  "prompt_file": ".ddx/executions/20260503T053312-7be85d71/prompt.md",
+  "manifest_file": ".ddx/executions/20260503T053312-7be85d71/manifest.json",
+  "result_file": ".ddx/executions/20260503T053312-7be85d71/result.json",
+  "usage_file": ".ddx/executions/20260503T053312-7be85d71/usage.json",
+  "started_at": "2026-05-03T05:33:12.911028325Z",
+  "finished_at": "2026-05-03T05:52:34.110335189Z"
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
