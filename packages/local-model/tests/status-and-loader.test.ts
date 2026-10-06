import { describe, expect, it, vi } from "vitest";

import { DEFAULT_LOCAL_MODEL, type LocalModelLease, type LocalModelService } from "../src/index.ts";
import { createLocalModelManager } from "../src/internal/manager.ts";
import {
  createFakeLlamaLease,
  Deferred,
  FakeLlamaEngine,
  FakeModelIo,
  flushTasks,
} from "./support/fakes.ts";

describe("default local model status", () => {
  it("returns immediately in checking while cache verification runs in the background", async () => {
    const io = new FakeModelIo();
    const inspection = new Deferred<boolean>();
    io.inspection = inspection;

    const service = createLocalModelManager(io, new FakeLlamaEngine());

    expect(service.status()).toMatchObject({
      phase: "checking",
      operationId: null,
      allowedActions: [],
    });
    expect(service.startDownload()).toEqual({ kind: "checking" });
    await expect(service.load()).resolves.toEqual({ kind: "checking" });
    inspection.resolve(false);
    await flushTasks();
    expect(service.status().phase).toBe("setup-required");
    await service.dispose();
    await expect(service.load()).resolves.toEqual({
      kind: "unavailable",
      code: "service_disposed",
    });
  });

  it("sanitizes background cache inspection failures", async () => {
    const io = new FakeModelIo();
    const inspection = new Deferred<boolean>();
    io.inspection = inspection;
    const service = createLocalModelManager(io, new FakeLlamaEngine());

    inspection.reject(new Error("C:\\Users\\alice\\private model"));
    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "failed",
      failure: { code: "cache_inspection_failed", retryable: true },
    });
    expect(JSON.stringify(service.status())).not.toContain("alice");
    await service.dispose();
  });

  it("dispose aborts and joins an in-flight cache inspection", async () => {
    const io = new FakeModelIo();
    io.inspection = new Deferred<boolean>();
    const service = createLocalModelManager(io, new FakeLlamaEngine());

    const first = service.dispose();
    const second = service.dispose();

    expect(second).toBe(first);
    await expect(first).resolves.toBeUndefined();
  });

  it("does not retain subscribers added after disposal", async () => {
    const service = createLocalModelManager(new FakeModelIo(), new FakeLlamaEngine());
    await service.dispose();

    const unsubscribe = service.subscribe(vi.fn());
    const listeners = Reflect.get(service, "listeners") as Set<unknown>;

    expect(listeners.size).toBe(0);
    unsubscribe();
    expect(listeners.size).toBe(0);
  });

  it("pins the small Qwen3 preset without exposing its URL or path", () => {
    expect(DEFAULT_LOCAL_MODEL).toEqual({
      id: "qwen3-1.7b-q4_k_m",
      displayName: "Qwen3 1.7B Q4_K_M",
      expectedDownloadBytes: 1_107_408_544,
      maxBytes: 2_147_483_648,
      minimumMemoryBytes: 4_294_967_296,
      sha256: "228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f",
    });
    expect(Object.keys(DEFAULT_LOCAL_MODEL)).not.toContain("url");
    expect(Object.keys(DEFAULT_LOCAL_MODEL)).not.toContain("path");
  });

  it("reports setup-required without starting a download", async () => {
    const io = new FakeModelIo();
    const service = createLocalModelManager(io, new FakeLlamaEngine());
    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "setup-required",
      model: DEFAULT_LOCAL_MODEL,
      operationId: null,
      allowedActions: ["download"],
    });
    expect(io.downloadCalls).toBe(0);
    const external = service.status();
    (external.allowedActions as string[]).splice(0);
    expect(service.status().allowedActions).toEqual(["download"]);
  });

  it("reports ready for an installed verified model", async () => {
    const io = new FakeModelIo();
    io.installed = true;

    const service = createLocalModelManager(io, new FakeLlamaEngine());
    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "ready",
      operationId: null,
      allowedActions: [],
    });
    expect(io.downloadCalls).toBe(0);
  });
});

