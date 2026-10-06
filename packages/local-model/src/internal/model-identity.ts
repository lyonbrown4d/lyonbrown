import type { NativeModelIdentity } from "../native-storage.ts";
import { DEFAULT_PRESET } from "./catalog.ts";
import type { ModelPreset } from "./contracts.ts";

export const modelIdentityFor = (preset: ModelPreset): NativeModelIdentity => ({
  modelId: preset.id,
  revision: preset.revision,
  sha256: preset.sha256,
  filename: preset.filename,
  maxBytes: preset.maxBytes,
});

export const DEFAULT_MODEL_IDENTITY: NativeModelIdentity = Object.freeze(
  modelIdentityFor(DEFAULT_PRESET),
);
