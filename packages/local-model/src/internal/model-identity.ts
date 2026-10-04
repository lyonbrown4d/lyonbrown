import type { NativeModelIdentity } from "../native-storage.js";
import { DEFAULT_PRESET } from "./catalog.js";

export const DEFAULT_MODEL_IDENTITY: NativeModelIdentity = Object.freeze({
  modelId: DEFAULT_PRESET.id,
  revision: DEFAULT_PRESET.revision,
  sha256: DEFAULT_PRESET.sha256,
  filename: DEFAULT_PRESET.filename,
  maxBytes: DEFAULT_PRESET.maxBytes,
});
