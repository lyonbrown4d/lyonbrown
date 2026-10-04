import { DEFAULT_PRESET } from "./internal/catalog.js";
import type { LocalModelDescriptor } from "./types.js";

export const DEFAULT_LOCAL_MODEL = Object.freeze({
  id: DEFAULT_PRESET.id,
  displayName: DEFAULT_PRESET.displayName,
  expectedDownloadBytes: DEFAULT_PRESET.expectedDownloadBytes,
  minimumMemoryBytes: DEFAULT_PRESET.minimumMemoryBytes,
} as const satisfies LocalModelDescriptor);
