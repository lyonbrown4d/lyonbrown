import { CATALOG_FALLBACK_PRESET } from "./internal/catalog.ts";
import type { ModelPreset } from "./internal/contracts.ts";
import type { LocalModelDescriptor } from "./types.ts";

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
