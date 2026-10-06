import {
  CATALOG_FALLBACK_PRESET,
  MODEL_CATALOG,
} from "@lyonbrown/local-model/internal/internal/catalog";
import type { ModelPreset } from "@lyonbrown/local-model/internal/internal/contracts";
import type { LocalModelDescriptor } from "@lyonbrown/local-model/internal/types";

export const descriptorFor = (preset: ModelPreset): LocalModelDescriptor => ({
  id: preset.id,
  displayName: preset.displayName,
  expectedDownloadBytes: preset.expectedDownloadBytes,
  maxBytes: preset.maxBytes,
  minimumMemoryBytes: preset.minimumMemoryBytes,
  sha256: preset.sha256,
});

/** Initial conservative descriptor shown while hardware selection runs in the background. */
export const DEFAULT_LOCAL_MODEL = Object.freeze(descriptorFor(CATALOG_FALLBACK_PRESET));

/** Trusted, path-free descriptors used by diagnostics and black-box verification. */
export const TRUSTED_LOCAL_MODELS: readonly LocalModelDescriptor[] = Object.freeze(
  MODEL_CATALOG.map((preset) => Object.freeze(descriptorFor(preset))),
);
