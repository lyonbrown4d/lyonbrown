import type { OpenJevRuntime } from "@lyonbrown/decision-engine/internal/open-jev-runtime";
import { toOpenJevQuestion } from "@lyonbrown/decision-engine/internal/question-mapping";
import { isValidDecisionRequest } from "@lyonbrown/decision-engine/internal/request-validation";
import { mapOpenJevAnswers } from "@lyonbrown/decision-engine/internal/response-validation";
import type {
  DecisionEnginePort,
  DecisionOutcome,
  DecisionRequest,
} from "@lyonbrown/decision-engine/internal/types";

export interface CreateOpenJevDecisionEngineOptions {
  readonly runtime: OpenJevRuntime;
  readonly modelId: string;
  readonly modelRevision: string;
}

const undetermined = (
  reason: Extract<DecisionOutcome, { kind: "undetermined" }>["reason"],
): DecisionOutcome => ({
  kind: "undetermined",
  reason,
});

function raceAbort<T>(promise: Promise<T>, signal: AbortSignal): Promise<T | typeof aborted> {
  if (signal.aborted) return Promise.resolve(aborted);
  return new Promise((resolve, reject) => {
    const onAbort = (): void => {
      resolve(aborted);
    };
    signal.addEventListener("abort", onAbort, { once: true });
    void promise.then(
      (value) => {
        signal.removeEventListener("abort", onAbort);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener("abort", onAbort);
        reject(error instanceof Error ? error : new Error("OpenJev provider failed"));
      },
    );
  });
}

const aborted = Symbol("aborted");

export function createOpenJevDecisionEngine(
  options: CreateOpenJevDecisionEngineOptions,
): DecisionEnginePort {
  let disposed = false;
  let disposePromise: Promise<void> | undefined;

  return {
    async decide(request: DecisionRequest): Promise<DecisionOutcome> {
      if (disposed) return undetermined("disposed");
      if (!isValidDecisionRequest(request)) return undetermined("invalid_request");
      if (request.signal.aborted) return undetermined("aborted");

      const providerCall = options.runtime.decide(
        request.state,
        request.questions.map(toOpenJevQuestion),
        { truncation: "error" },
      );
      try {
        const raw = await raceAbort(providerCall, request.signal);
        if (raw === aborted) {
          void providerCall.catch(() => undefined);
          return undetermined("aborted");
        }
        const answers = mapOpenJevAnswers(request.questions, raw);
        if (answers === undefined) return undetermined("malformed_response");
        return {
          kind: "decided",
          providerId: "open-jev",
          modelId: options.modelId,
          modelRevision: options.modelRevision,
          answers,
        };
      } catch {
        return undetermined("provider_unavailable");
      }
    },
    dispose(): Promise<void> {
      disposed = true;
      disposePromise ??= options.runtime.dispose().catch(() => undefined);
      return disposePromise;
    },
  };
}
