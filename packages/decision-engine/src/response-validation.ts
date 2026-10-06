import type { DecisionAnswer, DecisionQuestion } from "@lyonbrown/decision-engine/internal/types";

const PROBABILITY_TOLERANCE = 0.01;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isProbability = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1;

function probabilitiesFor(
  candidate: unknown,
  labels: readonly string[],
): Readonly<Record<string, number>> | undefined {
  if (!isRecord(candidate)) return undefined;
  const keys = Object.keys(candidate);
  if (keys.length !== labels.length || !labels.every((label) => keys.includes(label))) {
    return undefined;
  }
  const probabilities: Record<string, number> = {};
  let total = 0;
  for (const label of labels) {
    const probability = candidate[label];
    if (!isProbability(probability)) return undefined;
    probabilities[label] = probability;
    total += probability;
  }
  return Math.abs(total - 1) <= PROBABILITY_TOLERANCE ? probabilities : undefined;
}

function mapChoice(question: Extract<DecisionQuestion, { type: "choice" }>, raw: unknown) {
  if (!isRecord(raw) || raw.type !== "choice" || typeof raw.choice !== "string") return undefined;
  const probabilities = probabilitiesFor(raw.probabilities, question.options);
  const selected = probabilities?.[raw.choice];
  if (
    probabilities === undefined ||
    !question.options.includes(raw.choice) ||
    !isProbability(raw.confidence) ||
    selected === undefined ||
    Math.abs(raw.confidence - selected) > PROBABILITY_TOLERANCE
  ) {
    return undefined;
  }
  return {
    id: question.id,
    type: "choice" as const,
    choice: raw.choice,
    confidence: raw.confidence,
    probabilities,
  };
}

function mapScore(question: Extract<DecisionQuestion, { type: "score" }>, raw: unknown) {
  if (!isRecord(raw) || raw.type !== "score" || typeof raw.level !== "string") return undefined;
  const probabilities = probabilitiesFor(raw.probabilities, question.levels);
  if (
    probabilities === undefined ||
    !question.levels.includes(raw.level) ||
    typeof raw.score !== "number" ||
    !Number.isFinite(raw.score) ||
    raw.score < 0 ||
    raw.score > question.levels.length - 1 ||
    !isProbability(raw.normalized) ||
    !isProbability(raw.confidence)
  ) {
    return undefined;
  }
  return {
    id: question.id,
    type: "score" as const,
    score: raw.score,
    normalized: raw.normalized,
    level: raw.level,
    confidence: raw.confidence,
    probabilities,
  };
}

function mapNoul(question: Extract<DecisionQuestion, { type: "noul" }>, raw: unknown) {
  if (
    !isRecord(raw) ||
    raw.type !== "noul" ||
    typeof raw.answer !== "boolean" ||
    !isProbability(raw.probability) ||
    !isProbability(raw.confidence)
  ) {
    return undefined;
  }
  return {
    id: question.id,
    type: "noul" as const,
    answer: raw.answer,
    probability: raw.probability,
    confidence: raw.confidence,
  };
}

export function mapOpenJevAnswers(
  questions: readonly DecisionQuestion[],
  raw: unknown,
): readonly DecisionAnswer[] | undefined {
  if (!Array.isArray(raw) || raw.length !== questions.length) return undefined;
  const values: readonly unknown[] = raw;
  const answers: DecisionAnswer[] = [];
  for (const [index, question] of questions.entries()) {
    const candidate = values[index];
    const answer =
      question.type === "choice"
        ? mapChoice(question, candidate)
        : question.type === "score"
          ? mapScore(question, candidate)
          : mapNoul(question, candidate);
    if (answer === undefined) return undefined;
    answers.push(answer);
  }
  return answers;
}
