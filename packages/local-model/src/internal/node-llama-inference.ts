import {
  LlamaChat,
  LlamaContextSequence,
  type ChatHistoryItem,
  type ChatModelFunctions,
} from "node-llama-cpp";

import type {
  LocalModelInferenceEvent,
  LocalModelInferenceRequest,
  LocalModelMessage,
} from "@lyonbrown/local-model/internal/types";
import {
  InferenceEventChannel,
  singleInferenceEventIterator,
} from "@lyonbrown/local-model/internal/internal/inference-channel";
import {
  utf8Prefix,
  validateInferenceRequest,
} from "@lyonbrown/local-model/internal/internal/inference-request";
import {
  normalizeNativeFunctionCalls,
  toNativeFunctions,
  toNativeHistory,
} from "@lyonbrown/local-model/internal/internal/node-llama-function-calls";

const LENGTH_ABORT = Object.freeze(new Error("local_model_output_limit"));
export const DISPOSE_ABORT = Object.freeze(new Error("local_model_disposed"));

interface TokenMeterState {
  readonly usedInputTokens: number;
  readonly usedOutputTokens: number;
}

export interface NativeSequence {
  readonly tokenMeter: {
    getState(): TokenMeterState;
    diff(state: TokenMeterState): TokenMeterState;
  };
  dispose(): Promise<void>;
}

interface NativeContext {
  getSequence(): NativeSequence;
  dispose(): Promise<void>;
}

interface NativeContextOptions {
  readonly contextSize: 4_096;
  readonly sequences: 1;
  readonly createSignal: AbortSignal;
}

export interface NativeModel {
  createContext?(options: NativeContextOptions): Promise<NativeContext>;
  dispose(): Promise<void>;
}

interface NativeGenerationOptions {
  readonly onTextChunk: (text: string) => void;
  readonly signal: AbortSignal;
  readonly stopOnAbortSignal: true;
  readonly maxTokens: number;
  readonly budgets: { readonly thoughtTokens: 0 };
  readonly functions?: ChatModelFunctions;
}

interface NativeGenerationResult {
  readonly metadata: { readonly stopReason: string };
  readonly functionCalls?: readonly unknown[];
}

export interface NativeChat {
  generateResponse(
    history: ChatHistoryItem[],
    options: NativeGenerationOptions,
  ): Promise<NativeGenerationResult>;
  dispose(): void;
}

export interface ActiveInferenceRun {
  readonly controller: AbortController;
  readonly completion: Promise<void>;
}

export const createDefaultNativeChat = (sequence: NativeSequence): NativeChat => {
  if (!(sequence instanceof LlamaContextSequence)) {
    throw new Error("native_context_invalid");
  }
  return new LlamaChat({ contextSequence: sequence, autoDisposeSequence: false });
};

const validUsage = (usage: TokenMeterState): boolean =>
  Number.isSafeInteger(usage.usedInputTokens) &&
  usage.usedInputTokens >= 0 &&
  Number.isSafeInteger(usage.usedOutputTokens) &&
  usage.usedOutputTokens >= 0;

const releaseStreamResources = async (
  chat: NativeChat | undefined,
  sequence: NativeSequence | undefined,
  context: NativeContext | undefined,
): Promise<boolean> => {
  let failed = false;
  try {
    chat?.dispose();
  } catch {
    failed = true;
  }
  try {
    await sequence?.dispose();
  } catch {
    failed = true;
  }
  try {
    await context?.dispose();
  } catch {
    failed = true;
  }
  return !failed;
};

