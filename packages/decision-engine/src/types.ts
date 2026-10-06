export type DecisionQuestion =
  | {
      readonly id: string;
      readonly type: "choice";
      readonly instructions: string;
      readonly options: readonly string[];
    }
  | {
      readonly id: string;
      readonly type: "score";
      readonly instructions: string;
      readonly levels: readonly string[];
    }
  | {
      readonly id: string;
      readonly type: "noul";
      readonly instructions: string;
    };

export interface DecisionRequest {
  readonly state: string;
  readonly questions: readonly DecisionQuestion[];
  readonly signal: AbortSignal;
}

export type DecisionAnswer =
  | {
      readonly id: string;
      readonly type: "choice";
      readonly choice: string;
      readonly confidence: number;
      readonly probabilities: Readonly<Record<string, number>>;
    }
  | {
      readonly id: string;
      readonly type: "score";
      readonly score: number;
      readonly normalized: number;
      readonly level: string;
      readonly confidence: number;
      readonly probabilities: Readonly<Record<string, number>>;
    }
  | {
      readonly id: string;
      readonly type: "noul";
      readonly answer: boolean;
      readonly probability: number;
      readonly confidence: number;
    };

export type DecisionUndeterminedReason =
  | "aborted"
  | "disposed"
  | "invalid_request"
  | "malformed_response"
  | "provider_unavailable";

export type DecisionOutcome =
  | {
      readonly kind: "decided";
      readonly providerId: string;
      readonly modelId: string;
      readonly modelRevision: string;
      readonly answers: readonly DecisionAnswer[];
    }
  | {
      readonly kind: "undetermined";
      readonly reason: DecisionUndeterminedReason;
    };

export interface DecisionEnginePort {
  decide(request: DecisionRequest): Promise<DecisionOutcome>;
  dispose(): Promise<void>;
}
