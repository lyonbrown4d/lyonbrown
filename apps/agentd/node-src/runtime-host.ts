import type { RuntimeEvent, RuntimeSnapshot } from "@lyonbrown/protocol";

export type RuntimeCommand =
  | { readonly kind: "start-turn"; readonly text: string }
  | { readonly kind: "steer-current-turn"; readonly text: string }
  | { readonly kind: "queue-follow-up"; readonly text: string }
  | { readonly kind: "cancel-turn" }
  | { readonly kind: "start-model-download"; readonly modelId: string }
  | { readonly kind: "cancel-model-download"; readonly operationId: string };

export type RuntimeFailure =
  | { readonly code: "RUNTIME_STOPPED" }
  | { readonly code: "TURN_ALREADY_ACTIVE" }
  | { readonly code: "NO_ACTIVE_TURN" }
  | { readonly code: "EMPTY_TEXT" }
  | { readonly code: "STEERING"; readonly reason: string }
  | { readonly code: "MODEL_SETUP_REQUIRED" }
  | { readonly code: "MODEL_DOWNLOAD_UNAVAILABLE" }
  | {
      readonly code: "INVALID_MODEL_ID";
      readonly requestedModelId: string;
      readonly configuredModelId: string;
    }
  | { readonly code: "NO_ACTIVE_MODEL_DOWNLOAD" }
  | {
      readonly code: "MODEL_DOWNLOAD_OPERATION_MISMATCH";
      readonly expectedOperationId: string;
      readonly receivedOperationId: string;
    }
  | { readonly code: "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE" }
  | { readonly code: "RESOURCE_EXHAUSTED" };

export type RuntimeResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly error: RuntimeFailure };

/** Pi and other execution-engine types stop at this daemon-owned port. */
export interface RuntimeHost {
  execute(command: RuntimeCommand): Promise<RuntimeResult>;
  snapshot(): RuntimeSnapshot | Promise<RuntimeSnapshot>;
  subscribe(afterRevision: bigint, signal: AbortSignal): AsyncIterable<RuntimeEvent>;
  capabilities(): readonly string[];
}