const executeInference = async (
  model: NativeModel,
  messages: readonly LocalModelMessage[],
  request: LocalModelInferenceRequest,
  controller: AbortController,
  channel: InferenceEventChannel,
  createChat: (sequence: NativeSequence) => NativeChat,
): Promise<void> => {
  let context: NativeContext | undefined;
  let sequence: NativeSequence | undefined;
  let chat: NativeChat | undefined;
  let terminal: LocalModelInferenceEvent;
  let usage: TokenMeterState | undefined;
  let outputBytes = 0;

  try {
    if (model.createContext === undefined) throw new Error("native_context_unavailable");
    context = await model.createContext({
      contextSize: 4_096,
      sequences: 1,
      createSignal: controller.signal,
    });
    sequence = context.getSequence();
    const initialUsage = sequence.tokenMeter.getState();
    chat = createChat(sequence);
    const functions = toNativeFunctions(request.tools ?? []);
    const response = await chat.generateResponse(toNativeHistory(messages), {
      maxTokens: request.maxOutputTokens,
      budgets: { thoughtTokens: 0 },
      signal: controller.signal,
      stopOnAbortSignal: true,
      ...(functions === undefined ? {} : { functions }),
      onTextChunk(text): void {
        if (controller.signal.aborted) return;
        const accepted = utf8Prefix(text, request.maxOutputBytes - outputBytes);
        if (accepted !== "") {
          outputBytes += Buffer.byteLength(accepted, "utf8");
          channel.push({ type: "text_delta", text: accepted });
        }
        if (accepted !== text) {
          controller.abort(LENGTH_ABORT);
        }
      },
    });
    const functionCalls = normalizeNativeFunctionCalls(response.functionCalls, request.tools ?? []);
    if (response.metadata.stopReason === "functionCalls" && functionCalls.length === 0) {
      throw new Error("native_function_calls_missing");
    }
    for (const call of functionCalls) channel.push(call);
    usage = sequence.tokenMeter.diff(initialUsage);
    if (!validUsage(usage)) throw new Error("native_usage_invalid");
    terminal = {
      type: "done",
      reason:
        functionCalls.length > 0
          ? "tool_use"
          : controller.signal.reason === LENGTH_ABORT
            ? "length"
            : controller.signal.aborted
              ? "cancelled"
              : response.metadata.stopReason === "maxTokens"
                ? "length"
                : "stop",
    };
  } catch {
    terminal = controller.signal.aborted
      ? {
          type: "done",
          reason: controller.signal.reason === LENGTH_ABORT ? "length" : "cancelled",
        }
      : {
          type: "error",
          code: "native_inference_failed",
          message: "Local model inference failed.",
        };
  }

  const released = await releaseStreamResources(chat, sequence, context);
  if (!released) {
    terminal = {
      type: "error",
      code: "native_inference_failed",
      message: "Local model inference failed.",
    };
    usage = undefined;
  }
  if (usage !== undefined) {
    channel.push({
      type: "usage",
      inputTokens: usage.usedInputTokens,
      outputTokens: usage.usedOutputTokens,
    });
  }
  channel.push(terminal);
  channel.close();
};

export const createNodeLlamaInferenceStream = (
  model: NativeModel,
  request: LocalModelInferenceRequest,
  activeRuns: Set<ActiveInferenceRun>,
  isDisposed: () => boolean,
  createChat: (sequence: NativeSequence) => NativeChat,
): AsyncIterable<LocalModelInferenceEvent> => ({
  [Symbol.asyncIterator](): AsyncIterator<LocalModelInferenceEvent> {
    if (isDisposed()) {
      return singleInferenceEventIterator({
        type: "error",
        code: "disposed",
        message: "Local model is disposed.",
      });
    }
    const validation = validateInferenceRequest(request);
    if (!validation.valid) {
      return singleInferenceEventIterator({ type: "error", ...validation.failure });
    }
    if (request.signal.aborted) {
      return singleInferenceEventIterator({ type: "done", reason: "cancelled" });
    }
    if (activeRuns.size >= 1) {
      return singleInferenceEventIterator({
        type: "error",
        code: "resource_busy",
        message: "Local model inference is already active.",
      });
    }

    const channel = new InferenceEventChannel();
    const controller = new AbortController();
    const cancel = (): void => {
      controller.abort(request.signal.reason);
    };
    request.signal.addEventListener("abort", cancel, { once: true });
    const completion = executeInference(
      model,
      validation.messages,
      request,
      controller,
      channel,
      createChat,
    );
    const run: ActiveInferenceRun = { controller, completion };
    activeRuns.add(run);
    const releaseRun = (): void => {
      request.signal.removeEventListener("abort", cancel);
      activeRuns.delete(run);
    };
    void completion.then(releaseRun, releaseRun);

    let returned = false;
    return {
      async next(): Promise<IteratorResult<LocalModelInferenceEvent, undefined>> {
        if (returned) return { value: undefined, done: true };
        const next = await channel.next();
        if (next.done) returned = true;
        return next;
      },
      async return(): Promise<IteratorResult<LocalModelInferenceEvent, undefined>> {
        if (returned) return { value: undefined, done: true };
        returned = true;
        channel.cancel();
        if (!controller.signal.aborted) controller.abort(DISPOSE_ABORT);
        await completion;
        return { value: undefined, done: true };
      },
    };
  },
});
