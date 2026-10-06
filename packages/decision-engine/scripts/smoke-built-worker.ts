import { pathToFileURL } from "node:url";
import { join, resolve } from "node:path";

import { parse, populate } from "dotenv";

import type { CreateDecisionEngineResult } from "@lyonbrown/decision-engine/internal/composition";
import {
  DECISION_MODEL_ROOT_ENV,
  type DecisionModelStorageConfiguration,
} from "@lyonbrown/decision-engine/internal/model-storage-configuration";
import type { LocalOpenJevConfiguration } from "@lyonbrown/decision-engine/internal/open-jev-configuration";

interface BuiltComposition {
  readonly createLocalOpenJevDecisionEngine: (
    configuration: LocalOpenJevConfiguration,
  ) => CreateDecisionEngineResult;
  readonly readDecisionModelStorageConfiguration: (
    environment: Readonly<NodeJS.ProcessEnv>,
  ) => DecisionModelStorageConfiguration;
}

function isBuiltComposition(value: unknown): value is BuiltComposition {
  if (typeof value !== "object" || value === null) return false;
  return (
    "createLocalOpenJevDecisionEngine" in value &&
    typeof value.createLocalOpenJevDecisionEngine === "function" &&
    "readDecisionModelStorageConfiguration" in value &&
    typeof value.readDecisionModelStorageConfiguration === "function"
  );
}

const entryUrl = pathToFileURL(resolve(import.meta.dirname, "../dist/composition-entry.js")).href;
const imported: unknown = await import(entryUrl);
if (!isBuiltComposition(imported)) throw new Error("DECISION_ENGINE_BUILD_EXPORTS_INVALID");

const environment: NodeJS.ProcessEnv = {};
const dynamicRoot = resolve(import.meta.dirname, "fixtures", "decision-models").replaceAll(
  "\\",
  "/",
);
populate(environment, parse(`${DECISION_MODEL_ROOT_ENV}=${JSON.stringify(dynamicRoot)}`), {
  override: false,
});
const storage = imported.readDecisionModelStorageConfiguration(environment);
if (storage.kind !== "configured") throw new Error("DECISION_MODEL_ROOT_NOT_CONFIGURED");

const result = imported.createLocalOpenJevDecisionEngine({
  modelPath: join(storage.rootDirectory, "missing-kev-0.6b"),
  modelId: "kev-0.6b",
  modelRevision: "0123456789abcdef0123456789abcdef01234567",
});
if (result.kind !== "configured") throw new Error("DECISION_WORKER_NOT_CONFIGURED");

const outcome = await result.engine.decide({
  state: "built worker smoke",
  questions: [
    {
      id: "route",
      type: "choice",
      instructions: "Select a route.",
      options: ["chat", "agent"],
    },
  ],
  signal: new AbortController().signal,
});
await result.engine.dispose();
if (outcome.kind !== "undetermined" || outcome.reason !== "provider_unavailable") {
  throw new Error("DECISION_WORKER_SMOKE_OUTCOME_INVALID");
}
process.stdout.write("Built decision worker smoke passed.\n");
