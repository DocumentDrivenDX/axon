/**
 * GraphQL-first Axon browser/client SDK.
 *
 * The data-plane client scopes requests to
 * /tenants/:tenant/databases/:database/graphql. Control-plane administration
 * uses /control/graphql. REST is intentionally left in HttpAxonClient as a
 * lower-level compatibility surface.
 */

export type GraphQLVariables = Record<string, unknown>;

export type GraphQLFetchResponse = {
  ok: boolean;
  status: number;
  text(): Promise<string>;
};

export type GraphQLFetchLike = (
  url: string,
  init?: {
    method?: string;
    headers?: Record<string, string>;
    body?: string;
  },
) => Promise<GraphQLFetchResponse>;

export interface AxonGraphQLClientOptions {
  baseUrl: string;
  fetchImpl?: GraphQLFetchLike;
  authToken?: string;
}

export interface GraphQLRequestOptions {
  headers?: Record<string, string>;
}

export interface GraphQLPayload<T = unknown> {
  data?: T;
  errors?: GraphQLErrorPayload[];
}

export interface GraphQLErrorPayload {
  message: string;
  path?: Array<string | number>;
  extensions?: Record<string, unknown>;
}

export interface CommitTransactionOptions {
  idempotencyKey?: string;
}

export interface RollbackEntityOptions {
  toVersion?: number;
  toAuditId?: string;
  expectedVersion?: number;
  dryRun?: boolean;
}

export type TransactionOperation =
  | { createEntity: { collection: string; id: string; data: Record<string, unknown> } }
  | {
      updateEntity: {
        collection: string;
        id: string;
        expectedVersion: number;
        data: Record<string, unknown>;
      };
    }
  | {
      patchEntity: {
        collection: string;
        id: string;
        expectedVersion: number;
        patch: Record<string, unknown>;
      };
    }
  | { deleteEntity: { collection: string; id: string; expectedVersion: number } }
  | {
      createLink: {
        sourceCollection: string;
        sourceId: string;
        targetCollection: string;
        targetId: string;
        linkType: string;
        metadata?: Record<string, unknown>;
      };
    }
  | {
      deleteLink: {
        sourceCollection: string;
        sourceId: string;
        targetCollection: string;
        targetId: string;
        linkType: string;
      };
    };

export interface ListEntitiesOptions {
  filter?: Record<string, unknown>;
  sort?: Array<Record<string, unknown>>;
  limit?: number;
  after?: string;
}

export interface AuditLogOptions {
  collection?: string;
  entityId?: string;
  actor?: string;
  operation?: string;
  sinceNs?: string;
  untilNs?: string;
  after?: string;
  limit?: number;
}

export interface LinkCandidatesOptions {
  search?: string;
  filter?: Record<string, unknown>;
  limit?: number;
}

export interface NeighborsOptions {
  linkType?: string;
  direction?: "outbound" | "inbound";
  limit?: number;
  after?: string;
}

export interface AggregationSpec {
  function: "COUNT" | "SUM" | "AVG" | "MIN" | "MAX";
  field?: string;
}

export interface AggregateOptions {
  filter?: Record<string, unknown>;
  groupBy?: string[];
  aggregations: AggregationSpec[];
}

const CURRENT_USER_DOCUMENT = `
query AxonCurrentUser {
  currentUser {
    actor
    role
    userId
    tenantId
  }
}
`.trim();

const METADATA_DOCUMENT = `
query AxonMetadata {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
  }
  collections {
    name
    version
    schemaVersion
    entityCount
    schema
  }
}
`.trim();

const COLLECTIONS_DOCUMENT = `
query AxonCollections {
  collections {
    name
    version
    schemaVersion
    entityCount
    schema
  }
}
`.trim();

const COLLECTION_DOCUMENT = `
query AxonCollection($name: String!) {
  collection(name: $name) {
    name
    version
    schemaVersion
    entityCount
    schema
  }
}
`.trim();

