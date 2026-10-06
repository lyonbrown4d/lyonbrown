import { describe, expect, it, vi } from "vitest";

import type { NativeModelStorageCapability } from "@lyonbrown/local-model/composition";

interface ModelStoragePlatformModule {
  selectWindowsModelStorageFactory?: (
    platform: NodeJS.Platform,
    loadWindows: () => Promise<{
      createWindowsModelStorageCapability: (directory: string) => NativeModelStorageCapability;
    }>,
    unavailable: (directory: string) => NativeModelStorageCapability,
  ) => Promise<(directory: string) => NativeModelStorageCapability>;
}

describe("native model storage platform contract", () => {
  it("does not load Windows DLLs on POSIX and fails closed", async () => {
    const module = (await import(
      "@lyonbrown/control-plane-native/internal/platform"
    )) as ModelStoragePlatformModule;
    const loadWindows = vi.fn(() =>
      Promise.reject(new Error("Windows DLLs must not load on POSIX")),
    );
    const unavailable = (): NativeModelStorageCapability => {
      throw new Error("native_model_storage_unavailable");
    };

    const factory = await module.selectWindowsModelStorageFactory?.(
      "linux",
      loadWindows,
      unavailable,
    );

    expect(loadWindows).not.toHaveBeenCalled();
    expect(() => factory?.("/var/lib/lyonbrown/models")).toThrow(
      "native_model_storage_unavailable",
    );
  });

  it("selects the supplied Windows model storage factory", async () => {
    const module = (await import(
      "@lyonbrown/control-plane-native/internal/platform"
    )) as ModelStoragePlatformModule;
    const capability = {} as NativeModelStorageCapability;
    const createWindowsModelStorageCapability = vi.fn(() => capability);
    const loadWindows = vi.fn(() => Promise.resolve({ createWindowsModelStorageCapability }));
    const unavailable = vi.fn(() => {
      throw new Error("unavailable must not be selected");
    });

    const factory = await module.selectWindowsModelStorageFactory?.(
      "win32",
      loadWindows,
      unavailable,
    );

    expect(factory?.("C:\\private\\models")).toBe(capability);
    expect(loadWindows).toHaveBeenCalledOnce();
    expect(createWindowsModelStorageCapability).toHaveBeenCalledWith("C:\\private\\models");
    expect(unavailable).not.toHaveBeenCalled();
  });
});
