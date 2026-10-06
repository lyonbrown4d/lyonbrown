import { describe, expect, it } from "vitest";

import type {
  ModelPreset,
  ModelSelector,
} from "@lyonbrown/local-model/internal/internal/contracts";
import { createLocalModelManager } from "@lyonbrown/local-model/internal/internal/manager";
import { createHardwareAwareModelSelector } from "@lyonbrown/local-model/internal/internal/model-selection";
import {
  artifact,
  FakeLlamaEngine,
  FakeModelIo,
  flushTasks,
} from "@lyonbrown/local-model/test/support/fakes";

const small = fixturePreset("fixture-small", 1, 500);
const installedLarge = fixturePreset("fixture-large", 2, 1_500);

describe("local model manager selection", () => {
  it("prefers a verified installed catalog model without invoking remote evaluation", async () => {
    const io = new FakeModelIo();
    io.installedPresetIds.add(installedLarge.id);
    const selector: ModelSelector = {
      select: () => Promise.reject(new Error("selector must not run")),
    };
    const service = createLocalModelManager(io, new FakeLlamaEngine(), undefined, undefined, {
      catalog: [small, installedLarge],
      selector,
    });

    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "ready",
      selectionReason: "installed-preferred",
      model: { id: installedLarge.id },
    });
    await service.dispose();
  });

  it("falls back to the smallest verified model when multiple installed models cannot be scored", async () => {
    const io = new FakeModelIo();
    io.installedPresetIds.add(small.id);
    io.installedPresetIds.add(installedLarge.id);
    const selector = createHardwareAwareModelSelector({
      evaluate: () => Promise.reject(new Error("offline")),
    });
    const service = createLocalModelManager(io, new FakeLlamaEngine(), undefined, undefined, {
      catalog: [small, installedLarge],
      selector,
    });

    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "ready",
      selectionReason: "installed-preferred",
      model: { id: small.id },
    });
    await service.dispose();
  });

  it("publishes a hardware recommendation in the background without downloading it", async () => {
    const io = new FakeModelIo();
    const selector: ModelSelector = {
      select: () => Promise.resolve({ preset: installedLarge, source: "hardware" }),
    };
    const service = createLocalModelManager(io, new FakeLlamaEngine(), undefined, undefined, {
      catalog: [small, installedLarge],
      selector,
    });

    expect(service.status()).toMatchObject({ phase: "checking", selectionReason: null });
    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "setup-required",
      selectionReason: "hardware-compatible",
      model: { id: installedLarge.id },
    });
    expect(io.downloadCalls).toBe(0);

    const result = service.startDownload();
    expect(result.kind).toBe("started");
    await flushTasks();
    expect(io.downloadedPresetIds).toEqual([installedLarge.id]);
    if (result.kind === "started")
      io.downloads[0]?.completion.resolve(artifact(result.operationId));
    await flushTasks();
    await service.dispose();
  });

  it("labels a safe selector fallback without blocking setup", async () => {
    const io = new FakeModelIo();
    const selector: ModelSelector = {
      select: () => Promise.resolve({ preset: small, source: "fallback" }),
    };
    const service = createLocalModelManager(io, new FakeLlamaEngine(), undefined, undefined, {
      catalog: [small, installedLarge],
      selector,
    });

    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "setup-required",
      selectionReason: "conservative-fallback",
      model: { id: small.id },
    });
    await service.dispose();
  });
});

function fixturePreset(
  id: string,
  qualityRank: number,
  expectedDownloadBytes: number,
): ModelPreset {
  return {
    id,
    displayName: id,
    owner: "Fixture",
    repository: "Fixture-GGUF",
    revision: "1111111111111111111111111111111111111111",
    filename: `${id}.gguf`,
    sha256: "a".repeat(64),
    expectedDownloadBytes,
    maxBytes: 2_000,
    minimumMemoryBytes: expectedDownloadBytes * 2,
    minimumContextTokens: 4_096,
    qualityRank,
    license: "apache-2.0",
  };
}
