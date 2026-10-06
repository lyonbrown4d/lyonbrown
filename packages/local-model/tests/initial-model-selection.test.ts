import { describe, expect, it, vi } from "vitest";

import { MODEL_CATALOG } from "@lyonbrown/local-model/internal/internal/catalog";
import {
  createDeterministicSelectionOptions,
  selectInitialModel,
} from "@lyonbrown/local-model/internal/internal/initial-model-selection";
import { Deferred, FakeModelIo } from "@lyonbrown/local-model/test/support/fakes";

describe("initial model selection boundaries", () => {
  it("rejects an empty trusted catalog without inspecting storage", async () => {
    const io = new FakeModelIo();
    const options = createDeterministicSelectionOptions(MODEL_CATALOG[0]);

    await expect(
      selectInitialModel(io, { ...options, catalog: [] }, new AbortController().signal),
    ).rejects.toThrow("trusted_model_catalog_empty");
    expect(io.inspectCalls).toBe(0);
  });

  it("fails cache inspection when every trusted identity check rejects", async () => {
    const io = new FakeModelIo();
    io.inspection = new Deferred<boolean>();
    const selector = vi.fn(() =>
      Promise.resolve({ preset: MODEL_CATALOG[0], source: "fallback" as const }),
    );
    const selection = selectInitialModel(
      io,
      { catalog: MODEL_CATALOG.slice(0, 2), selector: { select: selector } },
      new AbortController().signal,
    );

    io.inspection.reject(new Error("private storage details"));

    await expect(selection).rejects.toThrow("cache_inspection_failed");
    expect(selector).not.toHaveBeenCalled();
  });

  it("limits a first-run hardware recommendation to the configured small-download budget", async () => {
    const io = new FakeModelIo();
    const selector = vi.fn((catalog: readonly (typeof MODEL_CATALOG)[number][]) =>
      Promise.resolve({ preset: catalog.at(-1) ?? MODEL_CATALOG[0], source: "hardware" as const }),
    );

    const selection = await selectInitialModel(
      io,
      {
        catalog: MODEL_CATALOG,
        selector: { select: selector },
        maxAutoDownloadBytes: 1024 ** 3,
      },
      new AbortController().signal,
    );

    expect(selector).toHaveBeenCalledWith([MODEL_CATALOG[0]], expect.any(AbortSignal));
    expect(selection.preset).toBe(MODEL_CATALOG[0]);
  });

  it("fails closed when every first-run model exceeds the download budget", async () => {
    const io = new FakeModelIo();
    const selector = vi.fn(() =>
      Promise.resolve({ preset: MODEL_CATALOG[0], source: "fallback" as const }),
    );

    await expect(
      selectInitialModel(
        io,
        {
          catalog: MODEL_CATALOG,
          selector: { select: selector },
          maxAutoDownloadBytes: 1,
        },
        new AbortController().signal,
      ),
    ).rejects.toThrow("auto_download_budget_exhausted");
    expect(selector).not.toHaveBeenCalled();
  });

  it("still prefers an already-installed larger trusted model outside the download budget", async () => {
    const io = new FakeModelIo();
    const installed = MODEL_CATALOG[2];
    io.installedPresetIds.add(installed.id);
    const selector = vi.fn(() =>
      Promise.resolve({ preset: installed, source: "hardware" as const }),
    );

    const selection = await selectInitialModel(
      io,
      {
        catalog: MODEL_CATALOG,
        selector: { select: selector },
        maxAutoDownloadBytes: 1024 ** 3,
      },
      new AbortController().signal,
    );

    expect(selector).not.toHaveBeenCalled();
    expect(selection).toMatchObject({ preset: installed, installed: true });
  });
});