const ENTITY_DOCUMENT = `
query AxonEntity($collection: String!, $id: ID!) {
  entity(collection: $collection, id: $id) {
    id
    collection
    version
    data
    createdAt
    updatedAt
    lifecycles
  }
}
`.trim();

const ENTITIES_DOCUMENT = `
query AxonEntities($collection: String!, $filter: AxonFilterInput, $sort: [AxonSortInput!], $limit: Int, $after: ID) {
  entities(collection: $collection, filter: $filter, sort: $sort, limit: $limit, after: $after) {
    totalCount
    edges {
      cursor
      node {
        id
        collection
        version
        data
        createdAt
        updatedAt
        lifecycles
      }
    }
    pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
  }
}
`.trim();

const COMMIT_TRANSACTION_DOCUMENT = `
mutation AxonCommitTransaction($input: CommitTransactionInput!) {
  commitTransaction(input: $input) {
    transactionId
    replayHit
    results {
      index
      success
      collection
      id
      entity { id collection version data createdAt updatedAt lifecycles }
      link
    }
  }
}
`.trim();

const CREATE_COLLECTION_DOCUMENT = `
mutation AxonCreateCollection($input: CreateCollectionInput!) {
  createCollection(input: $input) {
    name
    version
    schemaVersion
    entityCount
    schema
  }
}
`.trim();

const PUT_SCHEMA_DOCUMENT = `
mutation AxonPutSchema($input: PutSchemaInput!) {
  putSchema(input: $input) {
    name
    schema
    compatibility
    diff
    dryRun
  }
}
`.trim();

const DROP_COLLECTION_DOCUMENT = `
mutation AxonDropCollection($input: DropCollectionInput!) {
  dropCollection(input: $input) {
    name
    entitiesRemoved
  }
}
`.trim();

const ROLLBACK_ENTITY_DOCUMENT = `
mutation AxonRollbackEntity($input: RollbackEntityInput!) {
  rollbackEntity(input: $input) {
    entity { id collection version data createdAt updatedAt lifecycles }
    auditEntry {
      id
      timestampNs
      collection
      entityId
      version
      mutation
      actor
      dataBefore
      dataAfter
      metadata
      transactionId
    }
    dryRun
    diff
  }
}
`.trim();

const LINK_CANDIDATES_DOCUMENT = `
query AxonLinkCandidates($sourceCollection: String!, $sourceId: ID!, $linkType: String!, $search: String, $filter: AxonFilterInput, $limit: Int) {
  linkCandidates(sourceCollection: $sourceCollection, sourceId: $sourceId, linkType: $linkType, search: $search, filter: $filter, limit: $limit) {
    targetCollection
    linkType
    cardinality
    existingLinkCount
    candidates {
      alreadyLinked
      entity { id collection version data }
    }
  }
}
`.trim();

const NEIGHBORS_DOCUMENT = `
query AxonNeighbors($collection: String!, $id: ID!, $linkType: String, $direction: String, $limit: Int, $after: ID) {
  neighbors(collection: $collection, id: $id, linkType: $linkType, direction: $direction, limit: $limit, after: $after) {
    groups {
      linkType
      direction
      edges {
        cursor
        metadata
        node { id collection version data }
      }
    }
    pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
    totalCount
  }
}
`.trim();

const AUDIT_LOG_DOCUMENT = `
query AxonAuditLog($collection: String, $entityId: ID, $actor: String, $operation: String, $sinceNs: String, $untilNs: String, $after: String, $limit: Int) {
  auditLog(collection: $collection, entityId: $entityId, actor: $actor, operation: $operation, sinceNs: $sinceNs, untilNs: $untilNs, after: $after, limit: $limit) {
    totalCount
    edges {
      cursor
      node {
        id
        timestampNs
        collection
        entityId
        version
        mutation
        actor
        dataBefore
        dataAfter
        metadata
        transactionId
      }
    }
    pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
  }
}
`.trim();

