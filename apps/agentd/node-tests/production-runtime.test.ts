import { mkdtemp, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { EventStore, PathSecurityVerifier, SessionMetadata } from "@lyonbrown/event-store";
import type { LocalModelService } from "@lyonbrown/local-model";
import type { NativeModelStorageCapability } from "@lyonbrown/local-model/composition";
import { describe, expect, it, vi } from "vitest";

import type { ModelAwareRuntime } from "@lyonbrown/agentd/internal/model-aware-runtime";
import {
  createProductionAgentdRuntime,
  type ProductionRuntimeDependencies,
} from "@lyonbrown/agentd/internal/production-runtime";

const CONTEXT = {
  canonicalWorkspace: "C:\\workspace",
  dataDirectory: "C:\\agent-data",
  scopeId: "a".repeat(64),
};

function dependencies() {
  const storeClose = vi.fn();
  const storeListSessions = vi.fn(async (): Promise<readonly SessionMetadata[]> => []);
  const store = {
    close: storeClose,
    listSessions: storeListSessions,
  } as unknown as EventStore;
  const verifier = {} as PathSecurityVerifier;
  const storage = {} as NativeModelStorageCapability;
  const modelDispose = vi.fn(() => Promise.resolve());
  const modelService = { dispose: modelDispose } as unknown as LocalModelService;
  const runtimeClose = vi.fn(async () => undefined);
  const runtime = { host: {}, close: runtimeClose } as unknown as ModelAwareRuntime;
  const releaseProxyEnvironment = vi.fn();
  const values: ProductionRuntimeDependencies = {
    openStore: vi.fn(() => store),
    createPathVerifier: vi.fn((_securityRoot: string) => verifier),
    createModelStorage: vi.fn(() => storage),
    createModelService: vi.fn(() => modelService),
    createRuntime: vi.fn(() => runtime),
    acquireProxyEnvironment: vi.fn(() => releaseProxyEnvironment),
  };
  return {
    values,
    store,
    storeClose,
    storeListSessions,
    verifier,
    storage,
    modelService,
    modelDispose,
    runtime,
    runtimeClose,
    releaseProxyEnvironment,
  };
}

describe("production runtime composition", () => {
  it.runIf(process.platform === "win32")(
    "starts with native path security when the scoped data directory does not exist",
    async () => {
      const parent = await mkdtemp(join(tmpdir(), "lyonbrown-agentd-runtime-"));
      const dataDirectory = join(parent, "data");
      try {
        const runtime = await createProductionAgentdRuntime({
          canonicalWorkspace: parent,
          dataDirectory,
          scopeId: "f".repeat(64),
        });
        await expect(stat(join(dataDirectory, "events"))).resolves.toMatchObject({});
        await runtime.close();
      } finally {
        await rm(parent, { recursive: true, force: true });
      }
    },
  );

  it("uses native capabilities under the scoped data directory", async () => {
    const state = dependencies();

    const runtime = await createProductionAgentdRuntime(CONTEXT, state.values);
    expect(runtime.host).toBe(state.runtime.host);
    expect(state.values.acquireProxyEnvironment).toHaveBeenCalledOnce();
    expect(state.values.createPathVerifier).toHaveBeenCalledWith(CONTEXT.dataDirectory);
    expect(state.values.openStore).toHaveBeenCalledWith({
      dataDirectory: join(CONTEXT.dataDirectory, "events"),
      pathVerifier: state.verifier,
    });
    expect(state.values.createModelStorage).toHaveBeenCalledWith(
      join(CONTEXT.dataDirectory, "models"),
    );
    expect(state.values.createModelService).toHaveBeenCalledWith(state.storage);
    expect(state.values.createRuntime).toHaveBeenCalledOnce();
    const runtimeOptions = vi.mocked(state.values.createRuntime).mock.calls[0]?.[0];
    expect(runtimeOptions).toMatchObject({
      sessionId: CONTEXT.scopeId,
      modelService: state.modelService,
      eventStore: state.store,
    });
    expect(typeof runtimeOptions?.createRuntime).toBe("function");

    await runtime.close();
    await runtime.close();
    expect(state.runtimeClose).toHaveBeenCalledOnce();
    expect(state.releaseProxyEnvironment).toHaveBeenCalledOnce();
  });

  it("rolls back the store when later composition fails", async () => {
    const state = dependencies();
    const failure = new Error("model service failed");
    const values: ProductionRuntimeDependencies = {
      ...state.values,
      createModelService: vi.fn(() => {
        throw failure;
      }),
    };

    const caught = await createProductionAgentdRuntime(CONTEXT, values).catch(
      (error: unknown) => error,
    );
    expect(caught).toBeInstanceOf(AggregateError);
    expect((caught as AggregateError).errors).toEqual([failure]);
    expect((caught as Error & { cause?: unknown }).cause).toBe(failure);
    expect(state.storeClose).toHaveBeenCalledOnce();
    expect(state.releaseProxyEnvironment).toHaveBeenCalledOnce();
  });

  it("disposes the model service before rolling back when runtime construction fails", async () => {
    const state = dependencies();
    const failure = new Error("runtime failed");
    const values: ProductionRuntimeDependencies = {
      ...state.values,
      createRuntime: vi.fn(() => {
        throw failure;
      }),
    };

    const caught = await createProductionAgentdRuntime(CONTEXT, values).catch(
      (error: unknown) => error,
    );
    expect(caught).toBeInstanceOf(AggregateError);
    expect((caught as AggregateError).errors).toEqual([failure]);
    expect(state.modelDispose).toHaveBeenCalledOnce();
    expect(state.storeClose).toHaveBeenCalledOnce();
    expect(state.releaseProxyEnvironment).toHaveBeenCalledOnce();
  });

  it("resumes the most recently updated workspace session", async () => {
    const state = dependencies();
    state.storeListSessions.mockResolvedValue([
      {
        sessionId: "recovered-session",
        currentRevision: 4n,
        createdAt: "2026-10-06T00:00:00.000Z",
        updatedAt: "2026-10-06T00:01:00.000Z",
      },
    ]);

    await createProductionAgentdRuntime(CONTEXT, state.values);
    const runtimeOptions = vi.mocked(state.values.createRuntime).mock.calls[0]?.[0];
    expect(runtimeOptions).toMatchObject({
      sessionId: "recovered-session",
      modelService: state.modelService,
      eventStore: state.store,
    });
    expect(typeof runtimeOptions?.createRuntime).toBe("function");
  });

  it("attempts rollback in reverse order and aggregates every cleanup failure", async () => {
    const order: string[] = [];
    const state = dependencies();
    const startup = new Error("runtime failed");
    const disposeFailure = new Error("model dispose failed");
    const closeFailure = new Error("store close failed");
    const modelService = {
      dispose: vi.fn(() => {
        order.push("model");
        return Promise.reject(disposeFailure);
      }),
    } as unknown as LocalModelService;
    const values: ProductionRuntimeDependencies = {
      ...state.values,
      createModelService: vi.fn(() => modelService),
      createRuntime: vi.fn(() => {
        throw startup;
      }),
      acquireProxyEnvironment: vi.fn(() => () => {
        order.push("proxy");
      }),
      openStore: vi.fn(
        () =>
          ({
            listSessions: vi.fn(async () => []),
            close: () => {
              order.push("store");
              throw closeFailure;
            },
          }) as unknown as EventStore,
      ),
    };

    const caught = await createProductionAgentdRuntime(CONTEXT, values).catch(
      (error: unknown) => error,
    );
    expect(order).toEqual(["model", "proxy", "store"]);
    expect(caught).toBeInstanceOf(AggregateError);
    expect((caught as AggregateError).errors).toEqual([startup, disposeFailure, closeFailure]);
    expect((caught as Error & { cause?: unknown }).cause).toBe(startup);
  });
});
