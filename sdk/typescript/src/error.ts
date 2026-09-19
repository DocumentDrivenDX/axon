/**
 * Typed error handling for the Axon client SDK.
 */

/** Known Axon error codes that agents can match on programmatically. */
export enum AxonErrorCode {
  /** Entity or resource not found. */
  NotFound = "not_found",
  /** Optimistic concurrency version conflict. */
  VersionConflict = "version_conflict",
  /** Entity data does not match the collection schema. */
  SchemaValidation = "schema_validation",
  /** Resource already exists (duplicate create). */
  AlreadyExists = "already_exists",
  /** Invalid argument or request format. */
  InvalidArgument = "invalid_argument",
  /** Generic access to an Axon-owned namespace is forbidden. */
  ReservedNamespace = "reserved_namespace",
  /** Internal server or storage error. */
  Internal = "internal",
  /** Unknown or unclassified error. */
  Unknown = "unknown",
}

/** Structured error from the Axon server. */
export class AxonError extends Error {
  public readonly code: AxonErrorCode | string;
  public readonly detail: Record<string, unknown>;
  public readonly reason?: string;

  constructor(
    code: AxonErrorCode | string,
    message: string,
    detail: Record<string, unknown> = {},
    reason?: string,
  ) {
    super(message);
    this.name = "AxonError";
    this.code = code;
    this.detail = detail;
    this.reason = reason;
  }

  /** Parse a gRPC status message into a typed AxonError. */
  static fromGrpcError(err: unknown): AxonError {
    if (err instanceof AxonError) return err;

    const grpcErr = err as { code?: number; message?: string };
    const message = grpcErr.message ?? "Unknown error";

    // Try to parse structured JSON from the message.
    let detail: Record<string, unknown> = {};
    let code: AxonErrorCode | string = AxonErrorCode.Unknown;
    let reason: string | undefined;

    try {
      const parsed = asRecord(JSON.parse(message));
      if (parsed) {
        if (typeof parsed.code === "string") {
          code = mapErrorCode(parsed.code);
        }
        if (typeof parsed.reason === "string") {
          reason = parsed.reason;
        }
        detail = asRecord(parsed.detail) ?? parsed;
      }
    } catch {
      // Message is not JSON — classify by gRPC status code.
      code = mapGrpcCode(grpcErr.code);
    }

    return new AxonError(code, message, detail, reason);
  }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function mapErrorCode(code: string): AxonErrorCode | string {
  switch (code) {
    case "not_found": return AxonErrorCode.NotFound;
    case "version_conflict": return AxonErrorCode.VersionConflict;
    case "schema_validation": return AxonErrorCode.SchemaValidation;
    case "already_exists": return AxonErrorCode.AlreadyExists;
    case "invalid_argument": return AxonErrorCode.InvalidArgument;
    case "reserved_namespace": return AxonErrorCode.ReservedNamespace;
    case "storage_error": return AxonErrorCode.Internal;
    default: return code;
  }
}

function mapGrpcCode(code: number | undefined): AxonErrorCode {
  switch (code) {
    case 5: return AxonErrorCode.NotFound;         // NOT_FOUND
    case 9: return AxonErrorCode.VersionConflict;   // FAILED_PRECONDITION
    case 3: return AxonErrorCode.InvalidArgument;   // INVALID_ARGUMENT
    case 6: return AxonErrorCode.AlreadyExists;     // ALREADY_EXISTS
    case 13: return AxonErrorCode.Internal;          // INTERNAL
    default: return AxonErrorCode.Unknown;
  }
}
