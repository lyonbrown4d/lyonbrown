import { resolve } from "node:path";

import { BUNDLED_DECISION_MODEL } from "@lyonbrown/decision-engine/composition";
import { parse } from "dotenv";
import { describe, expect, it, vi } from "vitest";

import {
  createDecisionModelController,
  type DecisionModelControllerDependencies,
} from "@lyonbrown/agentd/internal/decision-model-controller";
import type { DecisionInstallProgress } from "@lyonbrown/agentd/internal/decision-install-process";

function fixture() {
  let progress: (value: DecisionInstallProgress) => void = () => undefined;
  let complete: () => void = () => undefined;
  let fail: (error: Error) => void = () => undefined;
  let signal: AbortSignal | undefined;
  const installation = new Promise<void>((res, rej) => {
    complete = res;
    fail = rej;
  });
  const inspect = vi.fn<DecisionModelControllerDependencies["inspect"]>(async () => ({
    kind: "missing",
  }));
  const install = vi.fn<DecisionModelControllerDependencies["install"]>(
    (_root, _id, abort, onProgress) => {
      signal = abort;
      progress = onProgress;
      return installation;
    },
  );
  const root = resolve(import.meta.dirname, "fixtures", "decision models");
  const environment = parse(`LYONBROWN_DECISION_MODEL_ROOT=${root.replaceAll("\\", "/")}`);
  const controller = createDecisionModelController(environment, { inspect, install });
  return {
    controller,
    inspect,
    install,
    root,
    complete,
    fail,
    progress: (value: DecisionInstallProgress) => progress(value),
    signal: () => signal,
  };
}

async function settled() {
  await new Promise<void>((res) => setImmediate(res));
}

describe("daemon decision-model lifecycle", () => {
  it("inspects in the background without downloading and exposes an optional install", async () => {
    const f = fixture();
    expect(f.controller.status().phase).toBe("checking");
    await settled();
    expect(f.inspect).toHaveBeenCalledWith(f.root, expect.any(AbortSignal));
    expect(f.controller.status()).toMatchObject({
      phase: "required",
      allowedActions: ["download"],
      advisoryOnly: true,
    });
    expect(f.install).not.toHaveBeenCalled();
    await f.controller.dispose();
  });

  it("rejects unconfigured storage without choosing a system-drive fallback", async () => {
    const f = fixture();
    const controller = createDecisionModelController(
      {},
      { inspect: f.inspect, install: f.install },
    );
    expect(controller.status()).toMatchObject({ phase: "unavailable", allowedActions: [] });
    expect(controller.start(BUNDLED_DECISION_MODEL.modelId)).toMatchObject({ ok: false });
    await controller.dispose();
    await f.controller.dispose();
  });

  it("fences model identity, concurrent starts, operation cancellation and late cancellation", async () => {
    const f = fixture();
    await settled();
    expect(f.controller.start("other-model")).toMatchObject({
      ok: false,
      error: { code: "INVALID_MODEL_ID" },
    });
    expect(f.install).not.toHaveBeenCalled();
    expect(f.controller.start(BUNDLED_DECISION_MODEL.modelId)).toEqual({ ok: true });
    const operationId = f.controller.status().operationId;
    expect(f.controller.start(BUNDLED_DECISION_MODEL.modelId)).toMatchObject({ ok: false });
    expect(f.controller.cancel("wrong-operation")).toMatchObject({
      ok: false,
      error: { code: "MODEL_DOWNLOAD_OPERATION_MISMATCH" },
    });
    expect(f.signal()?.aborted).toBe(false);
    f.progress({
      phase: "downloading",
      downloadedBytes: 10,
      totalBytes: f.controller.status().totalBytes,
    });
    expect(f.controller.status().downloadedBytes).toBe(10);
    f.progress({ phase: "installing" });
    expect(f.controller.cancel(operationId ?? "")).toMatchObject({
      ok: false,
      error: { code: "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE" },
    });
    f.complete();
    await settled();
    expect(f.controller.status()).toMatchObject({
      phase: "configured",
      allowedActions: [],
      operationId: null,
    });
    await f.controller.dispose();
  });

  it("cancels the owned install and offers retry only after cleanup finishes", async () => {
    const f = fixture();
    await settled();
    f.controller.start(BUNDLED_DECISION_MODEL.modelId);
    expect(f.controller.cancel(f.controller.status().operationId ?? "")).toEqual({ ok: true });
    expect(f.signal()?.aborted).toBe(true);
    expect(f.controller.start(BUNDLED_DECISION_MODEL.modelId)).toMatchObject({ ok: false });
    f.fail(new Error("download aborted"));
    await settled();
    expect(f.controller.status()).toMatchObject({ phase: "cancelled", allowedActions: ["retry"] });
    await f.controller.dispose();
  });

  it("sanitizes failure details and clears listeners during shutdown", async () => {
    const f = fixture();
    await settled();
    const listener = vi.fn();
    const unsubscribe = f.controller.subscribe(listener);
    f.controller.start(BUNDLED_DECISION_MODEL.modelId);
    f.fail(new Error("secret https://user:password@host/private-model-root"));
    await settled();
    expect(f.controller.status()).toMatchObject({
      phase: "failed",
      failure: { code: "decision_model_install_failed" },
      allowedActions: ["retry"],
    });
    expect(JSON.stringify(f.controller.status())).not.toContain("password");
    unsubscribe();
    await f.controller.dispose();
    expect(f.controller.start(BUNDLED_DECISION_MODEL.modelId)).toMatchObject({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
  });
});
