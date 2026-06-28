I have enough to complete the adversarial review. Let me record one cross-cutting finding to memory check — actually, I have everything needed. Here is the review.

## Adversarial Implementation Review — `axon-2811c376`

I reviewed the working tree (`axon-esf` crate, `axon-schema` integration, `Cargo.toml` changes) against the round-8 plan and the surrounding code. I read every changed file, traced the serde behavior, verified jsonschema is used in only `axon-esf` + `axon-schema`, confirmed the CLI's `.json()` usage, and searched the repo for remote/file `$ref` reliance.

### Findings

| Severity | Area | Finding |
|---|---|---|
| WARNING | validation consistency | `validate_entity` now compiles via `CompiledSchema` with `should_validate_formats(true)` (`crates/axon-esf/src/validation.rs:75`), but `validate_link_metadata` keeps its own direct `jsonschema::options()` builder **without** format assertions (`crates/axon-schema/src/validation.rs:142-145`). Result: entity bodies now reject malformed `email`/`uuid`/`date-time`, while link-metadata bodies silently do not. This asymmetry is a real runtime behavior split that the plan did not explicitly reconcile. Non-blocking (plan keeps link-metadata shape unchanged), but worth a conscious sign-off. |
| NOTE | runtime semantics | Enabling format assertions changes **production** entity validation, not just tests — previously-accepted entity bodies with technically-malformed declared formats will now be rejected by the live `axon-api` handler path (`handler.rs` uses `validate_entity`). The plan marks this "intentional and required," and the bead aligns; flagging only so the operational impact is acknowledged before commit. |
| NOTE | serde / unknown keys | `reject_unknown_keys` uses a single shared allow-list `["field","fields","type","unique"]` (`crates/axon-esf/src/types.rs:84`). Because `IndexDef`/`CompoundIndexDef` are not `deny_unknown_fields`, a compound declaration carrying a stray `type` (e.g. `{"fields":[…],"type":"string"}`) passes the gate and silently drops `type` during `serde_json::from_value`. Minor deviation from the "reject unknown keys" intent; not a storage/correctness bug since the compound still parses correctly. The existing rejection tests don't cover this case. |
| NOTE | feature loss scope | Disabling `jsonschema` default features also removes `resolve-http`/`resolve-file` from the `validate_link_metadata` path, not just entity validation. Repo-wide search found no JSON-Schema `$ref` pointing at `http(s)://`/`file://` in any `.rs`/`.yaml`/`.json`, so the accepted loss has no in-repo consumer today. |

### Verified clean (no finding)

- **Public API shape** of `axon-esf` matches the plan exactly (`lib.rs` re-exports the 12 named items).
- **Index value-kinds preserved**: `IndexType` is `String/Integer/Float/Datetime/Boolean` with `rename_all="lowercase"` + matching `Display`, moved without semantic change; `axon-schema` re-exports them so existing `IndexType::String`/`::Datetime` callers (e.g. `axon-api/src/handler.rs:16710`) compile unchanged.
- **`IndexDeclaration` inside flattened carriers**: the custom `Deserialize` routes through `serde_json::Value`, which works under serde's `flatten` `Content` buffering — directly exercised by `document_carriers_preserve_extra_and_legacy_compound_indexes` (EsfCoreDocument with `indexes` + `extra`), and via serde_yaml in `axon-schema`'s `esf_description_and_indexes_convert_to_collection_schema`. Both pass.
- **Enhanced `axon-schema` errors retain required data**: `RawValidationError{instance_path, message, instance}` carries everything `enhance_raw_schema_error` and `classify_json_schema_error` need; `message` is exactly `ValidationError::to_string()` with no prefix, and `field_path` is rebuilt from `instance_path` exactly as before.
- **No dependency leakage**: `axon-esf` declares only `serde`/`serde_json`/`jsonschema`; the negative `cargo tree` check (already run) confirms no `axon-*`/`reqwest`/`hyper`/`tower` edges.
- **CLI reqwest change justified and sufficient**: `client.rs` uses `.json(&body)` in 8 places; `reqwest` is enabled via the default `serve` feature, so `cargo check` exercised it. Adding `json` replaces the feature previously unified in transitively through `jsonschema`'s now-removed `reqwest` dependency. No other crate depends on `jsonschema`, so the global default-features change is contained.
- **`EsfDocument`** correctly gains `description`/`indexes`/`compound_indexes`, splits unified `indexes`, appends legacy `compound_indexes`, preserves order/duplicates, and now forwards `description` (previously hard-coded `None`).

### Verdict: APPROVE

### Summary

The implementation faithfully executes the round-8 plan: the leaf crate's API, the preserved index value-kinds, the flatten-safe `IndexDeclaration` deserializer, and the contained `jsonschema` default-feature change all hold up under scrutiny, and the CLI `reqwest` json fix is both justified and sufficient. No blocking correctness or regression issues were found; the prior `cargo check`/`clippy`/`fmt` and crate-level test passes are consistent with my reading. The only items worth a conscious sign-off before commit are the entity-vs-link-metadata format-assertion asymmetry and the broadened runtime rejection of malformed formats — both are consequences the plan explicitly accepts, not defects.