import { resolve } from "node:path";

import type { DecisionModelBundleStoragePort } from "@lyonbrown/decision-engine/composition";
import { describe, expect, it, vi } from "vitest";

import {
  executeDecisionInstall,
  type DecisionInstallWorkerDependencies,
} from "@lyonbrown/agentd/internal/decision-install-worker";
import type { DecisionInstallProgress } from "@lyonbrown/agentd/internal/decision-install-process";

const ROOT = resolve(import.meta.dirname, "fixtures", "decision-models");
const OPERATION = "018f47d6-7c12-7b19-a0b6-ff87748f42bc";

function fixture() {
  const storage = {} as DecisionModelBundleStoragePort;
  const release = vi.fn();
  const createStorage = vi.fn(() => storage);
  const install = vi.fn<DecisionInstallWorkerDependencies["install"]>(async (_storage, options) => {
    options.onPhase?.("checking");
    options.onProgress?.({ downloadedBytes: 10, totalBytes: 100, filePath: "weights" });
    options.onProgress?.({ downloadedBytes: 10, totalBytes: 100, filePath: "weights" });
    options.onProgress?.({ downloadedBytes: 20, totalBytes: 100, filePath: "weights" });
    options.onPhase?.("installing");
    return { kind: "installed", modelPath: ROOT };
  });
  const dependencies: DecisionInstallWorkerDependencies = {
    createStorage,
    install,
    acquireProxy: () => release,
  };
  return { dependencies, install, createStorage, release };
}

describe("built installer task", () => {
  it("uses validated storage, bounds progress to percentage changes and completes after installation", async () => {
    const f = fixture();
    const report = vi.fn<(value: DecisionInstallProgress) => void>();
    const complete = vi.fn(async () => undefined);
    const signal = new AbortController().signal;
    await executeDecisionInstall(ROOT, OPERATION, signal, report, complete, f.dependencies);
    expect(f.createStorage).toHaveBeenCalledWith(ROOT);
    expect(f.install).toHaveBeenCalledWith(
      expect.any(Object),
      expect.objectContaining({ operationId: OPERATION, signal }),
    );
    expect(report.mock.calls.map(([progress]) => progress.phase)).toEqual([
      "checking",
      "downloading",
      "downloading",
      "installing",
    ]);
    expect(complete).toHaveBeenCalledOnce();
    expect(f.release).toHaveBeenCalledOnce();
  });

  it.each([
    [undefined, OPERATION],
    ["relative/path", OPERATION],
    [ROOT, undefined],
    [ROOT, "not-a-uuid"],
  ])("rejects invalid configuration %# before I/O", async (root, operation) => {
    const f = fixture();
    await expect(
      executeDecisionInstall(
        root,
        operation,
        AbortSignal.abort(),
        vi.fn(),
        vi.fn(),
        f.dependencies,
      ),
    ).rejects.toThrow("invalid_install_configuration");
    expect(f.createStorage).not.toHaveBeenCalled();
  });

  it("releases proxy resources and never reports success after installer failure", async () => {
    const f = fixture();
    f.install.mockRejectedValue(new Error("download failed"));
    const complete = vi.fn();
    await expect(
      executeDecisionInstall(
        ROOT,
        OPERATION,
        AbortSignal.abort(),
        vi.fn(),
        complete,
        f.dependencies,
      ),
    ).rejects.toThrow("download failed");
    expect(complete).not.toHaveBeenCalled();
    expect(f.release).toHaveBeenCalledOnce();
  });
});
