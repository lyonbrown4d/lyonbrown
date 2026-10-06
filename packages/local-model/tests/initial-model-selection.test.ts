import { describe, expect, it, vi } from "vitest";

import { MODEL_CATALOG } from "../src/internal/catalog.ts";
import {
  createDeterministicSelectionOptions,
  selectInitialModel,
} from "../src/internal/initial-model-selection.ts";
import { Deferred, FakeModelIo } from "./support/fakes.ts";

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
});
