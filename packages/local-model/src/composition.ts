import { createComposedLocalModelService } from "@lyonbrown/local-model/internal/internal/composed-service";
import type { NativeModelStorageCapability } from "@lyonbrown/local-model/internal/native-storage";
import type { LocalModelService } from "@lyonbrown/local-model/internal/types";

export type {
  NativeInstalledFileResult,
  NativeInstallLockAnchor,
  NativeModelIdentity,
  NativeModelStagingFile,
  NativeModelStorageCapability,
} from "@lyonbrown/local-model/internal/native-storage";

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
