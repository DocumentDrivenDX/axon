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

async function expectGraphQLError(
  promise: Promise<unknown>,
  expectedCode: string,
  expectedExtensions: Record<string, unknown>,
): Promise<void> {
  try {
    await promise;
    throw new Error(`expected GraphQL error code ${expectedCode}`);
  } catch (error) {
    expect(error).toBeInstanceOf(AxonGraphQLError);
    const gqlError = error as AxonGraphQLError;
    expect(gqlError.code).toBe(expectedCode);
    expect(gqlError.extensions).toMatchObject(expectedExtensions);
  }
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

describe("governed-workflow methods and outcomes (CONTRACT-009)", () => {
  it("previewMutation preserves needs_approval GraphQL outcome vocabulary @covers US-105-AC6", async () => {
    const previewResponse = {
      decision: "needs_approval",
      intentToken: "tok-approval",
      approvalRoute: { role: "finance-approver" },
      intent: { id: "i-1", approvalState: "pending", decision: "needs_approval" },
    };
    const { mock, calls } = mockFetch(
      200,
      JSON.stringify({ data: { previewMutation: previewResponse } }),
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

    const result = await client.tenant("acme").database("default").previewMutation(input);

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.previewMutation);
    expect(body.variables).toEqual({ input });
    expect(result).toEqual({ previewMutation: previewResponse });
  });

  it("commitIntent returns committed GraphQL success payload @covers US-107-AC6", async () => {
    const commitResponse = {
      committed: true,
      transactionId: "tx-1",
      errorCode: null,
      stale: null,
      intent: { id: "i-1", approvalState: "committed", decision: "allow" },
    };
    const { mock, calls } = mockFetch(
      200,
      JSON.stringify({ data: { commitMutationIntent: commitResponse } }),
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    const result = await client.tenant("acme").database("default").commitIntent({
      intentToken: "tok-1",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.commitMutationIntent);
    expect(body.variables).toEqual({ input: { intentToken: "tok-1" } });
    expect(result).toEqual({ commitMutationIntent: commitResponse });
  });

  it.each([
    {
      label: "intent_stale",
      expectedCode: "intent_stale",
      response: {
        errors: [
          {
            message: "intent stale",
            extensions: {
              code: "intent_stale",
              stale: [
                {
                  dimension: "pre_image",
                  expected: 1,
                  actual: 2,
                  path: "task-a",
                },
              ],
            },
          },
        ],
      },
      expectedExtensions: {
        code: "intent_stale",
        stale: [
          {
            dimension: "pre_image",
            expected: 1,
            actual: 2,
            path: "task-a",
          },
        ],
      },
    },
    {
      label: "intent_mismatch",
      expectedCode: "intent_mismatch",
      response: {
        errors: [
          {
            message: "intent mismatch",
            extensions: {
              code: "intent_mismatch",
              stale: [
                {
                  dimension: "operation_hash",
                  expected: "sha256:old",
                  actual: "sha256:new",
                  path: "input.operation",
                },
              ],
            },
          },
        ],
      },
      expectedExtensions: {
        code: "intent_mismatch",
        stale: [
          {
            dimension: "operation_hash",
            expected: "sha256:old",
            actual: "sha256:new",
            path: "input.operation",
          },
        ],
      },
    },
    {
      label: "forbidden",
      expectedCode: "forbidden",
      response: {
        errors: [
          {
            message: "forbidden",
            extensions: {
              code: "forbidden",
              detail: {
                reason: "needs_approval",
              },
            },
          },
        ],
      },
      expectedExtensions: {
        code: "forbidden",
        detail: {
          reason: "needs_approval",
        },
      },
    },
  ])(
    "commitIntent preserves stable GraphQL error vocabulary @covers US-107-AC6",
    async ({ expectedCode, response, expectedExtensions }) => {
      const { mock } = mockFetch(200, JSON.stringify(response));
      const client = new AxonGraphQLClient({
        baseUrl: "http://localhost:4170",
        fetchImpl: mock,
      });

      await expectGraphQLError(
        client.tenant("acme").database("default").commitIntent({ intentToken: "tok-1" }),
        expectedCode,
        expectedExtensions,
      );
    },
  );

  it("approveIntent preserves approved GraphQL outcome vocabulary @covers US-106-AC4", async () => {
    const approveResponse = {
      id: "i-1",
      approvalState: "approved",
      decision: "needs_approval",
    };
    const { mock, calls } = mockFetch(
      200,
      JSON.stringify({ data: { approveMutationIntent: approveResponse } }),
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    const result = await client.tenant("acme").database("default").approveIntent({
      intentId: "i-1",
      reason: "approved for release",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.approveMutationIntent);
    expect(body.variables).toEqual({ input: { intentId: "i-1", reason: "approved for release" } });
    expect(result).toEqual({ approveMutationIntent: approveResponse });
  });

  it("rejectIntent preserves rejected GraphQL outcome vocabulary @covers US-106-AC4", async () => {
    const rejectResponse = {
      id: "i-2",
      approvalState: "rejected",
      decision: "needs_approval",
    };
    const { mock, calls } = mockFetch(
      200,
      JSON.stringify({ data: { rejectMutationIntent: rejectResponse } }),
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    const result = await client.tenant("acme").database("default").rejectIntent({
      intentId: "i-2",
      reason: "insufficient justification",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.rejectMutationIntent);
    expect(body.variables).toEqual({
      input: { intentId: "i-2", reason: "insufficient justification" },
    });
    expect(result).toEqual({ rejectMutationIntent: rejectResponse });
  });

  it("explainPolicy preserves policy explanation vocabulary", async () => {
    const explainResponse = {
      operation: "update",
      collection: "tasks",
      decision: "needs_approval",
      reason: "needs_approval",
      policyVersion: 1,
    };
    const { mock, calls } = mockFetch(
      200,
      JSON.stringify({ data: { explainPolicy: explainResponse } }),
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    const result = await client.tenant("acme").database("default").explainPolicy({
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
    expect(result).toEqual({ explainPolicy: explainResponse });
  });

  it("queryAudit delegates to auditLog and preserves the returned audit vocabulary", async () => {
    const auditResponse = {
      totalCount: 1,
      edges: [
        {
          cursor: "cursor-1",
          node: {
            id: "a-1",
            timestampNs: "1000",
            collection: "tasks",
            entityId: "task-1",
            version: "4",
            mutation: "entity.update",
            actor: "agent-1",
            dataBefore: { status: "draft" },
            dataAfter: { status: "approved" },
            metadata: {},
            transactionId: "tx-1",
          },
        },
      ],
    };
    const { mock, calls } = mockFetch(
      200,
      JSON.stringify({ data: { auditLog: auditResponse } }),
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    const result = await client.tenant("acme").database("default").queryAudit({
      collection: "tasks",
      entityId: "task-1",
    });

    const body = requestBody(calls[0]);
    expect(body.query).toBe(AxonGraphQLDocuments.auditLog);
    expect(body.variables).toMatchObject({ collection: "tasks", entityId: "task-1" });
    expect(result).toEqual({ auditLog: auditResponse });
  });

  it("rollbackDryRun forces a dry-run rollback response and preserves the GraphQL payload", async () => {
    const rollbackResponse = {
      entity: { id: "task-1", collection: "tasks", version: 1, data: { status: "draft" } },
      auditEntry: { id: "a-1", mutation: "entity.update" },
      dryRun: true,
      diff: [{ path: ["status"], before: "draft", after: "approved" }],
    };
    const { mock, calls } = mockFetch(
      200,
      JSON.stringify({ data: { rollbackEntity: rollbackResponse } }),
    );
    const client = new AxonGraphQLClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    const result = await client.tenant("acme").database("default").rollbackDryRun({
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
    expect(result).toEqual({ rollbackEntity: rollbackResponse });
  });
});
