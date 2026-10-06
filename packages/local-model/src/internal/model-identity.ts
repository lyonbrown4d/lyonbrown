import type { NativeModelIdentity } from "@lyonbrown/local-model/internal/native-storage";
import { DEFAULT_PRESET } from "@lyonbrown/local-model/internal/internal/catalog";
import type { ModelPreset } from "@lyonbrown/local-model/internal/internal/contracts";

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
