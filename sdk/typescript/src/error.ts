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
  /** Internal server or storage error. */
  Internal = "internal",
  /** Unknown or unclassified error. */
  Unknown = "unknown",
}

/** Structured error from the Axon server. */
export class AxonError extends Error {
  public readonly code: AxonErrorCode;
  public readonly detail: Record<string, unknown>;

  constructor(code: AxonErrorCode, message: string, detail: Record<string, unknown> = {}) {
    super(message);
    this.name = "AxonError";
    this.code = code;
    this.detail = detail;
  }

  /** Parse a gRPC status message into a typed AxonError. */
  static fromGrpcError(err: unknown): AxonError {
    if (err instanceof AxonError) return err;

    const grpcErr = err as { code?: number; message?: string };
    const message = grpcErr.message ?? "Unknown error";

    // Try to parse structured JSON from the message.
    let detail: Record<string, unknown> = {};
    let code = AxonErrorCode.Unknown;

    try {
      const parsed = JSON.parse(message);
      if (typeof parsed === "object" && parsed !== null) {
        detail = parsed;
        if (typeof parsed.code === "string") {
          code = mapErrorCode(parsed.code);
        }
      }
    } catch {
      // Message is not JSON — classify by gRPC status code.
      code = mapGrpcCode(grpcErr.code);
    }

    return new AxonError(code, message, detail);
  }
}

function mapErrorCode(code: string): AxonErrorCode {
  switch (code) {
    case "not_found": return AxonErrorCode.NotFound;
    case "version_conflict": return AxonErrorCode.VersionConflict;
    case "schema_validation": return AxonErrorCode.SchemaValidation;
    case "already_exists": return AxonErrorCode.AlreadyExists;
    case "invalid_argument": return AxonErrorCode.InvalidArgument;
    case "storage_error": return AxonErrorCode.Internal;
    default: return AxonErrorCode.Unknown;
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