const CONTROL_OVERVIEW_DOCUMENT = `
query AxonControlOverview($tenantId: String!) {
  tenants { id name dbName createdAt }
  users { id displayName email createdAtMs suspendedAtMs }
  tenantMembers(tenantId: $tenantId) { tenantId userId role }
  tenantDatabases(tenantId: $tenantId) { tenantId name createdAtMs }
  credentials(tenantId: $tenantId) {
    jti
    userId
    tenantId
    issuedAtMs
    expiresAtMs
    revoked
    grants
  }
}
`.trim();

const ISSUE_CREDENTIAL_DOCUMENT = `
mutation AxonIssueCredential($tenantId: String!, $targetUser: String!, $grants: JSON!, $ttlSeconds: Int!) {
  issueCredential(tenantId: $tenantId, targetUser: $targetUser, grants: $grants, ttlSeconds: $ttlSeconds) {
    jwt
    jti
    expiresAt
  }
}
`.trim();

const REVOKE_CREDENTIAL_DOCUMENT = `
mutation AxonRevokeCredential($tenantId: String!, $jti: String!) {
  revokeCredential(tenantId: $tenantId, jti: $jti) {
    tenantId
    jti
    revoked
  }
}
`.trim();

export const AxonGraphQLDocuments = {
  currentUser: CURRENT_USER_DOCUMENT,
  metadata: METADATA_DOCUMENT,
  collections: COLLECTIONS_DOCUMENT,
  collection: COLLECTION_DOCUMENT,
  entity: ENTITY_DOCUMENT,
  entities: ENTITIES_DOCUMENT,
  commitTransaction: COMMIT_TRANSACTION_DOCUMENT,
  createCollection: CREATE_COLLECTION_DOCUMENT,
  putSchema: PUT_SCHEMA_DOCUMENT,
  dropCollection: DROP_COLLECTION_DOCUMENT,
  rollbackEntity: ROLLBACK_ENTITY_DOCUMENT,
  linkCandidates: LINK_CANDIDATES_DOCUMENT,
  neighbors: NEIGHBORS_DOCUMENT,
  auditLog: AUDIT_LOG_DOCUMENT,
  controlOverview: CONTROL_OVERVIEW_DOCUMENT,
  issueCredential: ISSUE_CREDENTIAL_DOCUMENT,
  revokeCredential: REVOKE_CREDENTIAL_DOCUMENT,
  aggregate: buildAggregateDocument,
  transitionLifecycle: buildTransitionLifecycleDocument,
  entityChangedSubscription: buildEntityChangedSubscriptionDocument,
};

export class AxonGraphQLError extends Error {
  public readonly code: string;

  constructor(
    public readonly errors: GraphQLErrorPayload[],
    public readonly status?: number,
    public readonly body?: string,
  ) {
    const first = errors[0];
    const code = String(first?.extensions?.code ?? "GRAPHQL_ERROR");
    super(first ? `${code}: ${first.message}` : code);
    this.name = "AxonGraphQLError";
    this.code = code;
  }

  get extensions(): Record<string, unknown> | undefined {
    return this.errors[0]?.extensions;
  }
}

export class AxonGraphQLClient {
  constructor(private readonly options: AxonGraphQLClientOptions) {}

  rootUrlFor(path: string): string {
    return `${this.options.baseUrl.replace(/\/$/, "")}${path}`;
  }

  async currentUser<T = unknown>(): Promise<T> {
    return this.control().currentUser<T>();
  }

  tenant(name: string): GraphQLTenantClient {
    return new GraphQLTenantClient(this.options, name);
  }

  control(): ControlGraphQLClient {
    return new ControlGraphQLClient(this.options);
  }
}

export class GraphQLTenantClient {
  constructor(
    private readonly options: AxonGraphQLClientOptions,
    private readonly tenantName: string,
  ) {}

  database(name: string): GraphQLDatabaseClient {
    return new GraphQLDatabaseClient(this.options, this.tenantName, name);
  }
}

export class ControlGraphQLClient {
  constructor(private readonly options: AxonGraphQLClientOptions) {}

  urlFor(): string {
    return `${this.options.baseUrl.replace(/\/$/, "")}/control/graphql`;
  }