describe("llama runtime loading", () => {
  it("returns setup-required without touching llama.cpp when the model is absent", async () => {
    const engine = new FakeLlamaEngine();
    const service = createLocalModelManager(new FakeModelIo(), engine);
    await flushTasks();

    await expect(service.load()).resolves.toEqual({ kind: "setup-required" });
    expect(engine.loadCalls).toBe(0);
  });

  it("returns an opaque Lyonbrown runtime lease when ready", async () => {
    const io = new FakeModelIo();
    const engine = new FakeLlamaEngine();
    io.installed = true;
    const service: LocalModelService = createLocalModelManager(io, engine);
    await flushTasks();

    const result = await service.load();

    expect(result).toEqual({ kind: "ready", model: DEFAULT_LOCAL_MODEL, lease: engine.lease });
    expect(result).not.toHaveProperty("llama");
    expect(result).not.toHaveProperty("modelPath");
  });

  it("redacts native loader failures", async () => {
    const io = new FakeModelIo();
    const engine = new FakeLlamaEngine();
    io.installed = true;
    engine.error = new Error("C:\\Users\\alice\\private\\model.gguf failed");
    const service = createLocalModelManager(io, engine);
    await flushTasks();

    await expect(service.load()).resolves.toEqual({
      kind: "unavailable",
      code: "native_runtime_unavailable",
    });
  });

  it("tracks concurrent loads and disposes every late lease before disposal completes", async () => {
    const io = new FakeModelIo();
    io.installed = true;
    const first = new Deferred<LocalModelLease>();
    const second = new Deferred<LocalModelLease>();
    const firstDispose = vi.fn(() => Promise.resolve());
    const secondDispose = vi.fn(() => Promise.resolve());
    const engine = {
      loadDefault: vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise),
    };
    const service = createLocalModelManager(io, engine);
    await flushTasks();

    const firstLoad = service.load();
    const secondLoad = service.load();
    const disposal = service.dispose();
    let disposalFinished = false;
    void disposal.then(() => {
      disposalFinished = true;
    });
    await flushTasks();
    expect(disposalFinished).toBe(false);
    const firstSignal = engine.loadDefault.mock.calls[0]?.[0] as AbortSignal | undefined;
    const secondSignal = engine.loadDefault.mock.calls[1]?.[0] as AbortSignal | undefined;
    expect(firstSignal?.aborted).toBe(true);
    expect(secondSignal?.aborted).toBe(true);

    first.resolve(createFakeLlamaLease(firstDispose));
    second.resolve(createFakeLlamaLease(secondDispose));

    await expect(firstLoad).resolves.toEqual({
      kind: "unavailable",
      code: "service_disposed",
    });
    await expect(secondLoad).resolves.toEqual({
      kind: "unavailable",
      code: "service_disposed",
    });
    await expect(disposal).resolves.toBeUndefined();
    expect(firstDispose).toHaveBeenCalledOnce();
    expect(secondDispose).toHaveBeenCalledOnce();
    expect(engine.loadDefault).toHaveBeenCalledTimes(2);
  });

  it("bounds shutdown while still fencing and disposing a load that resolves late", async () => {
    const io = new FakeModelIo();
    io.installed = true;
    const lateLease = new Deferred<LocalModelLease>();
    const leaseDispose = vi.fn(() => Promise.resolve());
    const engine = {
      loadDefault: vi.fn((signal: AbortSignal) => {
        void signal;
        return lateLease.promise;
      }),
    };
    const service = createLocalModelManager(io, engine, () => "unused", 10);
    await flushTasks();

    const load = service.load();
    await expect(service.dispose()).resolves.toBeUndefined();
    expect(engine.loadDefault.mock.calls[0]?.[0].aborted).toBe(true);

    lateLease.resolve(createFakeLlamaLease(leaseDispose));
    await expect(load).resolves.toEqual({
      kind: "unavailable",
      code: "service_disposed",
    });
    expect(leaseDispose).toHaveBeenCalledOnce();
  });
});
