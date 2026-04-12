---
title: Core Concepts
weight: 2
prev: /docs/getting-started
next: /docs/cli
---

Axon models the world as **entities** organized into **collections**, connected by **links**, governed by **schemas**, and traced by an **audit log**.

{{< cards >}}
  {{< card link="collections" title="Collections" subtitle="Named containers for entities. Each collection has its own schema, indexes, and access rules." >}}
  {{< card link="entities" title="Entities" subtitle="JSON documents with a stable ID, versioned history, and schema validation on every write." >}}
  {{< card link="schema" title="Schema" subtitle="JSON Schema per collection. Defines shape, constraints, and valid values — enforced on every write." >}}
  {{< card link="links" title="Links & Graph" subtitle="Typed edges connecting entities across collections. Traverse to arbitrary depth." >}}
  {{< card link="audit-log" title="Audit Log" subtitle="Immutable record of every mutation — who changed what, when, and what it looked like before." >}}
  {{< card link="authentication" title="Authentication & Authorization" subtitle="Tailscale-based identity, RBAC roles, ACL tag mapping, and the /auth/me endpoint." >}}
{{< /cards >}}