  graphql<T = unknown>(
    query: string,
    variables?: GraphQLVariables,
    requestOptions?: GraphQLRequestOptions,
  ): Promise<T> {
    return requestGraphQL<T>(this.options, this.urlFor(), query, variables, requestOptions);
  }

  currentUser<T = unknown>(): Promise<T> {
    return this.graphql<T>(CURRENT_USER_DOCUMENT);
  }

  overview(tenantId: string): Promise<unknown> {
    return this.graphql(CONTROL_OVERVIEW_DOCUMENT, { tenantId });
  }

  tenants(): Promise<unknown> {
    return this.graphql("query AxonTenants { tenants { id name dbName createdAt } }");
  }

  tenant(id: string): Promise<unknown> {
    return this.graphql(
      "query AxonTenant($id: String!) { tenant(id: $id) { id name dbName createdAt } }",
      { id },
    );
  }

  users(): Promise<unknown> {
    return this.graphql(
      "query AxonUsers { users { id displayName email createdAtMs suspendedAtMs } }",
    );
  }

  tenantMembers(tenantId: string): Promise<unknown> {
    return this.graphql(
      "query AxonTenantMembers($tenantId: String!) { tenantMembers(tenantId: $tenantId) { tenantId userId role } }",
      { tenantId },
    );
  }

  tenantDatabases(tenantId: string): Promise<unknown> {
    return this.graphql(
      "query AxonTenantDatabases($tenantId: String!) { tenantDatabases(tenantId: $tenantId) { tenantId name createdAtMs } }",
      { tenantId },
    );
  }

  credentials(tenantId: string): Promise<unknown> {
    return this.graphql(
      "query AxonCredentials($tenantId: String!) { credentials(tenantId: $tenantId) { jti userId tenantId issuedAtMs expiresAtMs revoked grants } }",
      { tenantId },
    );
  }

  createTenant(name: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonCreateTenant($name: String!) { createTenant(name: $name) { id name dbName dbPath createdAt } }",
      { name },
    );
  }

  deleteTenant(id: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonDeleteTenant($id: String!) { deleteTenant(id: $id) { deleted tenantId dbName } }",
      { id },
    );
  }

  provisionUser(displayName: string, email?: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonProvisionUser($displayName: String!, $email: String) { provisionUser(displayName: $displayName, email: $email) { id displayName email createdAtMs suspendedAtMs } }",
      { displayName, email },
    );
  }

  suspendUser(userId: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonSuspendUser($userId: String!) { suspendUser(userId: $userId) { userId suspended } }",
      { userId },
    );
  }

  upsertTenantMember(tenantId: string, userId: string, role: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonUpsertTenantMember($tenantId: String!, $userId: String!, $role: String!) { upsertTenantMember(tenantId: $tenantId, userId: $userId, role: $role) { tenantId userId role } }",
      { tenantId, userId, role },
    );
  }

  removeTenantMember(tenantId: string, userId: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonRemoveTenantMember($tenantId: String!, $userId: String!) { removeTenantMember(tenantId: $tenantId, userId: $userId) { tenantId userId deleted } }",
      { tenantId, userId },
    );
  }

  createTenantDatabase(tenantId: string, name: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonCreateTenantDatabase($tenantId: String!, $name: String!) { createTenantDatabase(tenantId: $tenantId, name: $name) { tenantId name createdAtMs } }",
      { tenantId, name },
    );
  }

  deleteTenantDatabase(tenantId: string, name: string): Promise<unknown> {
    return this.graphql(
      "mutation AxonDeleteTenantDatabase($tenantId: String!, $name: String!) { deleteTenantDatabase(tenantId: $tenantId, name: $name) { tenantId name deleted } }",
      { tenantId, name },
    );
  }

  issueCredential(
    tenantId: string,
    targetUser: string,
    grants: Record<string, unknown>,
    ttlSeconds: number,
  ): Promise<unknown> {
    return this.graphql(ISSUE_CREDENTIAL_DOCUMENT, {
      tenantId,
      targetUser,
      grants,
      ttlSeconds,
    });
  }

  revokeCredential(tenantId: string, jti: string): Promise<unknown> {
    return this.graphql(REVOKE_CREDENTIAL_DOCUMENT, { tenantId, jti });
  }
}

