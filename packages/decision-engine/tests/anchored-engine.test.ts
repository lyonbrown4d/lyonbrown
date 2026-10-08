import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  createAnchoredDecisionEngine,
  type AnchoredEngineDependencies,
} from "@lyonbrown/decision-engine/internal/anchored-engine";
import type {
  DecisionModelBundleIdentity,
  DecisionModelBundleStoragePort,
} from "@lyonbrown/decision-engine/internal/model-bundle-storage";
import type { DecisionEnginePort } from "@lyonbrown/decision-engine/internal/types";
import { afterEach, describe, expect, it, vi } from "vitest";

const roots: string[] = [];
const bytes = Buffer.from("verified model", "utf8");
const model: DecisionModelBundleIdentity = Object.freeze({
  modelId: "kev-0.6b",
  revision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
  files: Object.freeze([
    Object.freeze({
      path: "config.json",
      sha256: createHash("sha256").update(bytes).digest("hex"),
      maxBytes: bytes.byteLength,
    }),
  ]),
});

afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true });
});

describe("anchored OpenJev engine", () => {
  it("keeps the verified native bundle leased until the engine is disposed", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-anchored-engine-"));
    roots.push(root);
    await writeFile(join(root, "config.json"), bytes);
    let anchored = false;
    const storage = storageWithBundle(root, (value) => {
      anchored = value;
    });
    const fake = fakeEngine();
    const createEngine = vi.fn(() => ({ kind: "configured" as const, engine: fake.engine }));

    const result = await createAnchoredDecisionEngine(storage, model, {
      dependencies: { createEngine },
    });

    expect(result.kind).toBe("configured");
    expect(anchored).toBe(true);
    expect(createEngine).toHaveBeenCalledWith({
      modelPath: root,
      modelId: model.modelId,
      modelRevision: model.revision,
    });
    if (result.kind !== "configured") throw new Error("engine was not configured");
    await result.engine.dispose();
    expect(anchored).toBe(false);
    expect(fake.dispose).toHaveBeenCalledOnce();
  });

  it("surfaces provider disposal failures after releasing the native lease", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-anchored-engine-dispose-"));
    roots.push(root);
    await writeFile(join(root, "config.json"), bytes);
    let anchored = false;
    const storage = storageWithBundle(root, (value) => {
      anchored = value;
    });
    const disposeFailure = new Error("provider dispose failed");
    const engine: DecisionEnginePort = {
      decide: vi.fn().mockResolvedValue({ kind: "undetermined", reason: "provider_unavailable" }),
      dispose: vi.fn().mockRejectedValue(disposeFailure),
    };

    const result = await createAnchoredDecisionEngine(storage, model, {
      dependencies: {
        createEngine: () => ({ kind: "configured", engine }),
      },
    });

    expect(result.kind).toBe("configured");
    expect(anchored).toBe(true);
    if (result.kind !== "configured") throw new Error("engine was not configured");
    await expect(result.engine.dispose()).rejects.toBe(disposeFailure);
    expect(anchored).toBe(false);
  });

  it("reports missing and corrupted bundles without creating a worker", async () => {
    const createEngine = vi.fn<AnchoredEngineDependencies["createEngine"]>();
    const missing = storageWithoutBundle();
    await expect(
      createAnchoredDecisionEngine(missing, model, {
        dependencies: { createEngine },
      }),
    ).resolves.toEqual({ kind: "unavailable", reason: "model_missing" });

    const root = await mkdtemp(join(tmpdir(), "lyonbrown-anchored-engine-corrupt-"));
    roots.push(root);
    await writeFile(join(root, "config.json"), "tampered", "utf8");
    await expect(
      createAnchoredDecisionEngine(storageWithBundle(root), model, {
        dependencies: { createEngine },
      }),
    ).resolves.toEqual({ kind: "unavailable", reason: "model_corrupted" });
    expect(createEngine).not.toHaveBeenCalled();
  });

  it.runIf(process.platform === "win32")(
    "converts a trusted native extended path only at the provider boundary",
    async () => {
      const root = await mkdtemp(join(tmpdir(), "lyonbrown-anchored-engine-native-"));
      roots.push(root);
      await writeFile(join(root, "config.json"), bytes);
      const fake = fakeEngine();
      const createEngine = vi.fn(() => ({ kind: "configured" as const, engine: fake.engine }));

      const result = await createAnchoredDecisionEngine(
        storageWithBundle(`\\\\?\\${root}`),
        model,
        { dependencies: { createEngine } },
      );

      expect(createEngine).toHaveBeenCalledWith({
        modelPath: root,
        modelId: model.modelId,
        modelRevision: model.revision,
      });
      if (result.kind !== "configured") throw new Error("engine was not configured");
      await result.engine.dispose();
    },
  );
});

function fakeEngine(): {
  readonly engine: DecisionEnginePort;
  readonly dispose: ReturnType<typeof vi.fn>;
} {
  const dispose = vi.fn().mockResolvedValue(undefined);
  return {
    dispose,
    engine: {
      decide: vi.fn().mockResolvedValue({ kind: "undetermined", reason: "provider_unavailable" }),
      dispose,
    },
  };
}

function storageWithBundle(
  root: string,
  onAnchor: (anchored: boolean) => void = () => undefined,
): DecisionModelBundleStoragePort {
  return {
    acquireInstallLockAnchor: () => Promise.reject(new Error("not used")),
    createStagingBundle: () => Promise.reject(new Error("not used")),
    async withInstalledBundle<T>(
      _identity: DecisionModelBundleIdentity,
      _signal: AbortSignal,
      use: (directoryPath: string) => Promise<T>,
    ) {
      onAnchor(true);
      try {
        return { kind: "present" as const, value: await use(root) };
      } finally {
        onAnchor(false);
      }
    },
  };
}

function storageWithoutBundle(): DecisionModelBundleStoragePort {
  return {
    acquireInstallLockAnchor: () => Promise.reject(new Error("not used")),
    createStagingBundle: () => Promise.reject(new Error("not used")),
    withInstalledBundle: async () => ({ kind: "missing" }),
  };
}
