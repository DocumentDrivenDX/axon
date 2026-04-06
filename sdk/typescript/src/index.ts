/**
 * @axon/client — TypeScript SDK for the Axon data store.
 *
 * Generated gRPC client with a typed convenience layer for entity CRUD,
 * links, traversal, and audit queries.
 */

export { AxonClient, type AxonClientOptions } from "./client.js";
export { AxonError, AxonErrorCode } from "./error.js";
export type {
  Entity,
  Link,
  AuditEntry,
  TraverseResult,
} from "./types.js";

// Re-export generated proto types for advanced usage.
export * as proto from "./generated/axon.js";
export { AxonServiceClient } from "./generated/axon.client.js";
