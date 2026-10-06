import { DEFAULT_PRESET } from "./internal/catalog.ts";
import type { LocalModelDescriptor } from "./types.ts";

export const DEFAULT_LOCAL_MODEL = Object.freeze({
  id: DEFAULT_PRESET.id,
  displayName: DEFAULT_PRESET.displayName,
  expectedDownloadBytes: DEFAULT_PRESET.expectedDownloadBytes,
  maxBytes: DEFAULT_PRESET.maxBytes,
  minimumMemoryBytes: DEFAULT_PRESET.minimumMemoryBytes,
  sha256: DEFAULT_PRESET.sha256,
} as const satisfies LocalModelDescriptor);
