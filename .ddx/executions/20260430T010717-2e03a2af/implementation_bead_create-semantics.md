<execute-bead>
  <bead id="axon-create-semantics-impl-1">
    <title>implement create semantics decision: document strict GraphQL/transaction create vs HTTP/gRPC upsert</title>
    <description>
Implement the decision from docs/helix/02-design/decisions/create-semantics.md. Ensure the strict duplicate-rejecting semantics are documented and tested for typed GraphQL createXxx and commitTransaction op:create, while HTTP /entities POST and gRPC CreateEntity remain overwrite/upsert as currently implemented.

Preferred scope: docs + tests only unless a small code clarification is needed to align observable behavior with the decision. If code changes are required, keep them minimal and update/add contract tests to cover all surfaces in the survey table.
    </description>
    <acceptance>
AC1. Decision doc exists and the implementation bead targets the chosen pattern.
AC2. Tests/documentation explicitly reflect current create behavior across typed GraphQL, commitTransaction, HTTP, gRPC, and storage.
AC3. No silent behavior drift across transports.
    </acceptance>
  </bead>
</execute-bead>