export class GraphQLDatabaseClient {
  constructor(
    private readonly options: AxonGraphQLClientOptions,
    private readonly tenantName: string,
    private readonly databaseName: string,
  ) {}

  urlFor(): string {
    return `${this.options.baseUrl.replace(/\/$/, "")}/tenants/${this.tenantName}/databases/${this.databaseName}/graphql`;
  }

  subscriptionUrl(): string {
    const endpoint = this.urlFor().replace(/^http:/, "ws:").replace(/^https:/, "wss:");
    return `${endpoint}/ws`;
  }

  graphql<T = unknown>(
    query: string,
    variables?: GraphQLVariables,
    requestOptions?: GraphQLRequestOptions,
  ): Promise<T> {
    return requestGraphQL<T>(this.options, this.urlFor(), query, variables, requestOptions);
  }

  metadata(expectedSchemaHash?: string): Promise<unknown> {
    const headers =
      expectedSchemaHash === undefined
        ? undefined
        : { "x-axon-schema-hash": expectedSchemaHash };
    return this.graphql(METADATA_DOCUMENT, undefined, { headers });
  }

  refreshSchema(expectedSchemaHash?: string): Promise<unknown> {
    return this.metadata(expectedSchemaHash);
  }

  collections(): Promise<unknown> {
    return this.graphql(COLLECTIONS_DOCUMENT);
  }

  collection(name: string): Promise<unknown> {
    return this.graphql(COLLECTION_DOCUMENT, { name });
  }

  getEntity(collection: string, id: string): Promise<unknown> {
    return this.graphql(ENTITY_DOCUMENT, { collection, id });
  }

  listEntities(collection: string, options: ListEntitiesOptions = {}): Promise<unknown> {
    return this.graphql(ENTITIES_DOCUMENT, {
      collection,
      filter: options.filter,
      sort: options.sort,
      limit: options.limit,
      after: options.after,
    });
  }

  createEntity(
    collection: string,
    id: string,
    data: Record<string, unknown>,
    options: CommitTransactionOptions = {},
  ): Promise<unknown> {
    return this.commitTransaction([{ createEntity: { collection, id, data } }], options);
  }

  updateEntity(
    collection: string,
    id: string,
    expectedVersion: number,
    data: Record<string, unknown>,
    options: CommitTransactionOptions = {},
  ): Promise<unknown> {
    return this.commitTransaction(
      [{ updateEntity: { collection, id, expectedVersion, data } }],
      options,
    );
  }

  patchEntity(
    collection: string,
    id: string,
    expectedVersion: number,
    patch: Record<string, unknown>,
    options: CommitTransactionOptions = {},
  ): Promise<unknown> {
    return this.commitTransaction(
      [{ patchEntity: { collection, id, expectedVersion, patch } }],
      options,
    );
  }

  deleteEntity(
    collection: string,
    id: string,
    expectedVersion: number,
    options: CommitTransactionOptions = {},
  ): Promise<unknown> {
    return this.commitTransaction(
      [{ deleteEntity: { collection, id, expectedVersion } }],
      options,
    );
  }

  rollbackEntity(
    collection: string,
    id: string,
    options: RollbackEntityOptions,
  ): Promise<unknown> {
    return this.graphql(ROLLBACK_ENTITY_DOCUMENT, {
      input: {
        collection,
        id,
        toVersion: options.toVersion,
        toAuditId: options.toAuditId,
        expectedVersion: options.expectedVersion,
        dryRun: options.dryRun,
      },
    });
  }

  createCollection(name: string, schema: Record<string, unknown>): Promise<unknown> {
    return this.graphql(CREATE_COLLECTION_DOCUMENT, { input: { name, schema } });
  }

