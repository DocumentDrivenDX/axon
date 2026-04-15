/**
 * AUTO-MAINTAINED: mirror of schema/auth-errors.manifest.json
 *
 * If you edit this file, also edit the manifest. The parity test in
 * test/auth-error-manifest.test.ts will fail if the two drift.
 */

export const AUTH_ERROR_CODES = [
  "unauthenticated",
  "credential_malformed",
  "credential_invalid",
  "credential_expired",
  "credential_not_yet_valid",
  "credential_revoked",
  "credential_foreign_issuer",
  "credential_wrong_tenant",
  "user_suspended",
  "not_a_tenant_member",
  "database_not_granted",
  "op_not_granted",
  "grants_exceed_issuer_role",
  "grants_exceed_role",
  "grants_malformed",
] as const;

export type AuthErrorCode = (typeof AUTH_ERROR_CODES)[number];

/** HTTP status by error code (matches ADR-018 §4). */
export const AUTH_ERROR_STATUS: Record<AuthErrorCode, 401 | 403> = {
  unauthenticated: 401,
  credential_malformed: 401,
  credential_invalid: 401,
  credential_expired: 401,
  credential_not_yet_valid: 401,
  credential_revoked: 401,
  credential_foreign_issuer: 401,
  credential_wrong_tenant: 403,
  user_suspended: 401,
  not_a_tenant_member: 403,
  database_not_granted: 403,
  op_not_granted: 403,
  grants_exceed_issuer_role: 401,
  grants_exceed_role: 403,
  grants_malformed: 401,
};

export const AUTH_ERROR_COUNT = AUTH_ERROR_CODES.length; // 15
