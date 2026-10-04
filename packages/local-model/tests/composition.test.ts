import { describe, expect, it, vi } from "vitest";

import { createLocalModelService, type SecureLocalModelCapability } from "../src/composition.js";
import { Deferred, flushTasks } from "./support/fakes.js";

describe("secure local-model composition", () => {
  it("fails closed without a native secure capability", () => {
    expect(() =>
      createLocalModelService(undefined as unknown as SecureLocalModelCapability),
    ).toThrow("secure_model_capability_required");
  });

  it("holds the verified native anchor until node-llama loading completes", async () => {
    const loadCompletion = new Deferred<{ dispose(): Promise<void> }>();
    let released = false;
    const release = vi.fn(() => {
      released = true;
      return Promise.resolve();
    });
    const load = vi.fn((signal: AbortSignal) => {
      void signal;
      expect(released).toBe(false);
      return loadCompletion.promise;
    });
    const capability = readyCapability({ load, release });
    const service = createLocalModelService(capability);
    await flushTasks();

    const result = service.load();
    expect(release).not.toHaveBeenCalled();
    loadCompletion.resolve({ dispose: () => Promise.resolve() });

    await expect(result).resolves.toMatchObject({ kind: "ready" });
    expect(release).toHaveBeenCalledOnce();
  });

  it("releases the verified native anchor when loading fails", async () => {
    const release = vi.fn(() => Promise.resolve());
    const capability = readyCapability({
      load: () => Promise.reject(new Error("private native failure")),
      release,
    });
    const service = createLocalModelService(capability);
    await flushTasks();

    await expect(service.load()).resolves.toEqual({
      kind: "unavailable",
      code: "native_runtime_unavailable",
    });
    expect(release).toHaveBeenCalledOnce();
  });
});

const readyCapability = (
  anchor: Awaited<ReturnType<SecureLocalModelCapability["acquireVerifiedModelAnchor"]>>,
): SecureLocalModelCapability => ({
  inspectInstalled: () => Promise.resolve(true),
  acquireInstallLock: () => Promise.reject(new Error("unused")),
  download: () => Promise.reject(new Error("unused")),
  verify: () => Promise.reject(new Error("unused")),
  activate: () => Promise.reject(new Error("unused")),
  cleanup: () => Promise.resolve(),
  acquireVerifiedModelAnchor: () => Promise.resolve(anchor),
});