  putSchema(
    collection: string,
    schema: Record<string, unknown>,
    options: { force?: boolean; dryRun?: boolean } = {},
  ): Promise<unknown> {
    return this.graphql(PUT_SCHEMA_DOCUMENT, {
      input: { collection, schema, force: options.force, dryRun: options.dryRun },
    });
  }

  dropCollection(name: string, confirm = true): Promise<unknown> {
    return this.graphql(DROP_COLLECTION_DOCUMENT, { input: { name, confirm } });
  }

  commitTransaction(
    operations: TransactionOperation[],
    options: CommitTransactionOptions = {},
  ): Promise<unknown> {
    const input =
      options.idempotencyKey === undefined
        ? { operations }
        : { operations, idempotencyKey: options.idempotencyKey };
    return this.graphql(COMMIT_TRANSACTION_DOCUMENT, { input });
  }

  createLink(
    sourceCollection: string,
    sourceId: string,
    targetCollection: string,
    targetId: string,
    linkType: string,
    metadata?: Record<string, unknown>,
    options: CommitTransactionOptions = {},
  ): Promise<unknown> {
    return this.commitTransaction(
      [
        {
          createLink: {
            sourceCollection,
            sourceId,
            targetCollection,
            targetId,
            linkType,
            metadata,
          },
        },
      ],
      options,
    );
  }

  deleteLink(
    sourceCollection: string,
    sourceId: string,
    targetCollection: string,
    targetId: string,
    linkType: string,
    options: CommitTransactionOptions = {},
  ): Promise<unknown> {
    return this.commitTransaction(
      [{ deleteLink: { sourceCollection, sourceId, targetCollection, targetId, linkType } }],
      options,
    );
  }

  linkCandidates(
    sourceCollection: string,
    sourceId: string,
    linkType: string,
    options: LinkCandidatesOptions = {},
  ): Promise<unknown> {
    return this.graphql(LINK_CANDIDATES_DOCUMENT, {
      sourceCollection,
      sourceId,
      linkType,
      search: options.search,
      filter: options.filter,
      limit: options.limit,
    });
  }

  neighbors(collection: string, id: string, options: NeighborsOptions = {}): Promise<unknown> {
    return this.graphql(NEIGHBORS_DOCUMENT, {
      collection,
      id,
      linkType: options.linkType,
      direction: options.direction,
      limit: options.limit,
      after: options.after,
    });
  }

  auditLog(options: AuditLogOptions = {}): Promise<unknown> {
    return this.graphql(AUDIT_LOG_DOCUMENT, options as GraphQLVariables);
  }

  aggregate(collection: string, options: AggregateOptions): Promise<unknown> {
    return this.graphql(buildAggregateDocument(collection, options), {
      filter: options.filter,
    });
  }

  transitionLifecycle(
    collection: string,
    id: string,
    lifecycleName: string,
    targetState: string,
    expectedVersion: number,
  ): Promise<unknown> {
    return this.graphql(buildTransitionLifecycleDocument(collection), {
      id,
      lifecycleName,
      targetState,
      expectedVersion,
    });
  }

  entityChangedSubscription(collection?: string): string {
    return buildEntityChangedSubscriptionDocument(collection);
  }
}

async function requestGraphQL<T>(
  options: AxonGraphQLClientOptions,
  url: string,
  query: string,
  variables?: GraphQLVariables,
  requestOptions: GraphQLRequestOptions = {},
): Promise<T> {
  const fetchImpl = resolveFetch(options);
  const headers: Record<string, string> = {
    "content-type": "application/json",
    ...requestOptions.headers,
  };
  if (options.authToken) {
    headers.authorization = `Bearer ${options.authToken}`;
  }

  const response = await fetchImpl(url, {
    method: "POST",
    headers,
    body: JSON.stringify({ query, variables: variables ?? {} }),
  });
  const text = await response.text();

  let payload: GraphQLPayload<T>;
  try {
    payload = text ? (JSON.parse(text) as GraphQLPayload<T>) : {};
  } catch {
    if (!response.ok) {
      throw new AxonGraphQLError(
        [{ message: text || `HTTP ${response.status}`, extensions: { code: "HTTP_ERROR" } }],
        response.status,
        text,
      );
    }
    throw new AxonGraphQLError(
      [{ message: "invalid GraphQL JSON response", extensions: { code: "INVALID_JSON" } }],
      response.status,
      text,
    );
  }

  if (!response.ok || payload.errors?.length) {
    throw new AxonGraphQLError(
      payload.errors ?? [
        { message: text || `HTTP ${response.status}`, extensions: { code: "HTTP_ERROR" } },
      ],
      response.status,
      text,
    );
  }

  return payload.data as T;
}

