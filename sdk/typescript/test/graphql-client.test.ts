import { describe, it, expect } from "vitest";
import {
  AxonGraphQLClient,
  AxonGraphQLError,
  AxonGraphQLDocuments,
  buildAggregateDocument,
  buildEntityChangedSubscriptionDocument,
  collectionFieldName,
  pascalCase,
  type GraphQLFetchLike,
  type GraphQLFetchResponse,
  type PreviewMutationInput,
} from "../src/graphql-client.js";

function mockFetch(
  status: number,
  body: string,
  ok?: boolean,
): { mock: GraphQLFetchLike; calls: Array<[string, unknown]> } {
  const calls: Array<[string, unknown]> = [];
  const isOk = ok ?? (status >= 200 && status < 300);
  const mock: GraphQLFetchLike = async (url, init) => {
    calls.push([url, init]);
    const response: GraphQLFetchResponse = {
      ok: isOk,
      status,
      text: async () => body,
    };
    return response;
  };
  return { mock, calls };
}

function requestBody(call: [string, unknown]): { query: string; variables: Record<string, unknown> } {
  return JSON.parse((call[1] as { body: string }).body) as {
    query: string;
    variables: Record<string, unknown>;
  };
}

describe("GraphQL endpoint scoping", () => {
  it("routes tenant database requests to /tenants/:tenant/databases/:database/graphql", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"collections":[]}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170/",
      fetchImpl: mock,
      authToken: "tenant-token",
    });

    await client.tenant("acme").database("orders").collections();

    const [url, init] = calls[0];
    expect(url).toBe("http://localhost:4170/tenants/acme/databases/orders/graphql");
    expect((init as { method: string }).method).toBe("POST");
    expect((init as { headers: Record<string, string> }).headers.authorization).toBe(
      "Bearer tenant-token",
    );
  });

  it("keeps tenant and database isolation in the URL", () => {
    const client = new AxonGraphQLClient({ baseUrl: "https://axon.example" });
    expect(client.tenant("acme").database("orders").urlFor()).toBe(
      "https://axon.example/tenants/acme/databases/orders/graphql",
    );
    expect(client.tenant("beta").database("orders").urlFor()).toBe(
      "https://axon.example/tenants/beta/databases/orders/graphql",
    );
  });
});

describe("metadata and schema refresh", () => {
  it("uses GraphQL metadata and sends the expected schema hash header when supplied", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"collections":[]}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("orders").refreshSchema("fnv64:stale");

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.metadata);
    expect((calls[0][1] as { headers: Record<string, string> }).headers["x-axon-schema-hash"]).toBe(
      "fnv64:stale",
    );
  });
});

describe("CRUD and idempotency", () => {
  it("creates entities through GraphQL commitTransaction with body idempotency", async () => {
    const { mock, calls } = mockFetch(
      200,
      '{"data":{"commitTransaction":{"transactionId":"tx1","replayHit":false,"results":[]}}}',
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client
      .tenant("acme")
      .database("orders")
      .createEntity("items", "i-1", { sku: "SKU-1" }, { idempotencyKey: "retry-1" });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.commitTransaction);
    expect(body.variables).toEqual({
      input: {
        idempotencyKey: "retry-1",
        operations: [{ createEntity: { collection: "items", id: "i-1", data: { sku: "SKU-1" } } }],
      },
    });
  });

  it("generates filtered list and entity detail documents for generic CRUD reads", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"entities":{"edges":[]}}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("orders").listEntities("items", {
      filter: { status: { eq: "open" } },
      limit: 20,
      after: "i-1",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toContain("entities(collection: $collection");
    expect(body.variables).toMatchObject({
      collection: "items",
      filter: { status: { eq: "open" } },
      limit: 20,
      after: "i-1",
    });
  });
});

