import type { LocalModelSelectionReason } from "@lyonbrown/local-model/internal/types";
import type {
  LocalModelIo,
  ModelPreset,
  ModelSelector,
} from "@lyonbrown/local-model/internal/internal/contracts";

export interface ModelSelectionOptions {
  readonly catalog: readonly ModelPreset[];
  readonly selector: ModelSelector;
}

export interface InitialModelSelection {
  readonly preset: ModelPreset;
  readonly reason: LocalModelSelectionReason;
  readonly installed: boolean;
}

export const createDeterministicSelectionOptions = (
  fallback: ModelPreset,
): ModelSelectionOptions => ({
  catalog: [fallback],
  selector: {
    select(catalog) {
      const preset = catalog[0];
      if (preset === undefined) throw new Error("trusted_model_catalog_empty");
      return Promise.resolve({ preset, source: "fallback" });
    },
  },
});

export const selectInitialModel = async (
  io: LocalModelIo,
  options: ModelSelectionOptions,
  signal: AbortSignal,
): Promise<InitialModelSelection> => {
  if (options.catalog.length === 0) throw new Error("trusted_model_catalog_empty");
  const inspections = await Promise.allSettled(
    options.catalog.map((preset) => io.inspectInstalled(preset, signal)),
  );
  throwIfCancelled(signal);
  const installed = options.catalog.filter(
    (_preset, index) => inspections[index]?.status === "fulfilled" && inspections[index].value,
  );
  if (installed.length > 0) {
    const preset =
      installed.length === 1
        ? requireFirstPreset(installed)
        : (await options.selector.select(installed, signal)).preset;
    return { preset, reason: "installed-preferred", installed: true };
  }
  if (inspections.every((result) => result.status === "rejected")) {
    throw new Error("cache_inspection_failed");
  }
  const selection = await options.selector.select(options.catalog, signal);
  return {
    preset: selection.preset,
    reason: selection.source === "hardware" ? "hardware-compatible" : "conservative-fallback",
    installed: false,
  };
};

const requireFirstPreset = (presets: readonly ModelPreset[]): ModelPreset => {
  const selected = presets[0];
  if (selected === undefined) throw new Error("installed_model_not_found");
  return selected;
};

const throwIfCancelled = (signal: AbortSignal): void => {
  if (signal.aborted) throw new Error("model_selection_cancelled");
};
