import { describe, expect, it } from "vitest";
import { createFailClosedPosixSecurity } from "../src/posix-provider.js";
import { capture, NativeFailure } from "../src/result.js";

describe("fail-closed platform behavior", () => {
  it("does not pretend POSIX security exists", async () => {
    const security = createFailClosedPosixSecurity();
    expect(security.contract.platform).toBe("posix");
    await expect(security.workspaceIdentity("/tmp/workspace")).resolves.toEqual({
      ok: false,
      code: "io",
    });
    await expect(security.openSecureRunDirectory("/tmp/run")).resolves.toEqual({
      ok: false,
      code: "io",
    });
  });

  it("maps only owned failures and scrubs hostile errors", async () => {
    await expect(capture(() => 42)).resolves.toEqual({ ok: true, value: 42 });
    await expect(
      capture(() => {
        throw new NativeFailure("oversize");
      }),
    ).resolves.toEqual({ ok: false, code: "oversize" });
    await expect(
      capture(() => {
        throw Object.assign(new Error("hostile"), {
          code: "access_denied",
          details: "secret",
        });
      }),
    ).resolves.toEqual({ ok: false, code: "io" });
  });
});
