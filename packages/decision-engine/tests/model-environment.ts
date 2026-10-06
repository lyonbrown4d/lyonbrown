import { join, resolve } from "node:path";

import { parse, populate } from "dotenv";

import {
  DECISION_MODEL_ROOT_ENV,
  readDecisionModelStorageConfiguration,
} from "@lyonbrown/decision-engine/internal/model-storage-configuration";
import type { LocalOpenJevConfiguration } from "@lyonbrown/decision-engine/internal/open-jev-configuration";

const modelRevision = "0123456789abcdef0123456789abcdef01234567";

export function environmentFromDotenv(rootDirectory: string): NodeJS.ProcessEnv {
  const environment: NodeJS.ProcessEnv = {};
  const encodedRoot = JSON.stringify(rootDirectory.replaceAll("\\", "/"));
  const parsed = parse(`${DECISION_MODEL_ROOT_ENV}=${encodedRoot}`);
  populate(environment, parsed, { override: false });
  return environment;
}

export function injectedDecisionModelRoot(): string {
  const injectedRoot = resolve(import.meta.dirname, "fixtures", "decision-models");
  const configuration = readDecisionModelStorageConfiguration(environmentFromDotenv(injectedRoot));
  if (configuration.kind !== "configured") {
    throw new Error("TEST_DECISION_MODEL_ROOT_INVALID");
  }
  return configuration.rootDirectory;
}

export function injectedOpenJevConfiguration(): LocalOpenJevConfiguration {
  return {
    modelPath: join(injectedDecisionModelRoot(), "kev-0.6b"),
    modelId: "kev-0.6b",
    modelRevision,
  };
}
