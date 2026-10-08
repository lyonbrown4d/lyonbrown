import { bounded, errorFromUnknown } from "@lyonbrown/workspace/internal/smoke-real-gguf-lib";
import {
  isReady,
  requireManagedModel,
  requireSessionId,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-assertions";
import { prepareRealGgufModel } from "@lyonbrown/workspace/internal/smoke-real-gguf-setup";
import { waitSmokeSnapshot } from "@lyonbrown/workspace/internal/smoke-real-gguf-wait";

export interface SmokeSnapshot {
  readonly revision: bigint;
  readonly phase: string;
  readonly sessionId: string | null;
  readonly pendingApproval: {
    readonly requestId: string;
    readonly toolName: string;
    readonly argumentsJson: string;
  } | null;
  readonly modelSelection:
    | { readonly kind: "auto" }
    | { readonly kind: "pinned"; readonly provider: string; readonly model: string };
  readonly modelSetup: {
    readonly phase: string;
    readonly operationId: string | null;
    readonly downloadedBytes: bigint;
    readonly totalBytes: bigint | null;
    readonly managedModel: {
      readonly id: string;
      readonly maxBytes: bigint;
      readonly sha256: string;
    } | null;
    readonly allowedActions: readonly string[];
    readonly failure: { readonly code: string } | null;
  } | null;
  readonly turns: readonly {
    readonly userText: string;
    readonly status: string;
    readonly assistantText: string;
    readonly activities: readonly {
      readonly phase: string;
      readonly usage: { readonly input: number; readonly output: number } | null;
      readonly tools: readonly {
        readonly name: string;
        readonly argumentsJson: string;
        readonly state: string;
        readonly result: { readonly ok: boolean; readonly error: string | null } | null;
      }[];
    }[];
  }[];
}

export interface RealGgufSmokePort {
  readSnapshot(): Promise<SmokeSnapshot>;
  startDownload(modelId: string): Promise<void>;
  cancelDownload(operationId: string): Promise<void>;
  startTurn(text: string): Promise<void>;
  cancelTurn(): Promise<void>;
  respondApproval(requestId: string): Promise<void>;
  selectModel(selection: SmokeSnapshot["modelSelection"]): Promise<void>;
  verifyWorkspaceFile(path: string, expectedContent: string): Promise<void>;
  closeServer(): Promise<void>;
  restartServer(): Promise<void>;
  verifyInstalledCache(expectedSha256: string): Promise<void>;
  cleanup(): Promise<void>;
}

export interface RealGgufSmokeFlowOptions {
  readonly phaseTimeoutMs: number;
  readonly downloadTimeoutMs: number;
  readonly pollIntervalMs: number;
  readonly cancelAfterBytes: bigint;
  readonly closeTimeoutMs: number;
  readonly trustedModelDigests: Readonly<Record<string, string>>;
}

const SHORT_PROMPT = "Respond with exactly one short sentence confirming the smoke test.";
const TOOL_PROOF_FILE = "lyonbrown-tool-proof.txt";
const TOOL_PROMPT =
  `Call the write tool exactly once with path "${TOOL_PROOF_FILE}" and content ` +
  '"LYONBROWN_TOOL_OK". Use a relative path. Do not answer until the tool succeeds.';
const LONG_PROMPT =
  "Write a detailed numbered explanation with at least 500 items. Continue until cancelled.";

export async function runRealGgufSmokeFlow(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<void> {
  let primaryFailure: unknown;
  try {
    const { modelId, expectedSha256, ready } = await prepareRealGgufModel(port, options);

    await verifyModelSelection(port, options, modelId);
    await completeShortTurn(port, options);
    await completeApprovedWorkspaceWrite(port, options);
    await cancelStreamingTurn(port, options);
    const completedBeforeRestart = await completeShortTurn(port, options);
    const previousSessionId = requireSessionId(completedBeforeRestart);
    if (requireManagedModel(ready).sha256 !== expectedSha256) {
      throw new Error("REAL_GGUF_READY_DIGEST_MISMATCH");
    }
    await bounded("first-close", options.closeTimeoutMs, port.closeServer());
    await port.verifyInstalledCache(expectedSha256);

    await port.restartServer();
    await waitSmokeSnapshot(port, options, "cached-restart-ready", (snapshot) => {
      return (
        isReady(snapshot) &&
        snapshot.turns.length === 0 &&
        snapshot.sessionId !== null &&
        snapshot.sessionId !== previousSessionId
      );
    });
    await completeShortTurn(port, options);
    await startStreamingTurn(port, options);
    await bounded("active-inference-close", options.closeTimeoutMs, port.closeServer());
  } catch (error: unknown) {
    primaryFailure = error;
  }

  try {
    await port.cleanup();
  } catch (cleanupFailure: unknown) {
    if (primaryFailure !== undefined) {
      throw new AggregateError(
        [primaryFailure, cleanupFailure],
        "real GGUF smoke and cleanup failed",
        { cause: primaryFailure },
      );
    }
    throw errorFromUnknown(cleanupFailure);
  }
  if (primaryFailure !== undefined) throw errorFromUnknown(primaryFailure);
}

async function verifyModelSelection(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
  modelId: string,
): Promise<void> {
  await port.selectModel({ kind: "pinned", provider: "local", model: modelId });
  await waitSmokeSnapshot(port, options, "model-pinned", (snapshot) => {
    const selection = snapshot.modelSelection;
    return (
      selection.kind === "pinned" && selection.provider === "local" && selection.model === modelId
    );
  });
  await port.selectModel({ kind: "auto" });
  await waitSmokeSnapshot(
    port,
    options,
    "model-auto",
    (snapshot) => snapshot.modelSelection.kind === "auto",
  );
}

async function completeApprovedWorkspaceWrite(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<void> {
  const baseline = (await port.readSnapshot()).turns.length;
  await port.startTurn(TOOL_PROMPT);
  const pending = await waitSmokeSnapshot(port, options, "tool-approval", (snapshot) => {
    return snapshot.phase === "awaiting-approval" && snapshot.pendingApproval?.toolName === "write";
  });
  const approval = pending.pendingApproval;
  if (approval === null) throw new Error("REAL_GGUF_APPROVAL_REQUEST_MISSING");
  const requestId = approval.requestId;
  assertExpectedWriteArguments(approval.argumentsJson);
  await port.respondApproval(requestId);
  await waitSmokeSnapshot(port, options, "approved-tool-complete", (snapshot) => {
    const turn = snapshot.turns.at(-1);
    return (
      snapshot.phase === "completed" &&
      snapshot.pendingApproval === null &&
      snapshot.turns.length > baseline &&
      turn?.userText === TOOL_PROMPT &&
      turn.status === "completed" &&
      hasCompletedWriteTool(turn.activities)
    );
  });
  await port.verifyWorkspaceFile(TOOL_PROOF_FILE, "LYONBROWN_TOOL_OK");
}

function assertExpectedWriteArguments(argumentsJson: string): void {
  let value: unknown;
  try {
    value = JSON.parse(argumentsJson);
  } catch {
    throw new Error("REAL_GGUF_WORKSPACE_TOOL_ARGUMENTS_INVALID");
  }
  if (
    typeof value !== "object" ||
    value === null ||
    Array.isArray(value) ||
    !("path" in value) ||
    !("content" in value) ||
    value.path !== TOOL_PROOF_FILE ||
    value.content !== "LYONBROWN_TOOL_OK"
  ) {
    throw new Error(`REAL_GGUF_WORKSPACE_TOOL_ARGUMENTS_UNEXPECTED:${argumentsJson}`);
  }
}

function hasCompletedWriteTool(activities: SmokeSnapshot["turns"][number]["activities"]): boolean {
  return activities.some((activity) =>
    activity.tools.some(
      (tool) =>
        tool.name === "write" &&
        tool.state === "completed" &&
        tool.result?.ok === true &&
        tool.result.error === null,
    ),
  );
}

async function completeShortTurn(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<SmokeSnapshot> {
  const baseline = (await port.readSnapshot()).turns.length;
  await port.startTurn(SHORT_PROMPT);
  const snapshot = await waitSmokeSnapshot(port, options, "short-turn", (value) => {
    const turn = value.turns.at(-1);
    return (
      value.phase === "completed" &&
      value.turns.length > baseline &&
      turn?.userText === SHORT_PROMPT &&
      turn.status === "completed"
    );
  });
  const turn = snapshot.turns.at(-1);
  const activity = turn?.activities.at(-1);
  const usage = activity?.usage;
  if (
    turn === undefined ||
    turn.assistantText.trim().length === 0 ||
    turn.userText !== SHORT_PROMPT ||
    activity?.phase !== "completed" ||
    usage === null ||
    usage === undefined ||
    usage.input <= 0 ||
    usage.output < 0
  ) {
    throw new Error("REAL_GGUF_SHORT_TURN_INVALID");
  }
  return snapshot;
}

async function cancelStreamingTurn(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<void> {
  const baseline = await startStreamingTurn(port, options);
  await port.cancelTurn();
  await waitSmokeSnapshot(port, options, "turn-cancelled", (snapshot) => {
    const turn = snapshot.turns.at(-1);
    return (
      snapshot.phase === "ready" &&
      snapshot.turns.length > baseline &&
      turn?.userText === LONG_PROMPT &&
      turn.status === "cancelled"
    );
  });
}

async function startStreamingTurn(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<number> {
  const baseline = (await port.readSnapshot()).turns.length;
  await port.startTurn(LONG_PROMPT);
  await waitSmokeSnapshot(port, options, "turn-streaming", (snapshot) => {
    const turn = snapshot.turns.at(-1);
    return (
      (snapshot.phase === "preparing" || snapshot.phase === "streaming") &&
      snapshot.turns.length > baseline &&
      turn?.userText === LONG_PROMPT &&
      turn.status === "running" &&
      turn.activities.length > 0
    );
  });
  return baseline;
}
