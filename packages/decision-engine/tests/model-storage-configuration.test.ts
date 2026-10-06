import { resolve } from "node:path";

import { parse, populate } from "dotenv";
import { describe, expect, it } from "vitest";

import {
  DECISION_MODEL_ROOT_ENV,
  readDecisionModelStorageConfiguration,
} from "@lyonbrown/decision-engine/internal/model-storage-configuration";
import { environmentFromDotenv } from "@lyonbrown/decision-engine/test/model-environment";

const injectedRoot = resolve(import.meta.dirname, "fixtures", "decision-models");

describe("decision model storage configuration", () => {
  it("reads an absolute model root injected through dotenv", () => {
    expect(readDecisionModelStorageConfiguration(environmentFromDotenv(injectedRoot))).toEqual({
      kind: "configured",
      rootDirectory: injectedRoot,
    });
  });

  it("preserves dotenv paths containing spaces and comment markers", () => {
    const modelRoot = resolve(import.meta.dirname, "fixtures", "decision models # local");

    expect(readDecisionModelStorageConfiguration(environmentFromDotenv(modelRoot))).toEqual({
      kind: "configured",
      rootDirectory: modelRoot,
    });
  });

  it("reports a missing model root without falling back to a system drive", () => {
    expect(readDecisionModelStorageConfiguration({})).toEqual({
      kind: "unavailable",
      reason: "missing_model_root",
    });
  });

  it.each(["relative/models", " ", `invalid\0path`])(
    "rejects invalid model root %j",
    (rootDirectory) => {
      expect(readDecisionModelStorageConfiguration(environmentFromDotenv(rootDirectory))).toEqual({
        kind: "unavailable",
        reason: "invalid_model_root",
      });
    },
  );

  it("preserves an explicit process value when dotenv is populated", () => {
    const processRoot = resolve(import.meta.dirname, "process-models");
    const environment: NodeJS.ProcessEnv = {
      [DECISION_MODEL_ROOT_ENV]: processRoot,
    };
    populate(
      environment,
      parse(`${DECISION_MODEL_ROOT_ENV}=${injectedRoot.replaceAll("\\", "/")}`),
      { override: false },
    );

    expect(readDecisionModelStorageConfiguration(environment)).toEqual({
      kind: "configured",
      rootDirectory: processRoot,
    });
  });
});
