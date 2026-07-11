# Bead review: axon-gap-closure-0c80b54e (round 1)

- Reviewed implementation: `f52e92663e4f778aff3e2406b14dde7cad51879a`
- Review route: DDx `codex/gpt-5.5`
- Verdict: `BLOCK`

```json
{
  "schema_version": 1,
  "verdict": "BLOCK",
  "summary": "Mechanical gates are credible, but the test-fixtures feature still exposes public unchecked mutation helpers to release consumers, leaving a governed-handler bypass unsealed.",
  "per_ac": [
    {
      "number": 1,
      "item": "The real external-consumer compile-fail suite passes.",
      "grade": "fail",
      "evidence": "The suite passes its listed cases, but its generated manifest does not enable `test-fixtures`; public unchecked fixture helpers remain exposed through that feature."
    },
    {
      "number": 2,
      "item": "The forbidden public-export scan passes.",
      "grade": "pass",
      "evidence": "The exact scan has no matches; handler extraction methods are crate-local test-only and StorageCursorStore is no longer root-re-exported."
    },
    {
      "number": 3,
      "item": "The whole workspace compiles after visibility changes.",
      "grade": "pass",
      "evidence": "The exact workspace no-run command passed."
    },
    {
      "number": 4,
      "item": "Focused clippy passes.",
      "grade": "pass",
      "evidence": "The exact clippy command passed."
    },
    {
      "number": 5,
      "item": "Formatting passes.",
      "grade": "pass",
      "evidence": "The exact formatting command passed."
    }
  ],
  "findings": [
    {
      "severity": "block",
      "summary": "`axon_api::test_fixtures` exposes unchecked helpers backed by raw storage mutation whenever the feature is enabled, allowing a release consumer to bypass governed policy.",
      "location": "crates/axon-api/src/test_fixtures.rs:27"
    },
    {
      "severity": "block",
      "summary": "`axon-server` enables `axon-api/test-fixtures` as a normal dependency, so production workspace builds activate the public fixture surface.",
      "location": "crates/axon-server/Cargo.toml:15"
    },
    {
      "severity": "warn",
      "summary": "The compile-fail harness does not exercise `axon-api` with `features = [\"test-fixtures\"]`.",
      "location": "crates/axon-api/tests/raw_access_compile_fail.rs:199"
    }
  ]
}
```