describe("GraphQL error mapping", () => {
  it("throws AxonGraphQLError with extensions for OCC conflicts", async () => {
    const { mock } = mockFetch(
      200,
      JSON.stringify({
        errors: [
          {
            message: "version conflict: expected 1, actual 2",
            extensions: { code: "VERSION_CONFLICT", expected: 1, actual: 2 },
          },
        ],
      }),
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await expect(
      client.tenant("acme").database("orders").getEntity("items", "i-1"),
    ).rejects.toBeInstanceOf(AxonGraphQLError);

    try {
      await client.tenant("acme").database("orders").getEntity("items", "i-1");
    } catch (error) {
      expect((error as AxonGraphQLError).code).toBe("VERSION_CONFLICT");
      expect((error as AxonGraphQLError).extensions).toMatchObject({
        expected: 1,
        actual: 2,
      });
    }
  });
});

describe("control plane helpers", () => {
  it("uses control-plane GraphQL for current user handshake", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"currentUser":{"actor":"ada"}}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.currentUser();

    const body = requestBody(calls[0]);
    expect(calls[0][0]).toBe("http://localhost:4170/control/graphql");
    expect(body.query).toBe(AxonGraphQLDocuments.currentUser);
  });

  it("uses /control/graphql for tenant/user/member/database administration", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"createTenant":{"id":"t1"}}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.control().createTenant("Acme");

    const body = requestBody(calls[0]);
    expect(calls[0][0]).toBe("http://localhost:4170/control/graphql");
    expect(body.query).toContain("createTenant(name: $name)");
    expect(body.variables).toEqual({ name: "Acme" });
  });

  it("lists credential metadata without selecting jwt secret material", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"credentials":[]}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.control().credentials("tenant-1");

    const body = requestBody(calls[0]);
    expect(body.query).toContain("credentials(tenantId: $tenantId)");
    expect(body.query).toContain("jti");
    expect(body.query).not.toContain("jwt");
  });
});

describe("relationship, aggregation, audit, lifecycle, and subscription helpers", () => {
  it("builds entity rollback mutation variables", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"rollbackEntity":{"dryRun":true}}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("orders").rollbackEntity("tasks", "task-1", {
      toVersion: 2,
      expectedVersion: 5,
      dryRun: true,
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.rollbackEntity);
    expect(body.variables).toEqual({
      input: {
        collection: "tasks",
        id: "task-1",
        toVersion: 2,
        expectedVersion: 5,
        dryRun: true,
      },
    });
  });

  it("builds link autocomplete, neighbors, and audit documents with variables", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });
    const db = client.tenant("acme").database("orders");

    await db.linkCandidates("users", "u1", "assigned-to", {
      search: "invoice",
      filter: { status: { eq: "open" } },
      limit: 10,
    });
    await db.neighbors("tasks", "task-1", { direction: "outbound", linkType: "depends-on" });
    await db.auditLog({ collection: "tasks", entityId: "task-1", operation: "entity.update" });

    expect(requestBody(calls[0]).query).toBe(AxonGraphQLDocuments.linkCandidates);
    expect(requestBody(calls[0]).variables).toMatchObject({
      sourceCollection: "users",
      sourceId: "u1",
      linkType: "assigned-to",
      search: "invoice",
      limit: 10,
    });
    expect(requestBody(calls[1]).query).toBe(AxonGraphQLDocuments.neighbors);
    expect(requestBody(calls[2]).query).toBe(AxonGraphQLDocuments.auditLog);
  });

  it("generates typed aggregation and lifecycle documents from collection names", async () => {
    const aggregate = buildAggregateDocument("time_entries", {
      filter: { status: { eq: "approved" } },
      groupBy: ["status"],
      aggregations: [
        { function: "COUNT" },
        { function: "SUM", field: "hours" },
      ],
    });

    expect(aggregate).toContain("timeEntriesAggregate");
    expect(aggregate).toContain("$filter: TimeEntriesFilter");
    expect(aggregate).toContain("groupBy: [status]");
    expect(aggregate).toContain("{ function: SUM, field: hours }");

    expect(pascalCase("time_entries")).toBe("TimeEntries");
    expect(collectionFieldName("auditLog")).toBe("auditlog");
  });

  it("exposes subscription URL and generic or typed subscription documents", () => {
    const client = new AxonGraphQLClient({ baseUrl: "https://axon.example" });
    const db = client.tenant("acme").database("orders");

    expect(db.subscriptionUrl()).toBe(
      "wss://axon.example/tenants/acme/databases/orders/graphql/ws",
    );
    expect(buildEntityChangedSubscriptionDocument()).toContain("entityChanged");
    expect(db.entityChangedSubscription("time_entries")).toContain("timeEntriesChanged");
  });
});

