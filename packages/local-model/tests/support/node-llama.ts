import { vi } from "vitest";

export interface GenerationOptions {
  readonly maxTokens: number;
  readonly budgets: { readonly thoughtTokens: 0 };
  readonly onTextChunk: (text: string) => void;
  readonly signal: AbortSignal;
  readonly stopOnAbortSignal: true;
}

export interface GenerationResult {
  readonly metadata: {
    readonly stopReason: "abort" | "eogToken" | "functionCalls" | "maxTokens";
  };
  readonly functionCalls?: readonly unknown[];
}

interface ContextOptions {
  readonly contextSize: 4_096;
  readonly sequences: 1;
  readonly createSignal: AbortSignal;
}

export const createNodeLlamaHarness = (
  generateResponse: (
    history: readonly unknown[],
    options: GenerationOptions,
  ) => Promise<GenerationResult>,
) => {
  const disposeChat = vi.fn();
  const disposeSequence = vi.fn(() => Promise.resolve());
  const disposeContext = vi.fn(() => Promise.resolve());
  const disposeModel = vi.fn(() => Promise.resolve());
  const disposeLlama = vi.fn(() => Promise.resolve());
  const createContext = vi.fn((options?: ContextOptions) => {
    void options;
    return Promise.resolve({
      getSequence: () => ({
        tokenMeter: {
          getState: () => ({ usedInputTokens: 0, usedOutputTokens: 0 }),
          diff: () => ({ usedInputTokens: 3, usedOutputTokens: 2 }),
        },
        dispose: disposeSequence,
      }),
      dispose: disposeContext,
    });
  });
  const createChat = vi.fn(() => ({ generateResponse, dispose: disposeChat }));
  const getLlama = vi.fn(() =>
    Promise.resolve({
      loadModel: () => Promise.resolve({ createContext, dispose: disposeModel }),
      dispose: disposeLlama,
    }),
  );

  return {
    api: { getLlama, createChat },
    createChat,
    createContext,
    disposeChat,
    disposeContext,
    disposeLlama,
    disposeModel,
    disposeSequence,
  };
};
