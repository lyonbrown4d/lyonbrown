import { status } from "@grpc/grpc-js";
import { createAgentClient } from "@lyonbrown/agent-client";
import {
  DecisionModelPhase,
  ModelPurpose,
  ModelSelection,
  RuntimePhase,
  RuntimeSnapshot,
  StartModelDownloadRequest,
  type CommandResponse,
} from "@lyonbrown/protocol";
import { describe, expect, it } from "vitest";

import { initialDecisionModelStatus } from "@lyonbrown/agentd/internal/decision-model-state";
import { projectDecisionModel } from "@lyonbrown/agentd/internal/decision-model-projection";
import {
  FakeHost,
  authorization,
  invokeUnary,
  requestMetadata,
  SCOPE_FINGERPRINT,
  SERVER_INSTANCE_ID,
  startTestServer,
} from "@lyonbrown/agentd/test/server-test-support";

class Host extends FakeHost {
  override snapshot() {
    return RuntimeSnapshot.create({
      protocolVersion: 1,
      revision: 4n,
      phase: RuntimePhase.RUNTIME_PHASE_READY,
      modelSelection: ModelSelection.create({ selection: { $case: "auto", auto: true } }),
      decisionModel: projectDecisionModel({
        ...initialDecisionModelStatus("required"),
        allowedActions: ["download"],
      }),
    });
  }
}

describe("authenticated decision-model gRPC", () => {
  it("projects independent installation status and dispatches purpose through the public client", async () => {
    const server = await startTestServer(new Host());
    const client = createAgentClient({
      binding: {
        pid: process.pid,
        endpoint: server.address,
        token: server.token,
        scopeId: SCOPE_FINGERPRINT,
        serverInstanceId: SERVER_INSTANCE_ID,
      },
    });
    try {
      await client.handshake({ clientName: "decision-test", clientVersion: "0.0.0" });
      const snapshot = await client.getSnapshot();
      expect(snapshot.phase).toBe("ready");
      expect(snapshot.decisionModel).toMatchObject({
        phase: "required",
        modelId: "kev-0.6b",
        advisoryOnly: true,
      });
      await client.enqueue({
        type: "start-model-download",
        purpose: "decision",
        modelId: "kev-0.6b",
      });
      await client.enqueue({
        type: "cancel-model-download",
        purpose: "decision",
        operationId: "decision-op",
      });
      expect(server.host.execute.mock.calls.map(([command]) => command)).toEqual([
        { kind: "start-model-download", purpose: "decision", modelId: "kev-0.6b" },
        { kind: "cancel-model-download", purpose: "decision", operationId: "decision-op" },
      ]);
    } finally {
      await client.close();
      await server.close();
    }
  });

  it("includes purpose in replay identity and rejects unknown purpose before host mutation", async () => {
    const server = await startTestServer();
    const metadata = authorization(server.token);
    const send = (purpose: ModelPurpose) =>
      invokeUnary<CommandResponse>((callback) =>
        server.client.startModelDownload(
          StartModelDownloadRequest.create({
            metadata: requestMetadata("same-command"),
            modelId: "kev-0.6b",
            purpose,
          }),
          metadata,
          callback,
        ),
      );
    try {
      await send(ModelPurpose.MODEL_PURPOSE_DECISION);
      await send(ModelPurpose.MODEL_PURPOSE_DECISION);
      expect(server.host.execute).toHaveBeenCalledOnce();
      await expect(send(ModelPurpose.MODEL_PURPOSE_CHAT)).rejects.toMatchObject({
        code: status.ALREADY_EXISTS,
      });
      await expect(send(999 as ModelPurpose)).rejects.toMatchObject({
        code: status.INVALID_ARGUMENT,
      });
      expect(server.host.execute).toHaveBeenCalledOnce();
    } finally {
      await server.close();
    }
  });

  it("projects every installation phase without reporting lazy weights as ready", () => {
    expect(projectDecisionModel(undefined)).toBeUndefined();
    expect(projectDecisionModel(initialDecisionModelStatus("configured"))?.phase).toBe(
      DecisionModelPhase.DECISION_MODEL_PHASE_CONFIGURED,
    );
  });
});
