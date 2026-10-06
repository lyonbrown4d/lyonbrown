import {
  status,
  type sendUnaryData,
  type ServerErrorResponse,
  type ServerUnaryCall,
} from "@grpc/grpc-js";
import type {
  CancelModelDownloadRequest,
  CancelTurnRequest,
  ForceStopRequest,
  GetSnapshotRequest,
  HandshakeRequest,
  HealthRequest,
  QueueFollowUpRequest,
  RespondApprovalRequest,
  SelectSessionModelRequest,
  StartModelDownloadRequest,
  StartTurnRequest,
  SteerCurrentTurnRequest,
} from "@lyonbrown/protocol";
import {
  CommandResponse,
  ApprovalDecision,
  HandshakeResponse,
  HealthResponse,
  HealthStatus,
  PROTOCOL_VERSION,
  UnsupportedCommand,
  type AgentServiceServer,
  type RequestMetadata,
  type RuntimeSnapshot,
} from "@lyonbrown/protocol";
import { commandFingerprint } from "@lyonbrown/agentd/internal/fingerprint";
import {
  ReplayCache,
  isReplayCapacityError,
  isReplayConflict,
} from "@lyonbrown/agentd/internal/replay-cache";
import { MAX_MESSAGE_INPUT_BYTES } from "@lyonbrown/agentd/internal/runtime-limits";
import type { RuntimeCommand, RuntimeHost } from "@lyonbrown/agentd/internal/runtime-host";
import {
  runtimeFailureStatus,
  statusAtBoundary,
  trustedStatus,
  unsupportedStatus,
} from "@lyonbrown/agentd/internal/status";
import { pumpSnapshotStream } from "@lyonbrown/agentd/internal/stream";
import {
  requireBoundedText,
  requireEpoch,
  requireIdentifier,
  requireMetadata,
  validateServerIdentity,
} from "@lyonbrown/agentd/internal/validation";

const DEFAULT_REPLAY_CAPACITY = 1_024;
const SERVER_VERSION = "0.0.0";

export interface CreateAgentServiceOptions {
  readonly host: RuntimeHost;
  readonly serverInstanceId: string;
  readonly scopeFingerprint: string;
  readonly replayCapacity?: number;
  readonly shutdownSignal?: AbortSignal;
}

function internalStatus(error: unknown): ServerErrorResponse {
  return statusAtBoundary(error);
}

function unary<Request, Response>(
  operation: (request: Request) => Response | Promise<Response>,
): (call: ServerUnaryCall<Request, Response>, callback: sendUnaryData<Response>) => void {
  return (call, callback) => {
    void Promise.resolve()
      .then(() => operation(call.request))
      .then(
        (response) => {
          callback(null, response);
        },
        (error: unknown) => {
          callback(internalStatus(error));
        },
      );
  };
}

function replayKey(metadata: RequestMetadata): string {
  return JSON.stringify([metadata.clientInstanceId, metadata.commandId]);
}

