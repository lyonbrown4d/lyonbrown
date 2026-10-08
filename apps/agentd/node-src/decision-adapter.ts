import type {
  DecisionAnswer,
  DecisionEnginePort,
  DecisionOutcome,
  DecisionQuestion,
} from "@lyonbrown/decision-engine";
import type { RuntimeDecisionPort, RuntimePreKernelDecision } from "@lyonbrown/agent-runtime";

const QUESTION_VERSION = "system-one-v1";
const DECISION_DEADLINE_MS = 15_000;
const QUESTIONS: readonly DecisionQuestion[] = Object.freeze([
  Object.freeze({
    id: "intent",
    type: "choice",
    instructions: "Classify the user's primary intent.",
    options: Object.freeze(["chat", "code-change", "system-operation"]),
  }),
  Object.freeze({
    id: "risk",
    type: "score",
    instructions: "Estimate the operational risk of fulfilling the request.",
    levels: Object.freeze(["low", "medium", "high"]),
  }),
  Object.freeze({
    id: "requires_tools",
    type: "noul",
    instructions: "Does fulfilling this request require using tools?",
  }),
]);

type UnavailableReason = Extract<RuntimePreKernelDecision, { readonly kind: "fallback" }>["reason"];

export interface CreateSystemOneDecisionPortOptions {
  readonly engine?: DecisionEnginePort;
  readonly unavailableReason?: UnavailableReason;
  readonly deadlineMs?: number;
}

export function createSystemOneDecisionPort(
  options: CreateSystemOneDecisionPortOptions,
): RuntimeDecisionPort {
  return {
    async evaluate({ input, signal }): Promise<RuntimePreKernelDecision> {
      if (options.engine === undefined) {
        return fallback(options.unavailableReason ?? "model_missing");
      }
      const deadline = AbortSignal.timeout(options.deadlineMs ?? DECISION_DEADLINE_MS);
      const combined = AbortSignal.any([signal, deadline]);
      const outcome = await options.engine.decide({
        state: input,
        questions: QUESTIONS,
        signal: combined,
      });
      return mapOutcome(outcome, combined);
    },
  };
}

function mapOutcome(outcome: DecisionOutcome, signal: AbortSignal): RuntimePreKernelDecision {
  if (outcome.kind === "undetermined") {
    if (outcome.reason === "aborted" && signal.aborted) return fallback("aborted");
    if (outcome.reason === "invalid_request" || outcome.reason === "malformed_response") {
      return fallback("invalid_response");
    }
    return fallback("provider_unavailable");
  }
  const intent = answer(outcome.answers, "intent", "choice");
  const risk = answer(outcome.answers, "risk", "score");
  const tools = answer(outcome.answers, "requires_tools", "noul");
  if (
    intent === undefined ||
    risk === undefined ||
    tools === undefined ||
    !isIntent(intent.choice) ||
    !isRisk(risk.level)
  ) {
    return fallback("invalid_response");
  }
  return {
    kind: "decided",
    questionVersion: QUESTION_VERSION,
    providerId: outcome.providerId,
    modelId: outcome.modelId,
    modelRevision: outcome.modelRevision,
    intent: intent.choice,
    risk: risk.level,
    requiresTools: tools.answer,
    confidence: Math.min(intent.confidence, risk.confidence, tools.confidence),
    probabilities: {
      intent: intent.probabilities,
      risk: risk.probabilities,
      requiresTools: {
        false: roundedComplement(tools.probability),
        true: tools.probability,
      },
    },
    action: "continue_pi",
  };
}

function answer<Type extends DecisionAnswer["type"]>(
  answers: readonly DecisionAnswer[],
  id: string,
  type: Type,
): Extract<DecisionAnswer, { readonly type: Type }> | undefined {
  const value = answers.find((candidate) => candidate.id === id);
  return value?.type === type
    ? (value as Extract<DecisionAnswer, { readonly type: Type }>)
    : undefined;
}

function fallback(reason: UnavailableReason): RuntimePreKernelDecision {
  return { kind: "fallback", questionVersion: QUESTION_VERSION, reason, action: "continue_pi" };
}

function isIntent(value: string): value is "chat" | "code-change" | "system-operation" {
  return value === "chat" || value === "code-change" || value === "system-operation";
}

function isRisk(value: string): value is "low" | "medium" | "high" {
  return value === "low" || value === "medium" || value === "high";
}

function roundedComplement(value: number): number {
  return Number((1 - value).toFixed(12));
}
