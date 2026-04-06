/**
 * High-level typed representations of Axon domain objects.
 *
 * These types parse the raw JSON strings from protobuf messages into
 * structured TypeScript objects.
 */

/** A versioned entity stored in an Axon collection. */
export interface Entity {
  collection: string;
  id: string;
  version: bigint;
  data: Record<string, unknown>;
}

/** A typed directional edge between two entities. */
export interface Link {
  sourceCollection: string;
  sourceId: string;
  targetCollection: string;
  targetId: string;
  linkType: string;
  metadata: Record<string, unknown>;
}

/** An immutable audit log entry. */
export interface AuditEntry {
  id: bigint;
  timestampNs: bigint;
  collection: string;
  entityId: string;
  version: bigint;
  mutation: string;
  dataBefore: Record<string, unknown> | null;
  dataAfter: Record<string, unknown> | null;
  actor: string;
  transactionId: string;
}

/** Result of a link traversal query. */
export interface TraverseResult {
  entities: Entity[];
}
