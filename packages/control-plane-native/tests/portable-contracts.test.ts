import { describe, expect, it } from "vitest";
import { createFailClosedPosixSecurity } from "../src/posix-provider.js";
import { capture, NativeFailure } from "../src/result.js";

type SecurityFactory =
  ReturnType<typeof createFailClosedPosixSecurity> extends infer Security ? () => Security : never;

interface PlatformModule {
  selectPlatformSecurity?: (
    platform: NodeJS.Platform,
    windows: SecurityFactory,
    posix: SecurityFactory,
  ) => ReturnType<SecurityFactory>;
  selectWindowsFactory?: (
    platform: NodeJS.Platform,
    loadWindows: () => Promise<{ createWindowsSecurity: SecurityFactory }>,
    unavailable: SecurityFactory,
  ) => Promise<SecurityFactory>;
}

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

  it("selects injected Windows and POSIX providers without native coupling", async () => {
    const module = (await import("../src/platform.js")) as PlatformModule;
    const windowsSecurity = createFailClosedPosixSecurity();
    const posixSecurity = createFailClosedPosixSecurity();
    const windows = () => windowsSecurity;
    const posix = () => posixSecurity;

    expect(module.selectPlatformSecurity?.("win32", windows, posix)).toBe(windowsSecurity);
    expect(module.selectPlatformSecurity?.("linux", windows, posix)).toBe(posixSecurity);
  });

  it("loads the Windows factory only for Windows", async () => {
    const module = (await import("../src/platform.js")) as PlatformModule;
    const windows = () => createFailClosedPosixSecurity();
    const unavailable = () => createFailClosedPosixSecurity();

    const nonWindows = module.selectWindowsFactory
      ? await module.selectWindowsFactory(
          "linux",
          () => Promise.reject(new Error("must not load Windows DLLs")),
          unavailable,
        )
      : undefined;
    const onWindows = module.selectWindowsFactory
      ? await module.selectWindowsFactory(
          "win32",
          () => Promise.resolve({ createWindowsSecurity: windows }),
          unavailable,
        )
      : undefined;
    expect(nonWindows).toBe(unavailable);
    expect(onWindows).toBe(windows);
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
