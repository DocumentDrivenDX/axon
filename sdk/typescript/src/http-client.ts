/**
 * HTTP-based Axon client that talks to the REST gateway and scopes every
 * call under /tenants/:tenant/databases/:database/ per ADR-018.
 *
 * Usage:
 *
 *     const client = new HttpAxonClient({ baseUrl: "http://localhost:4170" });
 *     const orders = client.tenant("acme").database("orders");
 *     await orders.createEntity("items", "item-1", { sku: "SKU-1" });
 *     const e = await orders.getEntity("items", "item-1");
 */

import { AUTH_ERROR_CODES, type AuthErrorCode } from "./auth-error-codes.js";

// Minimal fetch interface — avoids DOM lib dependency while remaining
// compatible with the global fetch in Node 18+ and all modern browsers.
export type FetchResponse = {
  ok: boolean;
  status: number;
  text(): Promise<string>;
};

export type FetchLike = (
  url: string,
  init?: {
    method?: string;
    headers?: Record<string, string>;
    body?: string;
  },
) => Promise<FetchResponse>;

export interface HttpAxonClientOptions {
  baseUrl: string;
  fetchImpl?: FetchLike;
  authToken?: string;
}

export class HttpAxonClient {
  constructor(private readonly options: HttpAxonClientOptions) {}

  rootUrlFor(path: string): string {
    const base = this.options.baseUrl.replace(/\/$/, "");
    return `${base}${path}`;
  }

  private async request<T>(method: string, path: string): Promise<T> {
    return requestJson<T>(this.options, this.rootUrlFor(path), method);
  }

  /** Resolve the current authenticated Axon identity. */
  async me(): Promise<unknown> {
    return this.request("GET", "/auth/me");
  }

  /** Scope into a tenant. Returns a TenantClient. */
  tenant(name: string): TenantClient {
    return new TenantClient(this.options, name);
  }
}

export class TenantClient {
  constructor(
    private readonly options: HttpAxonClientOptions,
    private readonly tenantName: string,
  ) {}

  /** Scope into a database within this tenant. Returns a DatabaseClient. */
  database(name: string): DatabaseClient {
    return new DatabaseClient(this.options, this.tenantName, name);
  }
}

export class DatabaseClient {
  constructor(
    private readonly options: HttpAxonClientOptions,
    private readonly tenantName: string,
    private readonly databaseName: string,
  ) {}

  urlFor(path: string): string {
    const base = this.options.baseUrl.replace(/\/$/, "");
    return `${base}/tenants/${this.tenantName}/databases/${this.databaseName}${path}`;
  }

  private async request<T>(
    method: string,
    path: string,
    body?: unknown,
    headers?: Record<string, string>,
  ): Promise<T> {
    return requestJson<T>(this.options, this.urlFor(path), method, body, headers);
  }

  // ── Entity CRUD ─────────────────────────────────────────────────────
  async createEntity(
    collection: string,
    id: string,
    data: Record<string, unknown>,
  ): Promise<unknown> {
    return this.request("POST", `/entities/${collection}/${id}`, data);
  }

  async getEntity(collection: string, id: string): Promise<unknown> {
    return this.request("GET", `/entities/${collection}/${id}`);
  }

  async updateEntity(
    collection: string,
    id: string,
    data: Record<string, unknown>,
    expectedVersion?: number,
  ): Promise<unknown> {
    const body =
      expectedVersion !== undefined
        ? { data, expected_version: expectedVersion }
        : { data };
    return this.request("PUT", `/entities/${collection}/${id}`, body);
  }

  async deleteEntity(collection: string, id: string): Promise<void> {
    await this.request("DELETE", `/entities/${collection}/${id}`);
  }

  // ── Collections ─────────────────────────────────────────────────────
  async listCollections(): Promise<unknown> {
    return this.request("GET", "/collections");
  }

  async createCollection(name: string): Promise<unknown> {
    return this.request("POST", `/collections/${name}`);
  }

  async schemaManifest(expectedHash?: string): Promise<unknown> {
    const headers =
      expectedHash === undefined ? undefined : { "x-axon-schema-hash": expectedHash };
    return this.request("GET", "/schema", undefined, headers);
  }

  // ── Query / snapshot ────────────────────────────────────────────────
  async query(
    collection: string,
    filter: Record<string, unknown>,
  ): Promise<unknown> {
    return this.request("POST", `/collections/${collection}/query`, filter);
  }

  async traverse(
    collection: string,
    id: string,
    body: {
      link_type?: string;
      max_depth?: number;
      direction?: "forward" | "reverse";
      hop_filter?: Record<string, unknown>;
    } = {},
  ): Promise<unknown> {
    return this.request("POST", `/traverse/${collection}/${id}`, body);
  }

  async graphql<T = unknown>(
    query: string,
    variables?: Record<string, unknown>,
  ): Promise<T> {
    return this.request("POST", "/graphql", { query, variables });
  }

  async commitTransaction(
    operations: unknown[],
    options: { idempotencyKey?: string } = {},
  ): Promise<unknown> {
    const headers =
      options.idempotencyKey === undefined
        ? undefined
        : { "idempotency-key": options.idempotencyKey };
    return this.request("POST", "/transactions", { operations }, headers);
  }

  async snapshot(collection: string): Promise<unknown> {
    return this.request("POST", "/snapshot", { collection });
  }

  // ── Audit ───────────────────────────────────────────────────────────
  async queryAudit(params: Record<string, unknown>): Promise<unknown> {
    const qs = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) {
      qs.append(k, String(v));
    }
    return this.request("GET", `/audit/query?${qs.toString()}`);
  }
}

async function requestJson<T>(
  options: HttpAxonClientOptions,
  url: string,
  method: string,
  body?: unknown,
  extraHeaders?: Record<string, string>,
): Promise<T> {
  // Node 18+ and modern browsers both provide a global fetch.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fetchImpl: FetchLike = options.fetchImpl ?? (globalThis as any).fetch;
  const headers: Record<string, string> = {
    "content-type": "application/json",
    ...extraHeaders,
  };
  if (options.authToken) {
    headers.authorization = `Bearer ${options.authToken}`;
  }
  const res = await fetchImpl(url, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    const errText = await res.text();
    let errCode = "unknown";
    try {
      const parsed = JSON.parse(errText);
      errCode = parsed.code ?? parsed.error?.code ?? "unknown";
    } catch (_) {
      /* ignore */
    }
    throw new AxonHttpError(res.status, errCode, errText);
  }
  const text = await res.text();
  if (!text) return undefined as unknown as T;
  return JSON.parse(text) as T;
}

export class AxonHttpError extends Error {
  constructor(
    public readonly status: number,
    public readonly code: AuthErrorCode | string,
    public readonly body: string,
  ) {
    super(`${status} ${code}: ${body}`);
    this.name = "AxonHttpError";
  }

  /** Narrow the code to AuthErrorCode if it matches a known variant. */
  isAuthError(): this is AxonHttpError & { code: AuthErrorCode } {
    return (AUTH_ERROR_CODES as readonly string[]).includes(this.code);
  }
}
