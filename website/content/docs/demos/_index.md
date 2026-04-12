---
title: Demo Reel
weight: 4
prev: /docs/cli
---

A full end-to-end walkthrough of Axon's core workflow, recorded live in Docker.

The demo covers:

1. **Start server** — in-process background server with in-memory storage
2. **Doctor** — verify server is reachable
3. **Create collections** — `tasks` and `projects`
4. **Define schema** — JSON Schema with required fields and enums
5. **Create entities** — four entities across two collections
6. **List and get** — read back what we created
7. **Update** — version auto-fetched, status changed to done
8. **Query with filter** — `--filter status=open` returns one result
9. **Set links** — `belongs-to` and `depends-on` edges
10. **List links** — outbound edges from a single entity
11. **Graph traversal** — depth-2 traversal shows the full graph
12. **Audit log** — five entries showing full mutation history
13. **Schema evolution** — add a compatible field, then force a breaking change
14. **Drop collection** — removes 3 entities, confirms cleanup

{{< asciinema src="quickstart" cols="100" rows="35" >}}

## Run it yourself

The demo script is included in the repository. Run it in Docker:

```bash
docker run --rm --entrypoint bash \
  ghcr.io/documentdrivendx/axon:latest \
  /scripts/demo.sh
```

Or clone and run locally (requires `axon` in `$PATH`):

```bash
git clone https://github.com/DocumentDrivenDX/axon.git
cd axon
bash scripts/demo.sh
```
