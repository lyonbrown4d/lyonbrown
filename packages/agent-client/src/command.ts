import type { Metadata, ServiceError } from "@grpc/grpc-js";
import {
  ApprovalDecision as WireApprovalDecision,
  CancelModelDownloadRequest,
  CancelTurnRequest,
  ForceStopRequest,
  ModelRoute,
  ModelSelection,
  QueueFollowUpRequest,
  RespondApprovalRequest,
  SelectSessionModelRequest,
  StartModelDownloadRequest,
  StartTurnRequest,
  SteerCurrentTurnRequest,
  type AgentServiceClient as WireClient,
  type CommandResponse,
  type RequestMetadata,
} from "@lyonbrown/protocol";

import type { AgentCommand, ApprovalDecision, ModelSelectionDto } from "./types.ts";
import { requireId, requireText } from "./validation.ts";

export interface WireCommand {
  readonly invoke: (
    client: WireClient,
    metadata: Metadata,
    deadline: Date,
    callback: (error: ServiceError | null, response: CommandResponse) => void,
  ) => void;
}

export function mapCommand(command: AgentCommand, requestMetadata: RequestMetadata): WireCommand {
  switch (command.type) {
    case "start-turn": {
      const request = StartTurnRequest.fromPartial({
        metadata: requestMetadata,
        text: requireText("text", command.text),
      });
      return invocation((client, metadata, deadline, callback) =>
        client.startTurn(request, metadata, { deadline }, callback),
      );
    }
    case "steer-current-turn": {
      const request = SteerCurrentTurnRequest.fromPartial({
        metadata: requestMetadata,
        text: requireText("text", command.text),
      });
      return invocation((client, metadata, deadline, callback) =>
        client.steerCurrentTurn(request, metadata, { deadline }, callback),
      );
    }
    case "queue-follow-up": {
      const request = QueueFollowUpRequest.fromPartial({
        metadata: requestMetadata,
        text: requireText("text", command.text),
      });
      return invocation((client, metadata, deadline, callback) =>
        client.queueFollowUp(request, metadata, { deadline }, callback),
      );
    }
    case "cancel-turn": {
      const request = CancelTurnRequest.fromPartial({ metadata: requestMetadata });
      return invocation((client, metadata, deadline, callback) =>
        client.cancelTurn(request, metadata, { deadline }, callback),
      );
    }
    case "force-stop": {
      const request = ForceStopRequest.fromPartial({ metadata: requestMetadata });
      return invocation((client, metadata, deadline, callback) =>
        client.forceStop(request, metadata, { deadline }, callback),
      );
    }
    case "respond-approval": {
      const request = RespondApprovalRequest.fromPartial({
        metadata: requestMetadata,
        requestId: requireId("requestId", command.requestId),
        decision: decision(command.decision),
      });
      return invocation((client, metadata, deadline, callback) =>
        client.respondApproval(request, metadata, { deadline }, callback),
      );
    }
    case "select-session-model": {
      const request = SelectSessionModelRequest.fromPartial({
        metadata: requestMetadata,
        selection: selection(command.selection),
      });
      return invocation((client, metadata, deadline, callback) =>
        client.selectSessionModel(request, metadata, { deadline }, callback),
      );
    }
    case "start-model-download": {
      const request = StartModelDownloadRequest.fromPartial({
        metadata: requestMetadata,
        modelId: requireId("modelId", command.modelId),
      });
      return invocation((client, metadata, deadline, callback) =>
        client.startModelDownload(request, metadata, { deadline }, callback),
      );
    }
    case "cancel-model-download": {
      const request = CancelModelDownloadRequest.fromPartial({
        metadata: requestMetadata,
        operationId: requireId("operationId", command.operationId),
      });
      return invocation((client, metadata, deadline, callback) =>
        client.cancelModelDownload(request, metadata, { deadline }, callback),
      );
    }
  }
}

function invocation(invoke: WireCommand["invoke"]): WireCommand {
  return { invoke };
}

function decision(value: ApprovalDecision): WireApprovalDecision {
  switch (value) {
    case "allow-once":
      return WireApprovalDecision.APPROVAL_DECISION_ALLOW_ONCE;
    case "allow-for-session":
      return WireApprovalDecision.APPROVAL_DECISION_ALLOW_FOR_SESSION;
    case "deny-once":
      return WireApprovalDecision.APPROVAL_DECISION_DENY_ONCE;
    case "deny-remaining":
      return WireApprovalDecision.APPROVAL_DECISION_DENY_REMAINING;
  }
}

function selection(value: ModelSelectionDto): ModelSelection {
  if (value.kind === "auto") {
    return ModelSelection.fromPartial({ selection: { $case: "auto", auto: true } });
  }
  return ModelSelection.fromPartial({
    selection: {
      $case: "pinned",
      pinned: ModelRoute.fromPartial({
        provider: requireId("selection.provider", value.provider),
        model: requireId("selection.model", value.model),
      }),
    },
  });
}
