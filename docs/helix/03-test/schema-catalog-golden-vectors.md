---
ddx:
  id: SCHEMA-CATALOG-GOLDEN-VECTORS
  depends_on:
    - CONTRACT-001
    - CONTRACT-004
    - CONTRACT-010
    - FEAT-002
    - FEAT-017
    - ADR-007
  review:
    self_hash: d5b6e9833b49ba599d40cfd927877ff4d6091c6382dad02d653b3c09736b6482
    deps:
      ADR-007: 5a96b23ec82c256af094753065c60c6862a9a7c2fd8e7db3bb681d896627f727
      CONTRACT-001: dba7833be1c4bdf643ff7c45c7f3faac5224013a7d4855ac8769067505bd453e
      CONTRACT-004: 44362ccf2c383194383cf49261736b27869dc8cfe54c7bdfa4e4ac50ccc46a10
      CONTRACT-010: 9250599003d21f3885a52eb67ad688139715e9aa0497bf1634ad27e2d505e134
      FEAT-002: 84f680ec396f34b25b2a91172d8cab7a8e9204817430b9e3aa8f9ec1ee3afd03
      FEAT-017: 2651b9abea59b48d54097bc01b2643320f9b6cc04b7101f2ebb964a89bca1ff9
    reviewed_at: "2026-07-11T03:00:17Z"
---
# Schema Catalog Golden Vectors

## Purpose

This document freezes the golden vectors for the whole-catalog structural hash
used by schema activation. The canonical preimage is `StructuralSchemaV1`,
serialized with AXON-CJSON-1 and hashed with SHA-256 as
`AXON-SCHEMA-CATALOG-HASH-1`.

The vectors must pass on the memory, SQLite, and PostgreSQL adapters and
through the `/schema` activation surface so the active-version binding stays
identical across backends.

## Golden Vectors

| Vector | Change shape | Expected outcome |
|---|---|---|
| `structural.only` | Reorder collection definitions, change descriptions or timestamps, or reorder link declarations without changing semantics | Active `AXON-SCHEMA-CATALOG-HASH-1` stays unchanged; policy hash stays unchanged; inactive history is excluded |
| `policy.only` | Change `access_control` or other policy-only metadata without touching the structural schema | Structural hash stays unchanged; policy hash changes; activation still sees the same active schema hash |
| `inactive.history` | Add or mutate historical schema rows without changing the active version | Whole-catalog hash stays unchanged; history is not part of the preimage |
| `whole.catalog` | Add or remove a collection or alter target/cardinality/required/metadata/default semantics in the active schema | Structural hash changes once; activation must OCC-rotate the active version and hash together |
| `surface` | Read the same canonical active schema through memory, SQLite, PostgreSQL, and `/schema` | Each surface reports the same active `StructuralSchemaV1` and `AXON-SCHEMA-CATALOG-HASH-1` |
| `retry` | Replay the same canonical activation snapshot after timeout or duplicate request | Idempotent result; hash/version pair does not double-advance |

## Normalization Notes

- `format_version` is `1`.
- `tenant_id`, `database_id`, and the active version are part of the canonical
  preimage.
- `collections` are sorted by qualified collection name and include only the
  active structural declarations.
- `description`, `timestamp`, inactive history, and policy data are excluded
  from the structural hash preimage.
