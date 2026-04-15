import { describe, it, expect } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import {
  AUTH_ERROR_CODES,
  AUTH_ERROR_STATUS,
  AUTH_ERROR_COUNT,
} from "../src/auth-error-codes.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

describe("auth error manifest parity", () => {
  it("matches the canonical manifest", () => {
    const manifestPath = path.resolve(
      __dirname,
      "../../../schema/auth-errors.manifest.json",
    );
    const raw = fs.readFileSync(manifestPath, "utf8");
    const json = JSON.parse(raw);
    expect(json.variants.length).toBe(AUTH_ERROR_COUNT);

    for (const v of json.variants) {
      expect(AUTH_ERROR_CODES).toContain(v.code);
      expect(AUTH_ERROR_STATUS[v.code as keyof typeof AUTH_ERROR_STATUS]).toBe(
        v.status,
      );
    }
  });
});
