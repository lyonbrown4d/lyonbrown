import { describe, expect, expectTypeOf, it } from "vitest";

import {
  DomainError,
  domainError,
  type DomainErrorCode,
} from "@lyonbrown/agent-domain/internal/index";

describe("typed domain errors", () => {
  it("supports every transport-independent error code", () => {
    const codes = [
      "invalid_argument",
      "unauthenticated",
      "failed_precondition",
      "stale_server_epoch",
      "not_found",
      "already_exists",
      "resource_exhausted",
      "cancelled",
      "deadline_exceeded",
      "internal",
    ] as const satisfies readonly DomainErrorCode[];
    expect(codes).toHaveLength(10);
  });

  it("exposes code and structured details independently of its human message", () => {
    const error = domainError("stale_server_epoch", "服务器已经重启", {
      expectedServerInstanceId: "old",
      actualServerInstanceId: "new",
    });
    expect(error).toBeInstanceOf(DomainError);
    expect(error.code).toBe("stale_server_epoch");
    expect(error.details).toEqual({
      expectedServerInstanceId: "old",
      actualServerInstanceId: "new",
    });
    expectTypeOf(error.details.expectedServerInstanceId).toEqualTypeOf<string | undefined>();
  });

  it("does not retain a mutable details object", () => {
    const details = { field: "prompt", reason: "empty" };
    const error = domainError("invalid_argument", "invalid", details);
    details.reason = "changed";
    expect(error.details.reason).toBe("empty");
    expect(Object.isFrozen(error.details)).toBe(true);
  });
});
