import type { LocalOpenJevConfiguration } from "@lyonbrown/decision-engine/internal/open-jev-configuration";
import { toOpenJevQuestion } from "@lyonbrown/decision-engine/internal/question-mapping";
import type { OpenJevQuestion } from "@lyonbrown/decision-engine/internal/open-jev-runtime";
import { isValidDecisionRequest } from "@lyonbrown/decision-engine/internal/request-validation";
import { mapOpenJevAnswers } from "@lyonbrown/decision-engine/internal/response-validation";
import type {
  DecisionEnginePort,
  DecisionOutcome,
  DecisionRequest,
} from "@lyonbrown/decision-engine/internal/types";

export interface OpenJevWorkerTask {
  readonly configuration: LocalOpenJevConfiguration;
  readonly state: string;
  readonly questions: readonly OpenJevQuestion[];
}

export interface DecisionWorkerPool {
  readonly run: (
    task: OpenJevWorkerTask,
    options: { readonly signal: AbortSignal },
  ) => Promise<unknown>;
  readonly close: () => Promise<void>;
}

export interface CreateWorkerOpenJevDecisionEngineOptions {
  readonly pool: DecisionWorkerPool;
  readonly configuration: LocalOpenJevConfiguration;
  readonly closeTimeoutMs?: number;
}

const undetermined = (
  reason: Extract<DecisionOutcome, { kind: "undetermined" }>["reason"],
): DecisionOutcome => ({ kind: "undetermined", reason });

const isAbortRequested = (signal: AbortSignal): boolean => signal.aborted;

function closeWithin(pool: DecisionWorkerPool, timeoutMs: number): Promise<void> {
  return new Promise((resolve) => {
    let completed = false;
    const complete = (): void => {
      if (completed) return;
      completed = true;
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(complete, timeoutMs);
    timer.unref();
    void pool.close().then(complete, complete);
  });
}

export function createWorkerOpenJevDecisionEngine(
  options: CreateWorkerOpenJevDecisionEngineOptions,
): DecisionEnginePort {
  let disposed = false;
  let disposePromise: Promise<void> | undefined;

  return {
    async decide(request: DecisionRequest): Promise<DecisionOutcome> {
      if (disposed) return undetermined("disposed");
      if (!isValidDecisionRequest(request)) return undetermined("invalid_request");
      if (isAbortRequested(request.signal)) return undetermined("aborted");
      try {
        const raw = await options.pool.run(
          {
            configuration: options.configuration,
            state: request.state,
            questions: request.questions.map(toOpenJevQuestion),
          },
          { signal: request.signal },
        );
        if (isAbortRequested(request.signal)) return undetermined("aborted");
        const answers = mapOpenJevAnswers(request.questions, raw);
        if (answers === undefined) return undetermined("malformed_response");
        return {
          kind: "decided",
          providerId: "open-jev",
          modelId: options.configuration.modelId,
          modelRevision: options.configuration.modelRevision,
          answers,
        };
      } catch {
        return undetermined(isAbortRequested(request.signal) ? "aborted" : "provider_unavailable");
      }
    },
    dispose(): Promise<void> {
      disposed = true;
      disposePromise ??= closeWithin(options.pool, options.closeTimeoutMs ?? 5_000);
      return disposePromise;
    },
  };
}
