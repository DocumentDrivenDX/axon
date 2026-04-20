/**
 * @axon/client — TypeScript SDK for the Axon data store.
 *
 * GraphQL-first browser/client SDK, plus generated gRPC and REST
 * compatibility clients for lower-level integrations.
 */

export {
  AxonGraphQLClient,
  GraphQLTenantClient,
  GraphQLDatabaseClient,
  ControlGraphQLClient,
  AxonGraphQLError,
  AxonGraphQLDocuments,
  buildAggregateDocument,
  buildEntityChangedSubscriptionDocument,
  buildTransitionLifecycleDocument,
  collectionFieldName,
  pascalCase,
  type AggregateOptions,
  type AggregationSpec,
  type AuditLogOptions,
  type AxonGraphQLClientOptions,
  type CommitTransactionOptions,
  type GraphQLErrorPayload,
  type GraphQLFetchLike,
  type GraphQLFetchResponse,
  type GraphQLPayload,
  type GraphQLRequestOptions,
  type GraphQLVariables,
  type LinkCandidatesOptions,
  type ListEntitiesOptions,
  type NeighborsOptions,
  type RollbackEntityOptions,
  type TransactionOperation,
} from "./graphql-client.js";
export { AxonClient, type AxonClientOptions } from "./client.js";
export { AxonError, AxonErrorCode } from "./error.js";
export {
  HttpAxonClient,
  TenantClient,
  DatabaseClient,
  AxonHttpError,
  type HttpAxonClientOptions,
} from "./http-client.js";
export {
  AUTH_ERROR_CODES,
  AUTH_ERROR_STATUS,
  AUTH_ERROR_COUNT,
  type AuthErrorCode,
} from "./auth-error-codes.js";
export type {
  Entity,
  Link,
  AuditEntry,
  TraverseResult,
} from "./types.js";

// Re-export generated proto types for advanced usage.
export * as proto from "./generated/axon.js";
export { AxonServiceClient } from "./generated/axon.client.js";
