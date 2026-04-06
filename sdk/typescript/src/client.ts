/**
 * High-level Axon client with typed convenience methods.
 *
 * Wraps the generated gRPC client, parsing JSON data fields and translating
 * gRPC errors into typed {@link AxonError} instances.
 */

import { GrpcTransport } from "@protobuf-ts/grpc-transport";
import { ChannelCredentials } from "@grpc/grpc-js";
import { AxonServiceClient } from "./generated/axon.client.js";
import { AxonError } from "./error.js";
import type { Entity, Link, AuditEntry, TraverseResult } from "./types.js";
import type { EntityProto, LinkProto, AuditEntryProto } from "./generated/axon.js";

export interface AxonClientOptions {
  /** Server address in `host:port` format. Default: `localhost:50051`. */
  address?: string;
  /** Use TLS. Default: `false` (insecure for local development). */
  tls?: boolean;
}

export class AxonClient {
  private readonly rpc: AxonServiceClient;

  constructor(options: AxonClientOptions = {}) {
    const address = options.address ?? "localhost:50051";
    const credentials = options.tls
      ? ChannelCredentials.createSsl()
      : ChannelCredentials.createInsecure();

    const transport = new GrpcTransport({ host: address, channelCredentials: credentials });
    this.rpc = new AxonServiceClient(transport);
  }

  // ── Entity CRUD ───────────────────────────────────────────────────────────

  /** Create a new entity. Returns the created entity at version 1. */
  async createEntity(
    collection: string,
    id: string,
    data: Record<string, unknown>,
    actor?: string,
  ): Promise<Entity> {
    try {
      const { response } = await this.rpc.createEntity({
        collection,
        id,
        dataJson: JSON.stringify(data),
        actor: actor ?? "",
      });
      return parseEntity(response.entity!);
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }

  /** Retrieve an entity by collection and ID. */
  async getEntity(collection: string, id: string): Promise<Entity> {
    try {
      const { response } = await this.rpc.getEntity({ collection, id });
      return parseEntity(response.entity!);
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }

  /** Update an entity with optimistic concurrency control. */
  async updateEntity(
    collection: string,
    id: string,
    data: Record<string, unknown>,
    expectedVersion: bigint,
    actor?: string,
  ): Promise<Entity> {
    try {
      const { response } = await this.rpc.updateEntity({
        collection,
        id,
        dataJson: JSON.stringify(data),
        expectedVersion,
        actor: actor ?? "",
      });
      return parseEntity(response.entity!);
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }

  /** Delete an entity. */
  async deleteEntity(
    collection: string,
    id: string,
    actor?: string,
  ): Promise<{ collection: string; id: string }> {
    try {
      const { response } = await this.rpc.deleteEntity({
        collection,
        id,
        actor: actor ?? "",
      });
      return { collection: response.collection, id: response.id };
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }

  // ── Links ─────────────────────────────────────────────────────────────────

  /** Create a typed link between two entities. */
  async createLink(
    sourceCollection: string,
    sourceId: string,
    targetCollection: string,
    targetId: string,
    linkType: string,
    metadata?: Record<string, unknown>,
    actor?: string,
  ): Promise<Link> {
    try {
      const { response } = await this.rpc.createLink({
        sourceCollection,
        sourceId,
        targetCollection,
        targetId,
        linkType,
        metadataJson: metadata ? JSON.stringify(metadata) : "",
        actor: actor ?? "",
      });
      return parseLink(response.link!);
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }

  /** Delete a typed link between two entities. */
  async deleteLink(
    sourceCollection: string,
    sourceId: string,
    targetCollection: string,
    targetId: string,
    linkType: string,
    actor?: string,
  ): Promise<void> {
    try {
      await this.rpc.deleteLink({
        sourceCollection,
        sourceId,
        targetCollection,
        targetId,
        linkType,
        actor: actor ?? "",
      });
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }

  // ── Traversal ─────────────────────────────────────────────────────────────

  /** Traverse links from a starting entity. */
  async traverse(
    collection: string,
    id: string,
    linkType?: string,
    maxDepth?: number,
  ): Promise<TraverseResult> {
    try {
      const { response } = await this.rpc.traverse({
        collection,
        id,
        linkType: linkType ?? "",
        maxDepth: maxDepth ?? 0,
      });
      return {
        entities: response.entities.map(parseEntity),
      };
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }

  // ── Audit ─────────────────────────────────────────────────────────────────

  /** Query audit log entries for a specific entity. */
  async queryAuditByEntity(
    collection: string,
    entityId: string,
  ): Promise<AuditEntry[]> {
    try {
      const { response } = await this.rpc.queryAuditByEntity({
        collection,
        entityId,
      });
      return response.entries.map(parseAuditEntry);
    } catch (err) {
      throw AxonError.fromGrpcError(err);
    }
  }
}

// ── Parsing helpers ─────────────────────────────────────────────────────────

function parseEntity(proto: EntityProto): Entity {
  return {
    collection: proto.collection,
    id: proto.id,
    version: proto.version,
    data: safeParseJson(proto.dataJson),
  };
}

function parseLink(proto: LinkProto): Link {
  return {
    sourceCollection: proto.sourceCollection,
    sourceId: proto.sourceId,
    targetCollection: proto.targetCollection,
    targetId: proto.targetId,
    linkType: proto.linkType,
    metadata: safeParseJson(proto.metadataJson),
  };
}

function parseAuditEntry(proto: AuditEntryProto): AuditEntry {
  return {
    id: proto.id,
    timestampNs: proto.timestampNs,
    collection: proto.collection,
    entityId: proto.entityId,
    version: proto.version,
    mutation: proto.mutation,
    dataBefore: proto.dataBeforeJson ? safeParseJson(proto.dataBeforeJson) : null,
    dataAfter: proto.dataAfterJson ? safeParseJson(proto.dataAfterJson) : null,
    actor: proto.actor,
    transactionId: proto.transactionId,
  };
}

function safeParseJson(json: string): Record<string, unknown> {
  if (!json) return {};
  try {
    return JSON.parse(json) as Record<string, unknown>;
  } catch {
    return {};
  }
}
