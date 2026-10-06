import type { OpenJevQuestion } from "@lyonbrown/decision-engine/internal/open-jev-runtime";
import type { DecisionQuestion } from "@lyonbrown/decision-engine/internal/types";

export function toOpenJevQuestion(question: DecisionQuestion): OpenJevQuestion {
  if (question.type === "choice") {
    return { type: "choice", instructions: question.instructions, options: [...question.options] };
  }
  if (question.type === "score") {
    return { type: "score", instructions: question.instructions, options: [...question.levels] };
  }
  return { type: "noul", instructions: question.instructions };
}
