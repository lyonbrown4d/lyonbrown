import { createComposedLocalModelService } from "./internal/composed-service.js";
import type { NativeModelStorageCapability } from "./native-storage.js";
import type { LocalModelService } from "./types.js";

export type {
  NativeInstalledFileResult,
  NativeInstallLockAnchor,
  NativeModelIdentity,
  NativeModelStagingFile,
  NativeModelStorageCapability,
} from "./native-storage.js";

export const createLocalModelService = (
  capability: NativeModelStorageCapability,
): LocalModelService => {
  if (!isNativeStorageCapability(capability)) {
    throw new Error("secure_model_capability_required");
  }
  return createComposedLocalModelService(capability);
};

const isNativeStorageCapability = (value: unknown): value is NativeModelStorageCapability =>
  typeof value === "object" &&
  value !== null &&
  ["acquireInstallLockAnchor", "createStagingFile", "withInstalledFile"].every(
    (key) => typeof Reflect.get(value, key) === "function",
  );
