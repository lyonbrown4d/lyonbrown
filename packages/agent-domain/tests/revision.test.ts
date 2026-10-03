import { describe, expect, expectTypeOf, it } from "vitest";

import { asRevision, incrementRevision, type Revision } from "../src/index.js";
import { captureError } from "./test-error.js";

describe("uint64 revisions", () => {
  it("keeps revisions as bigint beyond Number.MAX_SAFE_INTEGER", () => {
    const value = asRevision(18_446_744_073_709_551_615n);
    expect(value).toBe(18_446_744_073_709_551_615n);
    expectTypeOf(value).toEqualTypeOf<Revision>();
  });

  it("rejects negative revisions and non-bigint input", () => {
    expect(captureError(() => asRevision(-1n))).toMatchObject({ code: "invalid_argument" });
    expect(captureError(() => asRevision(1 as unknown as bigint))).toMatchObject({
      code: "invalid_argument",
    });
  });

  it("rejects increment past uint64 max", () => {
    expect(
      captureError(() => incrementRevision(asRevision(18_446_744_073_709_551_615n))),
    ).toMatchObject({ code: "resource_exhausted" });
  });
});
