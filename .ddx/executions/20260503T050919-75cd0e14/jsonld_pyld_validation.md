JSON-LD validation evidence for `axon-84088cbe`.

Command:

```bash
/tmp/axon-pyld-venv/bin/python - <<'PY'
from pyld import jsonld

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
PY
```

Output:

```text
pyld_expand_ok 1
```
