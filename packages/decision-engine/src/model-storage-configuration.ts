import { normalize } from "node:path";

import { isFullyQualifiedLocalPath } from "@lyonbrown/decision-engine/internal/local-path";

export const DECISION_MODEL_ROOT_ENV = "LYONBROWN_DECISION_MODEL_ROOT";

export type DecisionModelStorageConfiguration =
  | { readonly kind: "configured"; readonly rootDirectory: string }
  | {
      readonly kind: "unavailable";
      readonly reason: "missing_model_root" | "invalid_model_root";
    };

export function readDecisionModelStorageConfiguration(
  environment: Readonly<NodeJS.ProcessEnv>,
): DecisionModelStorageConfiguration {
  const configuredRoot = environment[DECISION_MODEL_ROOT_ENV];
  if (configuredRoot === undefined) {
    return { kind: "unavailable", reason: "missing_model_root" };
  }
  if (!isFullyQualifiedLocalPath(configuredRoot)) {
    return { kind: "unavailable", reason: "invalid_model_root" };
  }
  return { kind: "configured", rootDirectory: normalize(configuredRoot) };
}