describe("governed-workflow methods (CONTRACT-009)", () => {
  it("previewMutation sends the previewMutation mutation with input variable @covers US-105-AC6", async () => {
    const { mock, calls } = mockFetch(
      200,
      '{"data":{"previewMutation":{"decision":"allow","intentToken":"tok-1","intent":{"id":"i-1","approvalState":"none","decision":"allow"}}}}',
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });
    const input: PreviewMutationInput = {
      operation: {
        operationKind: "patch_entity",
        operation: {
          collection: "tasks",
          id: "task-1",
          expected_version: 1,
          patch: { status: "approved" },
        },
      },
      expiresInSeconds: 600,
    };

    await client.tenant("acme").database("default").previewMutation(input);

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.previewMutation);
    expect(body.variables).toEqual({ input });
  });

  it("commitIntent sends commitMutationIntent mutation with intentToken @covers US-107-AC6", async () => {
    const { mock, calls } = mockFetch(
      200,
      '{"data":{"commitMutationIntent":{"committed":true,"transactionId":"tx-1","intent":{"id":"i-1","approvalState":"committed","decision":"allow"}}}}',
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("default").commitIntent({ intentToken: "tok-1" });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.commitMutationIntent);
    expect(body.variables).toEqual({ input: { intentToken: "tok-1" } });
  });

  it("approveIntent sends approveMutationIntent mutation with intentId and reason @covers US-106-AC4", async () => {
    const { mock, calls } = mockFetch(
      200,
      '{"data":{"approveMutationIntent":{"id":"i-1","approvalState":"approved","decision":"needs_approval"}}}',
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("default").approveIntent({
      intentId: "i-1",
      reason: "approved for release",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.approveMutationIntent);
    expect(body.variables).toEqual({ input: { intentId: "i-1", reason: "approved for release" } });
  });

  it("rejectIntent sends rejectMutationIntent mutation with intentId and reason @covers US-106-AC4", async () => {
    const { mock, calls } = mockFetch(
      200,
      '{"data":{"rejectMutationIntent":{"id":"i-2","approvalState":"rejected","decision":"needs_approval"}}}',
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("default").rejectIntent({
      intentId: "i-2",
      reason: "insufficient justification",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.rejectMutationIntent);
    expect(body.variables).toEqual({
      input: { intentId: "i-2", reason: "insufficient justification" },
    });
  });

  it("explainPolicy sends explainPolicy query with input variable", async () => {
    const { mock, calls } = mockFetch(
      200,
      '{"data":{"explainPolicy":{"operation":"update","collection":"tasks","decision":"needs_approval","reason":"needs_approval","policyVersion":1}}}',
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("default").explainPolicy({
      operation: "update",
      collection: "tasks",
      entityId: "task-1",
      data: { budget_cents: 20000 },
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.explainPolicy);
    expect(body.variables).toEqual({
      input: {
        operation: "update",
        collection: "tasks",
        entityId: "task-1",
        data: { budget_cents: 20000 },
      },
    });
  });

  it("queryAudit delegates to auditLog with the same document and variables", async () => {
    const { mock, calls } = mockFetch(200, '{"data":{"auditLog":{"totalCount":0,"edges":[]}}}');
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("default").queryAudit({
      collection: "tasks",
      entityId: "task-1",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.auditLog);
    expect(body.variables).toMatchObject({ collection: "tasks", entityId: "task-1" });
  });

  it("rollbackDryRun calls rollbackEntity with dryRun forced to true", async () => {
    const { mock, calls } = mockFetch(
      200,
      '{"data":{"rollbackEntity":{"dryRun":true,"entity":{"id":"task-1","version":1}}}}',
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("default").rollbackDryRun({
      collection: "tasks",
      id: "task-1",
      toVersion: 2,
      expectedVersion: 5,
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.rollbackEntity);
    expect(body.variables).toEqual({
      input: {
        collection: "tasks",
        id: "task-1",
        toVersion: 2,
        toAuditId: undefined,
        expectedVersion: 5,
        dryRun: true,
      },
    });
  });
});
