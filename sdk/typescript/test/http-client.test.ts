/**
 * Tests for the HTTP-based Axon client (HttpAxonClient).
 *
 * All tests mock the fetch implementation so no running server is required.
 */

import { describe, it, expect, vi } from "vitest";
import {
  HttpAxonClient,
  AxonHttpError,
  type FetchLike,
  type FetchResponse,
} from "../src/http-client.js";

// Helper: build a mock fetch that returns a canned response.
function mockFetch(
  status: number,
  body: string,
  ok?: boolean,
): { mock: FetchLike; calls: Array<[string, unknown]> } {
  const calls: Array<[string, unknown]> = [];
  const isOk = ok ?? (status >= 200 && status < 300);
  const mock: FetchLike = async (url, init) => {
    calls.push([url, init]);
    const response: FetchResponse = {
      ok: isOk,
      status,
      text: async () => body,
    };
    return response;
  };
  return { mock, calls };
}

describe("fluent_scope_builds_correct_urls", () => {
  it("urlFor produces /tenants/:t/databases/:d/... prefix", () => {
    const client = new HttpAxonClient({ baseUrl: "http://localhost:4170" });
    const db = client.tenant("acme").database("orders");
    expect(db.urlFor("/entities/items/item-1")).toBe(
      "http://localhost:4170/tenants/acme/databases/orders/entities/items/item-1",
    );
    expect(db.urlFor("/collections")).toBe(
      "http://localhost:4170/tenants/acme/databases/orders/collections",
    );
  });
});

describe("create_entity_hits_correct_path", () => {
  it("POSTs to /tenants/:t/databases/:d/entities/:collection/:id", async () => {
    const { mock, calls } = mockFetch(200, "{}");
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });
    await client.tenant("acme").database("orders").createEntity("items", "item-1", { sku: "SKU-1" });

    expect(calls).toHaveLength(1);
    const [url, init] = calls[0];
    expect(url).toBe(
      "http://localhost:4170/tenants/acme/databases/orders/entities/items/item-1",
    );
    expect((init as { method: string }).method).toBe("POST");
  });
});

describe("get_entity_roundtrip", () => {
  it("parses and returns the JSON response body", async () => {
    const payload = { id: "item-1", collection: "items", data: { sku: "SKU-1" } };
    const { mock } = mockFetch(200, JSON.stringify(payload));
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });
    const result = await client.tenant("acme").database("orders").getEntity("items", "item-1");
    expect(result).toEqual(payload);
  });
});

describe("error_non_2xx_throws_http_error", () => {
  it("throws AxonHttpError with status=404 on 404 response", async () => {
    const { mock } = mockFetch(404, '{"code":"not_found"}');
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });
    await expect(
      client.tenant("acme").database("orders").getEntity("items", "missing"),
    ).rejects.toBeInstanceOf(AxonHttpError);

    try {
      await client.tenant("acme").database("orders").getEntity("items", "missing");
    } catch (e) {
      expect(e).toBeInstanceOf(AxonHttpError);
      expect((e as AxonHttpError).status).toBe(404);
    }
  });
});

describe("error_code_extracted_from_body", () => {
  it("extracts .code from JSON error body", async () => {
    const { mock } = mockFetch(403, '{"code":"credential_wrong_tenant"}');
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });
    try {
      await client.tenant("acme").database("orders").getEntity("items", "x");
      expect.fail("should have thrown");
    } catch (e) {
      expect(e).toBeInstanceOf(AxonHttpError);
      expect((e as AxonHttpError).code).toBe("credential_wrong_tenant");
      expect((e as AxonHttpError).status).toBe(403);
    }
  });
});

describe("auth_token_sets_authorization_header", () => {
  it("passes Authorization: Bearer <token> header when authToken provided", async () => {
    const { mock, calls } = mockFetch(200, "{}");
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
      authToken: "my-secret-token",
    });
    await client.tenant("acme").database("orders").getEntity("items", "item-1");

    const [, init] = calls[0];
    const headers = (init as { headers: Record<string, string> }).headers;
    expect(headers.authorization).toBe("Bearer my-secret-token");
  });
});

describe("current_identity", () => {
  it("GETs /auth/me outside tenant/database scope", async () => {
    const { mock, calls } = mockFetch(200, '{"actor":"anonymous","role":"admin"}');
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.me();

    const [url, init] = calls[0];
    expect(url).toBe("http://localhost:4170/auth/me");
    expect((init as { method: string }).method).toBe("GET");
  });
});

describe("schema_manifest_handshake", () => {
  it("sends x-axon-schema-hash when provided", async () => {
    const { mock, calls } = mockFetch(200, '{"schema_hash":"fnv64:abc"}');
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("orders").schemaManifest("fnv64:abc");

    const [url, init] = calls[0];
    expect(url).toBe("http://localhost:4170/tenants/acme/databases/orders/schema");
    const headers = (init as { headers: Record<string, string> }).headers;
    expect(headers["x-axon-schema-hash"]).toBe("fnv64:abc");
  });
});

describe("traverse_and_transactions", () => {
  it("POSTs traversal filters as JSON body", async () => {
    const { mock, calls } = mockFetch(200, '{"entities":[],"paths":[]}');
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client.tenant("acme").database("orders").traverse("engagements", "eng-1", {
      link_type: "has_phase",
      max_depth: 2,
      hop_filter: { type: "field", field: "status", op: "eq", value: "approved" },
    });

    const [url, init] = calls[0];
    expect(url).toBe(
      "http://localhost:4170/tenants/acme/databases/orders/traverse/engagements/eng-1",
    );
    expect(JSON.parse((init as { body: string }).body)).toEqual({
      link_type: "has_phase",
      max_depth: 2,
      hop_filter: { type: "field", field: "status", op: "eq", value: "approved" },
    });
  });

  it("passes idempotency keys on transaction commits", async () => {
    const { mock, calls } = mockFetch(200, '{"results":[]}');
    const client = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: mock,
    });

    await client
      .tenant("acme")
      .database("orders")
      .commitTransaction([{ op: "create", collection: "items", id: "i1", data: {} }], {
        idempotencyKey: "retry-1",
      });

    const [url, init] = calls[0];
    expect(url).toBe("http://localhost:4170/tenants/acme/databases/orders/transactions");
    const headers = (init as { headers: Record<string, string> }).headers;
    expect(headers["idempotency-key"]).toBe("retry-1");
  });
});

describe("base_url_trailing_slash_handled", () => {
  it("produces identical URLs regardless of trailing slash on baseUrl", async () => {
    const withSlash = new HttpAxonClient({
      baseUrl: "http://localhost:4170/",
      fetchImpl: async () => ({ ok: true, status: 200, text: async () => "{}" }),
    });
    const withoutSlash = new HttpAxonClient({
      baseUrl: "http://localhost:4170",
      fetchImpl: async () => ({ ok: true, status: 200, text: async () => "{}" }),
    });

    const urlWith = withSlash.tenant("acme").database("orders").urlFor("/entities/items/x");
    const urlWithout = withoutSlash.tenant("acme").database("orders").urlFor("/entities/items/x");

    expect(urlWith).toBe(urlWithout);
    expect(urlWith).toBe(
      "http://localhost:4170/tenants/acme/databases/orders/entities/items/x",
    );
  });
});
