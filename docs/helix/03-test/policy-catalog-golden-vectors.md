---
ddx:
  id: POLICY-CATALOG-GOLDEN-VECTORS
  depends_on:
    - CONTRACT-004
    - ADR-018
    - ADR-019
    - helix.security-requirements
    - FEAT-012
    - FEAT-029
    - FEAT-030
  review:
    reviewed_at: "2026-07-11T02:44:22Z"
    self_hash: 5e5254adc7975a330ca49a120d3f143855c225271747603fac69d0c9071da11b
    deps:
      ADR-018: 6282a6ac66a0dcfd400663681132c9f5f85ed7c78793a1cf7f8bf06853cf1d97
      ADR-019: 3ec156d9ec6696d67e0f12a6c80495c9166470525128ac475b95dae0b5647f7e
      CONTRACT-004: 44362ccf2c383194383cf49261736b27869dc8cfe54c7bdfa4e4ac50ccc46a10
      FEAT-012: d37c0b05aaef5e6da2c11ad0f7433660198cf96113dec4bf07fee4e095521eea
      FEAT-029: f548dd83b06d298a7e8c575870ae1a06e5e9c53e94d6ccb64b2b876daf7b3b0c
      FEAT-030: 81a89ddb42efe517ddde6ea7481c104b3600481a32072e31bd9d94cd7294922d
      helix.security-requirements: 09d5e34b217554b4c87407160c5102d800cf64c6260d7141768498e8ac76f58a
---

# Policy Catalog Golden Vectors

## Purpose

This document freezes the golden vectors for the adapter-owned
`policy_catalog` model before implementation reaches the policy and intent
execution phases. The canonical content-addressed form is
`AXON-POLICY-HASH-1`, computed with SHA-256 over AXON-CJSON-1 bytes after
normalization.

## Golden Vectors

| Vector | Change shape | Expected epoch/hash outcome |
|---|---|---|
| `implicit.vs.explicit-scope` | Request without an explicit auth scope versus the same request with an explicit `auth_scope_required` policy row | The explicit row is selected; the implicit/no-scope request fails closed with `policy_catalog_missing` |
| `structural.only` | Whitespace, YAML key order, comments, anchors, or other source-only formatting changes | `normalized_ast` stays byte-equivalent; `policy_epoch` and `policy_hash` stay unchanged |
| `policy.only` | A semantic policy change such as a rule predicate, default, or collection ordering change | `policy_epoch` increments once; `policy_hash` changes; `auth_epoch` is unchanged |
| `create` | First policy row for a fresh tenant/database pair | The catalog starts at epoch 1 with the empty/default normalized AST and a stable `policy_hash` |
| `delete` | Delete or deactivate the only policy row for a database | The next evaluation fails closed with `policy_catalog_missing`; there is no fallback to a stale `__axon_policies__` surface |
| `rollback` | Roll back to the prior semantic policy state | The rollback is represented as a new catalog write; `policy_epoch` advances once and the hash returns to the previous canonical value |
| `retry` | Replay the same semantic policy write after timeout or duplicate submission | The retry is idempotent; the hash/epoch pair does not double-advance and `auth_epoch` does not change |
| `multi.record` | One transaction mutates multiple records in the same tenant, such as policy plus membership or credential state | The tenant `auth_epoch` increments once; the database `policy_epoch` increments once if the catalog changed; partial visibility is forbidden |

## Normalization Notes

- `format_version` is `1`.
- `collections` are sorted lexicographically by collection name.
- `normalized_ast` must include explicit defaults for omitted booleans,
  empty lists, and nullable values so equivalent source forms hash
  identically.
- `AXON-POLICY-HASH-1` is the SHA-256 digest of the AXON-CJSON-1 encoded
  canonical manifest object described in CONTRACT-004.
