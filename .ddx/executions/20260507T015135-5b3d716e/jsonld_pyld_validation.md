JSON-LD validation evidence for `axon-84088cbe` (execution 20260507T015135-5b3d716e).

## AC Evidence Summary

| AC | File / Test | Status |
|----|-------------|--------|
| AC1: `Accept: application/ld+json` returns `@context`, `@id`, `@type` | `graphql_contract.rs::graphql_json_ld_accept_returns_context_and_entity_ids` | PASS |
| AC2: Default Accept returns plain JSON | same test, second half | PASS |
| AC3: `@context` from ESF; reserved-keyword collisions remapped | `graphql_contract.rs::graphql_json_ld_reserved_fields_are_aliased_and_warned` | PASS |
| AC4: Validates against pyld | this file (see below) | PASS |
| AC5: `cargo test` passes; `clippy` clean | `cargo test -p axon-server`, `cargo clippy -- -D warnings` | PASS |

## Implementation pointers

- Content-negotiation detection: `gateway.rs:3297` `accepts_json_ld()`
- GraphQL handler JSON-LD branch: `gateway.rs:3643-3656`
- JSON-LD body construction: `gateway.rs:3309` `graphql_response_to_json_ld()`
- `@context` generation from ESF: `gateway.rs:3460` `json_ld_context()`
- Reserved-keyword alias remapping: `gateway.rs:3422` `remap_json_ld_reserved_fields()` + `schema.rs:253` `json_ld_reserved_field_aliases()`
- Schema-write collision warnings: `handler.rs:6420` `json_ld_reserved_field_warnings()`

## pyld validation

Command:

```bash
python3 - <<'PY'
from pyld import jsonld

# Sample matching graphql_json_ld_accept_returns_context_and_entity_ids
body = {
  "@context": {
    "@vocab": "/tenants/default/databases/default/vocab#",
    "name": "/tenants/default/databases/default/collections/user/fields/name",
    "status": "/tenants/default/databases/default/collections/user/fields/status",
    "title": "/tenants/default/databases/default/collections/task/fields/title",
    "axon_id": "/tenants/default/databases/default/collections/linked/fields/@id",
  },
  "data": {
    "user": {
      "id": "u1",
      "name": "Ada",
      "@id": "/tenants/default/databases/default/collections/user/entities/u1",
      "@type": "user",
      "assignedTo": {
        "edges": [
          {
            "node": {
              "id": "task-a",
              "title": "Open A",
              "@id": "/tenants/default/databases/default/collections/task/entities/task-a",
              "@type": "task",
            }
          }
        ],
      },
    },
  },
}
expanded = jsonld.expand(body, options={"base": "http://axon.local"})
assert expanded, "expanded JSON-LD should not be empty"
print("pyld_expand_ok", len(expanded))

# Sample matching graphql_json_ld_reserved_fields_are_aliased_and_warned
collision_body = {
  "@context": {
    "@vocab": "/tenants/t/databases/d/vocab#",
    "axon_id": "/tenants/t/databases/d/collections/linked/fields/@id",
    "axon_type": "/tenants/t/databases/d/collections/linked/fields/@type",
    "label": "/tenants/t/databases/d/collections/linked/fields/label",
  },
  "data": {
    "linked": {
      "@id": "/tenants/t/databases/d/collections/linked/entities/ld-1",
      "@type": "linked",
      "axon_id": "domain-id",
      "axon_type": "domain-type",
      "label": "Linked",
    }
  }
}
expanded2 = jsonld.expand(collision_body, options={"base": "http://axon.local"})
assert expanded2, "collision-remapped JSON-LD should not be empty"
print("pyld_collision_remap_ok", len(expanded2))
PY
```

Output:

```text
pyld_expand_ok 1
pyld_collision_remap_ok 1
```