export function createAgentService(options: CreateAgentServiceOptions): AgentServiceServer {
  validateServerIdentity(options.serverInstanceId, options.scopeFingerprint);
  const replay = new ReplayCache<CommandResponse>(
    options.replayCapacity ?? DEFAULT_REPLAY_CAPACITY,
  );

  async function execute(
    metadataValue: RequestMetadata | undefined,
    command: RuntimeCommand,
    fingerprint: string,
  ): Promise<CommandResponse> {
    const metadata = requireMetadata(metadataValue);
    requireEpoch(metadata, options.serverInstanceId);
    try {
      return await replay.execute(replayKey(metadata), fingerprint, async () => {
        const result = await options.host.execute(command);
        if (!result.ok) throw runtimeFailureStatus(result.error);
        return CommandResponse.create({
          protocolVersion: PROTOCOL_VERSION,
          commandId: metadata.commandId,
          accepted: true,
        });
      });
    } catch (error) {
      if (isReplayConflict(error)) {
        throw trustedStatus(status.ALREADY_EXISTS, error.message);
      }
      if (isReplayCapacityError(error)) {
        throw trustedStatus(status.RESOURCE_EXHAUSTED, error.message);
      }
      throw error;
    }
  }

  function fence(metadataValue: RequestMetadata | undefined): RequestMetadata {
    const metadata = requireMetadata(metadataValue);
    requireEpoch(metadata, options.serverInstanceId);
    return metadata;
  }

  function unsupported(metadata: RequestMetadata | undefined, command: UnsupportedCommand): never {
    fence(metadata);
    throw unsupportedStatus(command);
  }

  return {
    health: unary((request: HealthRequest) => {
      requireMetadata(request.metadata);
      return HealthResponse.create({
        protocolVersion: PROTOCOL_VERSION,
        status: HealthStatus.HEALTH_STATUS_SERVING,
      });
    }),
    handshake: unary((request: HandshakeRequest) => {
      requireMetadata(request.metadata);
      requireBoundedText("client_name", request.clientName, 128);
      requireBoundedText("client_version", request.clientVersion, 64);
      return HandshakeResponse.create({
        protocolVersion: PROTOCOL_VERSION,
        serverVersion: SERVER_VERSION,
        serverInstanceId: options.serverInstanceId,
        capabilities: ["streaming", ...options.host.capabilities()],
        scopeFingerprint: options.scopeFingerprint,
      });
    }),
    startTurn: unary((request: StartTurnRequest) => {
      requireBoundedText("text", request.text, MAX_MESSAGE_INPUT_BYTES);
      return execute(
        request.metadata,
        { kind: "start-turn", text: request.text },
        commandFingerprint("start_turn", [request.text]),
      );
    }),
    steerCurrentTurn: unary((request: SteerCurrentTurnRequest) => {
      requireBoundedText("text", request.text, MAX_MESSAGE_INPUT_BYTES);
      return execute(
        request.metadata,
        { kind: "steer-current-turn", text: request.text },
        commandFingerprint("steer_current_turn", [request.text]),
      );
    }),
    queueFollowUp: unary((request: QueueFollowUpRequest) => {
      requireBoundedText("text", request.text, MAX_MESSAGE_INPUT_BYTES);
      return execute(
        request.metadata,
        { kind: "queue-follow-up", text: request.text },
        commandFingerprint("queue_follow_up", [request.text]),
      );
    }),
    cancelTurn: unary((request: CancelTurnRequest) =>
      execute(request.metadata, { kind: "cancel-turn" }, commandFingerprint("cancel_turn", [])),
    ),
    forceStop: unary((request: ForceStopRequest) =>
      unsupported(request.metadata, UnsupportedCommand.UNSUPPORTED_COMMAND_FORCE_STOP),
    ),
    respondApproval: unary((request: RespondApprovalRequest) => {
      requireIdentifier("request_id", request.requestId);
      const decision = runtimeApprovalDecision(request.decision);
      return execute(
        request.metadata,
        { kind: "respond-approval", requestId: request.requestId, decision },
        commandFingerprint("respond_approval", [request.requestId, decision]),
      );
    }),
    selectSessionModel: unary((request: SelectSessionModelRequest) => {
      const selection = runtimeModelSelection(request.selection);
      const fields =
        selection.kind === "auto" ? ["auto"] : ["pinned", selection.provider, selection.model];
      return execute(
        request.metadata,
        { kind: "select-session-model", selection },
        commandFingerprint("select_session_model", fields),
      );
    }),
    startModelDownload: unary((request: StartModelDownloadRequest) => {
      requireBoundedText("model_id", request.modelId, 128);
      return execute(
        request.metadata,
        { kind: "start-model-download", modelId: request.modelId },
        commandFingerprint("start_model_download", [request.modelId]),
      );
    }),
    cancelModelDownload: unary((request: CancelModelDownloadRequest) => {
      requireBoundedText("operation_id", request.operationId, 128);
      return execute(
        request.metadata,
        { kind: "cancel-model-download", operationId: request.operationId },
        commandFingerprint("cancel_model_download", [request.operationId]),
      );
    }),
    getSnapshot: unary(async (request: GetSnapshotRequest): Promise<RuntimeSnapshot> => {
      fence(request.metadata);
      return options.host.snapshot();
    }),
    subscribeEvents(call) {
      try {
        fence(call.request.metadata);
      } catch (error) {
        call.destroy(internalStatus(error));
        return;
      }
      const request = call.request;
      void pumpSnapshotStream(
        call,
        (signal) => options.host.subscribe(request.afterRevision, signal),
        options.shutdownSignal,
      ).catch((error: unknown) => call.destroy(internalStatus(error)));
    },
  };
}

function runtimeModelSelection(
  selection: SelectSessionModelRequest["selection"],
):
  | { readonly kind: "auto" }
  | { readonly kind: "pinned"; readonly provider: string; readonly model: string } {
  if (selection?.selection?.$case === "auto" && selection.selection.auto) return { kind: "auto" };
  if (selection?.selection?.$case === "pinned") {
    requireIdentifier("provider", selection.selection.pinned.provider);
    requireIdentifier("model", selection.selection.pinned.model);
    return {
      kind: "pinned",
      provider: selection.selection.pinned.provider,
      model: selection.selection.pinned.model,
    };
  }
  throw trustedStatus(status.INVALID_ARGUMENT, "model selection is invalid");
}

function runtimeApprovalDecision(
  decision: ApprovalDecision,
): "allow-once" | "allow-for-session" | "deny-once" | "deny-remaining" {
  switch (decision) {
    case ApprovalDecision.APPROVAL_DECISION_ALLOW_ONCE:
      return "allow-once";
    case ApprovalDecision.APPROVAL_DECISION_ALLOW_FOR_SESSION:
      return "allow-for-session";
    case ApprovalDecision.APPROVAL_DECISION_DENY_ONCE:
      return "deny-once";
    case ApprovalDecision.APPROVAL_DECISION_DENY_REMAINING:
      return "deny-remaining";
    case ApprovalDecision.APPROVAL_DECISION_UNSPECIFIED:
    case ApprovalDecision.UNRECOGNIZED:
      throw trustedStatus(status.INVALID_ARGUMENT, "approval decision is invalid");
    default:
      throw trustedStatus(status.INVALID_ARGUMENT, "approval decision is invalid");
  }
}