function resolveFetch(options: AxonGraphQLClientOptions): GraphQLFetchLike {
  if (options.fetchImpl) {
    return options.fetchImpl;
  }
  const fetchImpl = (globalThis as { fetch?: GraphQLFetchLike }).fetch;
  if (!fetchImpl) {
    throw new Error("No fetch implementation available");
  }
  return fetchImpl;
}

export function buildAggregateDocument(collection: string, options: AggregateOptions): string {
  const fieldName = `${collectionFieldName(collection)}Aggregate`;
  const filterType = `${pascalCase(collection)}Filter`;
  const groupBy = options.groupBy?.length
    ? `, groupBy: [${options.groupBy.map(graphqlEnumLiteral).join(", ")}]`
    : "";
  const aggregations = options.aggregations
    .map((aggregation) => {
      const field =
        aggregation.field === undefined ? "" : `, field: ${graphqlEnumLiteral(aggregation.field)}`;
      return `{ function: ${aggregation.function}${field} }`;
    })
    .join(", ");

  return `
query AxonAggregate($filter: ${filterType}) {
  ${fieldName}(filter: $filter${groupBy}, aggregations: [${aggregations}]) {
    totalCount
    groups {
      key
      keyFields
      count
      values { function field value count }
    }
  }
}
`.trim();
}

export function buildTransitionLifecycleDocument(collection: string): string {
  const typeName = pascalCase(collection);
  return `
mutation AxonTransitionLifecycle($id: ID!, $lifecycleName: String!, $targetState: String!, $expectedVersion: Int!) {
  transition${typeName}Lifecycle(id: $id, lifecycleName: $lifecycleName, targetState: $targetState, expectedVersion: $expectedVersion) {
    id
    version
    lifecycles
  }
}
`.trim();
}

export function buildEntityChangedSubscriptionDocument(collection?: string): string {
  if (!collection) {
    return `
subscription AxonEntityChanged($collection: String, $filter: AxonFilterInput) {
  entityChanged(collection: $collection, filter: $filter) {
    collection
    id
    mutation
    version
    data
    previousData
    previousVersion
    timestampMs
    timestampNs
    actor
  }
}
`.trim();
  }

  const fieldName = `${collectionFieldName(collection)}Changed`;
  const filterType = `${pascalCase(collection)}Filter`;
  return `
subscription AxonCollectionChanged($filter: ${filterType}) {
  ${fieldName}(filter: $filter) {
    collection
    id
    mutation
    version
    data
    previousData
    previousVersion
    timestampMs
    timestampNs
    actor
  }
}
`.trim();
}

export function pascalCase(value: string): string {
  const words = value.split(/[^A-Za-z0-9]+/).filter(Boolean);
  const name = words
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase())
    .join("");
  return name || "Collection";
}

export function collectionFieldName(collection: string): string {
  const words = collection.split(/[^A-Za-z0-9]+/).filter(Boolean);
  if (words.length === 0) {
    return "collection";
  }
  const [first, ...rest] = words;
  const name =
    first.toLowerCase() +
    rest.map((word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase()).join("");
  return ["entity", "entities", "collection", "collections", "auditLog"].includes(name)
    ? `${name}Collection`
    : name;
}

function graphqlEnumLiteral(value: string): string {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(value)) {
    throw new Error(`GraphQL enum literal cannot be generated from '${value}'`);
  }
  return value;
}
