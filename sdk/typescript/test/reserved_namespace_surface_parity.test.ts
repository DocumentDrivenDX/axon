import { describe, expect, it } from "vitest";
import { AxonClient } from "../src/client.js";
import { AxonError } from "../src/error.js";
import {
  AxonHttpError,
  HttpAxonClient,
  type FetchLike,
  type FetchResponse,
} from "../src/http-client.js";

const RESERVED_NAMESPACE_CODE = "reserved_namespace";
const RESERVED_NAMESPACE_REASON = "generic_access_forbidden";

const RESERVED_NAMESPACE_NAMES = [
  { name: "__axon_links", classification: "hidden" },
  { name: "__axon_links_rev", classification: "hidden" },
  { name: "__axon_cdc_cursors", classification: "hidden" },
  { name: "__axon_mutation_intents", classification: "virtual" },
  { name: "__axon_beads", classification: "governed_system" },
  { name: "__axon_policies", classification: "virtual" },
] as const;

const RESERVED_NAMESPACE_OPERATIONS = [
  "entity",
  "schema",
  "template",
  "lifecycle",
  "link",
  "rollback",
  "intent",
  "query",
  "traverse",
  "transaction",
  "audit",
] as const;

type ReservedNamespaceOperation = (typeof RESERVED_NAMESPACE_OPERATIONS)[number];

type ReservedNamespaceSurfaceParityVector = {
  code: typeof RESERVED_NAMESPACE_CODE;
  reason: typeof RESERVED_NAMESPACE_REASON;
  detailName: string;
  detailOperation: ReservedNamespaceOperation;
  classification: string;
};

type OperationDisposition =
  | { exposure: "exposed" }
  | { exposure: "not-exposed"; reason: string };

const HTTP_SDK_OPERATION_DISPOSITIONS: Record<
  ReservedNamespaceOperation,
  OperationDisposition
> = {
  entity: { exposure: "exposed" },
  schema: { exposure: "exposed" },
  template: {
    exposure: "not-exposed",
    reason: "HttpAxonClient does not expose collection template APIs",
  },
  lifecycle: {
    exposure: "not-exposed",
    reason: "HttpAxonClient does not expose generic lifecycle transition APIs",
  },
  link: {
    exposure: "not-exposed",
    reason: "HttpAxonClient does not expose generic link mutation APIs",
  },
  rollback: {
    exposure: "not-exposed",
    reason: "HttpAxonClient does not expose rollback APIs",
  },
  intent: {
    exposure: "not-exposed",
    reason: "mutation intent review is exposed through GraphQL, not the REST compatibility client",
  },
  query: { exposure: "exposed" },
  traverse: { exposure: "exposed" },
  transaction: { exposure: "exposed" },
  audit: { exposure: "exposed" },
};

const GRPC_SDK_OPERATION_DISPOSITIONS: Record<
  ReservedNamespaceOperation,
  OperationDisposition
> = {
  entity: { exposure: "exposed" },
  schema: {
    exposure: "not-exposed",
    reason: "AxonClient does not expose collection schema APIs",
  },
  template: {
    exposure: "not-exposed",
    reason: "collection template APIs are not exposed in the gRPC SDK client",
  },
  lifecycle: {
    exposure: "not-exposed",
    reason: "AxonClient does not expose generic lifecycle transition APIs",
  },
  link: { exposure: "exposed" },
  rollback: {
    exposure: "not-exposed",
    reason: "rollback APIs are not exposed in the gRPC SDK client",
  },
  intent: {
    exposure: "not-exposed",
    reason: "mutation intent review is exposed through GraphQL, not the gRPC SDK client",
  },
  query: {
    exposure: "not-exposed",
    reason: "AxonClient does not expose generic collection query APIs",
  },
  traverse: { exposure: "exposed" },
  transaction: {
    exposure: "not-exposed",
    reason: "AxonClient does not expose generic transaction commit APIs",
  },
  audit: { exposure: "exposed" },
};

function reservedNamespaceSurfaceParityVectors(): ReservedNamespaceSurfaceParityVector[] {
  return RESERVED_NAMESPACE_NAMES.flatMap((name) =>
    RESERVED_NAMESPACE_OPERATIONS.map((operation) => ({
      code: RESERVED_NAMESPACE_CODE,
      reason: RESERVED_NAMESPACE_REASON,
      detailName: name.name,
      detailOperation: operation,
      classification: name.classification,
    })),
  );
}

function assertDispositionCoverage(
  dispositions: Record<ReservedNamespaceOperation, OperationDisposition>,
): void {
  expect(Object.keys(dispositions).sort()).toEqual(
    [...RESERVED_NAMESPACE_OPERATIONS].sort(),
  );
}

function isExposed(
  dispositions: Record<ReservedNamespaceOperation, OperationDisposition>,
  vector: ReservedNamespaceSurfaceParityVector,
): boolean {
  const disposition = dispositions[vector.detailOperation];
  if (disposition.exposure === "not-exposed") {
    expect(disposition.reason).not.toBe("");
    return false;
  }
  return true;
}

function reservedNamespaceHttpFetch(
  vector: ReservedNamespaceSurfaceParityVector,
): { mock: FetchLike; calls: Array<[string, unknown]> } {
  const calls: Array<[string, unknown]> = [];
  const mock: FetchLike = async (url, init) => {
    calls.push([url, init]);
    const response: FetchResponse = {
      ok: false,
      status: 400,
      text: async () =>
        JSON.stringify({
          code: vector.code,
          detail: {
            reason: vector.reason,
            name: vector.detailName,
            operation: vector.detailOperation,
          },
        }),
    };
    return response;
  };
  return { mock, calls };
}

