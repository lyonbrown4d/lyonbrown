import { descriptorFor } from "../public-model.ts";
import type { LocalModelLoadResult } from "../types.ts";
import type { LlamaEngineAdapter, ModelPreset } from "./contracts.ts";

export const loadSelectedModel = async (
  engine: LlamaEngineAdapter,
  preset: ModelPreset,
  signal: AbortSignal,
  isDisposed: () => boolean,
): Promise<LocalModelLoadResult> => {
  try {
    const lease =
      engine.load === undefined
        ? await engine.loadDefault(signal)
        : await engine.load(preset, signal);
    if (isDisposed()) {
      try {
        await lease.dispose();
      } catch {
        // A late native lease remains private; disposal failures are redacted.
      }
      return { kind: "unavailable", code: "service_disposed" };
    }
    return { kind: "ready", model: descriptorFor(preset), lease };
  } catch {
    return {
      kind: "unavailable",
      code: isDisposed() || signal.aborted ? "service_disposed" : "native_runtime_unavailable",
    };
  }
};
