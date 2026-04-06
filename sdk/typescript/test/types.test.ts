/**
 * Unit tests for the TypeScript SDK types, error handling, and exports.
 *
 * These tests verify the SDK's type structure and error classification
 * without requiring a running Axon server.
 */

import { describe, it, expect } from "vitest";
import { AxonError, AxonErrorCode } from "../src/error.js";
import type { Entity, Link, AuditEntry } from "../src/types.js";

describe("AxonError", () => {
  it("constructs with code and message", () => {
    const err = new AxonError(AxonErrorCode.NotFound, "entity not found");
    expect(err.code).toBe(AxonErrorCode.NotFound);
    expect(err.message).toBe("entity not found");
    expect(err.name).toBe("AxonError");
    expect(err.detail).toEqual({});
  });

  it("constructs with detail", () => {
    const err = new AxonError(AxonErrorCode.VersionConflict, "conflict", {
      expected: 1,
      actual: 2,
    });
    expect(err.detail.expected).toBe(1);
    expect(err.detail.actual).toBe(2);
  });

  it("is instanceof Error", () => {
    const err = new AxonError(AxonErrorCode.Internal, "oops");
    expect(err).toBeInstanceOf(Error);
    expect(err).toBeInstanceOf(AxonError);
  });

  describe("fromGrpcError", () => {
    it("parses structured JSON message", () => {
      const grpcErr = {
        code: 9,
        message: JSON.stringify({
          code: "version_conflict",
          expected: 1,
          actual: 3,
        }),
      };
      const err = AxonError.fromGrpcError(grpcErr);
      expect(err.code).toBe(AxonErrorCode.VersionConflict);
      expect(err.detail.expected).toBe(1);
    });

    it("falls back to gRPC code mapping for plain messages", () => {
      const grpcErr = { code: 5, message: "tasks/t-001" };
      const err = AxonError.fromGrpcError(grpcErr);
      expect(err.code).toBe(AxonErrorCode.NotFound);
    });

    it("returns unknown for unrecognized codes", () => {
      const grpcErr = { code: 99, message: "wat" };
      const err = AxonError.fromGrpcError(grpcErr);
      expect(err.code).toBe(AxonErrorCode.Unknown);
    });

    it("passes through existing AxonError unchanged", () => {
      const original = new AxonError(AxonErrorCode.AlreadyExists, "dup");
      const result = AxonError.fromGrpcError(original);
      expect(result).toBe(original);
    });
  });
});

describe("AxonErrorCode", () => {
  it("has expected variants", () => {
    expect(AxonErrorCode.NotFound).toBe("not_found");
    expect(AxonErrorCode.VersionConflict).toBe("version_conflict");
    expect(AxonErrorCode.SchemaValidation).toBe("schema_validation");
    expect(AxonErrorCode.AlreadyExists).toBe("already_exists");
    expect(AxonErrorCode.InvalidArgument).toBe("invalid_argument");
    expect(AxonErrorCode.Internal).toBe("internal");
    expect(AxonErrorCode.Unknown).toBe("unknown");
  });
});

describe("Type shapes", () => {
  it("Entity type is structurally correct", () => {
    const entity: Entity = {
      collection: "tasks",
      id: "t-001",
      version: 1n,
      data: { title: "hello" },
    };
    expect(entity.collection).toBe("tasks");
    expect(entity.version).toBe(1n);
  });

  it("Link type is structurally correct", () => {
    const link: Link = {
      sourceCollection: "users",
      sourceId: "u-001",
      targetCollection: "tasks",
      targetId: "t-001",
      linkType: "owns",
      metadata: {},
    };
    expect(link.linkType).toBe("owns");
  });

  it("AuditEntry type is structurally correct", () => {
    const entry: AuditEntry = {
      id: 1n,
      timestampNs: 1000n,
      collection: "tasks",
      entityId: "t-001",
      version: 1n,
      mutation: "entity.create",
      dataBefore: null,
      dataAfter: { title: "hello" },
      actor: "agent-1",
      transactionId: "",
    };
    expect(entry.mutation).toBe("entity.create");
    expect(entry.dataBefore).toBeNull();
  });
});