async function invokeHttpReservedNamespaceVector(
  vector: ReservedNamespaceSurfaceParityVector,
  fetchImpl: FetchLike,
): Promise<unknown> {
  const db = new HttpAxonClient({
    baseUrl: "http://localhost:4170",
    fetchImpl,
  }).tenant("acme").database("orders");

  switch (vector.detailOperation) {
    case "entity":
      return db.getEntity(vector.detailName, "reserved-id");
    case "schema":
      return db.createCollection(vector.detailName);
    case "query":
      return db.query(vector.detailName, {});
    case "traverse":
      return db.traverse(vector.detailName, "reserved-id", { max_depth: 1 });
    case "transaction":
      return db.commitTransaction([
        {
          op: "create",
          collection: vector.detailName,
          id: "reserved-id",
          data: { title: "reserved namespace parity" },
        },
      ]);
    case "audit":
      return db.queryAudit({ collection: vector.detailName, limit: 1 });
    default:
      throw new Error(`HTTP vector is not exposed: ${vector.detailOperation}`);
  }
}

async function expectHttpReservedNamespaceError(
  promise: Promise<unknown>,
  vector: ReservedNamespaceSurfaceParityVector,
): Promise<void> {
  try {
    await promise;
    throw new Error(`expected HTTP reserved namespace error for ${vector.detailOperation}`);
  } catch (error) {
    expect(error).toBeInstanceOf(AxonHttpError);
    const axonError = error as AxonHttpError;
    expect(axonError.status).toBe(400);
    expect(axonError.code).toBe(vector.code);
    expect(axonError.reason).toBe(vector.reason);
    expect(axonError.detail.name).toBe(vector.detailName);
    expect(axonError.detail.operation).toBe(vector.detailOperation);
  }
}

type RejectingRpc = Record<string, (...args: unknown[]) => Promise<never>>;

function grpcErrorFor(vector: ReservedNamespaceSurfaceParityVector): {
  code: number;
  message: string;
} {
  return {
    code: 3,
    message: JSON.stringify({
      code: vector.code,
      reason: vector.reason,
      detail: {
        name: vector.detailName,
        operation: vector.detailOperation,
      },
    }),
  };
}

function grpcClientRejectingWith(vector: ReservedNamespaceSurfaceParityVector): AxonClient {
  const reject = async (): Promise<never> => {
    throw grpcErrorFor(vector);
  };
  const client = new AxonClient({ address: "localhost:0" });
  (client as unknown as { rpc: RejectingRpc }).rpc = {
    getEntity: reject,
    createLink: reject,
    traverse: reject,
    queryAuditByEntity: reject,
  };
  return client;
}

async function invokeGrpcReservedNamespaceVector(
  vector: ReservedNamespaceSurfaceParityVector,
): Promise<unknown> {
  const client = grpcClientRejectingWith(vector);
  switch (vector.detailOperation) {
    case "entity":
      return client.getEntity(vector.detailName, "reserved-id");
    case "link":
      return client.createLink(
        vector.detailName,
        "reserved-id",
        "public-target",
        "target-id",
        "reserved-namespace-parity",
      );
    case "traverse":
      return client.traverse(vector.detailName, "reserved-id", undefined, 1);
    case "audit":
      return client.queryAuditByEntity(vector.detailName, "reserved-id");
    default:
      throw new Error(`gRPC vector is not exposed: ${vector.detailOperation}`);
  }
}

async function expectGrpcReservedNamespaceError(
  promise: Promise<unknown>,
  vector: ReservedNamespaceSurfaceParityVector,
): Promise<void> {
  try {
    await promise;
    throw new Error(`expected gRPC reserved namespace error for ${vector.detailOperation}`);
  } catch (error) {
    expect(error).toBeInstanceOf(AxonError);
    const axonError = error as AxonError;
    expect(axonError.code).toBe(vector.code);
    expect(axonError.reason).toBe(vector.reason);
    expect(axonError.detail.name).toBe(vector.detailName);
    expect(axonError.detail.operation).toBe(vector.detailOperation);
  }
}

describe("reserved_namespace_surface_parity", () => {
  it("preserves structured HTTP reserved namespace errors", async () => {
    assertDispositionCoverage(HTTP_SDK_OPERATION_DISPOSITIONS);

    for (const vector of reservedNamespaceSurfaceParityVectors()) {
      if (!isExposed(HTTP_SDK_OPERATION_DISPOSITIONS, vector)) continue;

      const { mock, calls } = reservedNamespaceHttpFetch(vector);
      await expectHttpReservedNamespaceError(
        invokeHttpReservedNamespaceVector(vector, mock),
        vector,
      );
      expect(calls).toHaveLength(1);
    }
  });

  it("preserves structured gRPC reserved namespace errors", async () => {
    assertDispositionCoverage(GRPC_SDK_OPERATION_DISPOSITIONS);

    for (const vector of reservedNamespaceSurfaceParityVectors()) {
      if (!isExposed(GRPC_SDK_OPERATION_DISPOSITIONS, vector)) continue;

      await expectGrpcReservedNamespaceError(
        invokeGrpcReservedNamespaceVector(vector),
        vector,
      );
    }
  });
});
