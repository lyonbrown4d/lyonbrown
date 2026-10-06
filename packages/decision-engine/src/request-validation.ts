import type { DecisionQuestion, DecisionRequest } from "@lyonbrown/decision-engine/internal/types";

const MAX_STATE_BYTES = 64 * 1024;
const MAX_QUESTIONS = 16;
const MAX_INSTRUCTION_BYTES = 4 * 1024;
const MAX_OPTIONS = 32;
const MAX_OPTION_BYTES = 128;
const IDENTIFIER = /^[a-z][a-z0-9_-]{0,63}$/u;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const byteLengthWithin = (value: string, maximum: number): boolean =>
  value.length > 0 && Buffer.byteLength(value, "utf8") <= maximum;

function hasValidOptions(options: readonly string[]): boolean {
  return (
    options.length >= 2 &&
    options.length <= MAX_OPTIONS &&
    options.every((option) => byteLengthWithin(option, MAX_OPTION_BYTES)) &&
    new Set(options).size === options.length
  );
}

function stringArray(value: unknown): readonly string[] | undefined {
  return Array.isArray(value) && value.every((entry): entry is string => typeof entry === "string")
    ? value
    : undefined;
}

function validQuestion(candidate: unknown): candidate is DecisionQuestion {
  if (
    !isRecord(candidate) ||
    typeof candidate.id !== "string" ||
    typeof candidate.instructions !== "string" ||
    !IDENTIFIER.test(candidate.id) ||
    !byteLengthWithin(candidate.instructions, MAX_INSTRUCTION_BYTES)
  ) {
    return false;
  }
  if (candidate.type === "choice") {
    const options = stringArray(candidate.options);
    return options !== undefined && hasValidOptions(options);
  }
  if (candidate.type === "score") {
    const levels = stringArray(candidate.levels);
    return levels !== undefined && hasValidOptions(levels);
  }
  return candidate.type === "noul";
}

export function isValidDecisionRequest(request: DecisionRequest): boolean {
  const candidate: unknown = request;
  if (!isRecord(candidate)) return false;
  const { state, questions, signal } = candidate;
  if (
    typeof state !== "string" ||
    Buffer.byteLength(state, "utf8") > MAX_STATE_BYTES ||
    !Array.isArray(questions) ||
    questions.length === 0 ||
    questions.length > MAX_QUESTIONS ||
    !isRecord(signal) ||
    typeof signal.aborted !== "boolean" ||
    typeof signal.addEventListener !== "function" ||
    typeof signal.removeEventListener !== "function"
  ) {
    return false;
  }

  const ids = new Set<string>();
  for (const question of questions as readonly unknown[]) {
    if (!validQuestion(question) || ids.has(question.id)) return false;
    ids.add(question.id);
  }
  return true;
}
