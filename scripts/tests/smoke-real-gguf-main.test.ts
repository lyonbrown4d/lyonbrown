import { afterEach, describe, expect, test, vi } from "vitest";
import type * as FileSystemPromises from "node:fs/promises";

const mocks = vi.hoisted(() => ({
  command: vi.fn(),
  makeTemporaryRoot: vi.fn(),
  usePowerShell: vi.fn(),
}));

vi.mock("zx", () => ({
  $: mocks.command,
  usePowerShell: mocks.usePowerShell,
}));

vi.mock("node:fs/promises", async (importOriginal) => {
  const original = await importOriginal<typeof FileSystemPromises>();
  return { ...original, mkdtemp: mocks.makeTemporaryRoot };
});

import { main } from "@lyonbrown/workspace/internal/smoke-real-gguf";

describe("real GGUF smoke main guard", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    mocks.command.mockClear();
    mocks.makeTemporaryRoot.mockClear();
  });

  test("configures Windows zx and rejects before build or temporary storage", async () => {
    const write = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
    const startedAt = performance.now();

    await expect(main([], {})).rejects.toThrow("REAL_GGUF_SMOKE_DISABLED");

    expect(performance.now() - startedAt).toBeLessThan(1_000);
    expect(mocks.usePowerShell).toHaveBeenCalledOnce();
    expect(mocks.command).not.toHaveBeenCalled();
    expect(mocks.makeTemporaryRoot).not.toHaveBeenCalled();
    expect(write).toHaveBeenCalledWith(expect.stringContaining("REAL_GGUF_SMOKE_DISABLED"));
  });
});
