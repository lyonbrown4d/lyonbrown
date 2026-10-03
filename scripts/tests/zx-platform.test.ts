import { describe, expect, test, vi } from "vitest";

import { configureZxPlatform } from "../zx-platform.js";

describe("zx platform configuration", () => {
  test("selects PowerShell on Windows", () => {
    const usePowerShell = vi.fn();

    configureZxPlatform("win32", usePowerShell);

    expect(usePowerShell).toHaveBeenCalledOnce();
  });

  test("keeps the native shell on non-Windows platforms", () => {
    const usePowerShell = vi.fn();

    configureZxPlatform("linux", usePowerShell);

    expect(usePowerShell).not.toHaveBeenCalled();
  });
});
