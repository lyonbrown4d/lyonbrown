import { readFile } from "node:fs/promises";
import * as http from "node:http";
import { win32 } from "node:path";
import { pathToFileURL } from "node:url";

import type { AgentRuntimeState } from "@lyonbrown/agent-domain";
import type { AgentRuntime, RuntimePreKernelDecision } from "@lyonbrown/agent-runtime";
import { parse, populate } from "dotenv";
import { $, usePowerShell } from "zx";

import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";

const BUILD_TIMEOUT_MS = 15 * 60_000;
const INSTALL_TIMEOUT_MS = 60 * 60_000;
const DECISION_TIMEOUT_MS = 120_000;
const MODEL_ROOT_ENV = "LYONBROWN_DECISION_MODEL_ROOT";
const FIXTURES = Object.freeze([
  "Explain what this repository does.",
  "Change the TypeScript implementation and run its tests.",
  "请解释这个项目的核心架构。",
  "请修改 TypeScript 实现并运行测试。",
]);

interface SmokeResult {
  readonly input: string;
  readonly latencyMs: number;
  readonly outcome: RuntimePreKernelDecision;
  readonly durableSequence: readonly string[];
}

configureZxPlatform(process.platform, usePowerShell);

export async function main(): Promise<void> {
  const environment = await loadLocalEnvironment();
  const modelRoot = requireFDriveRoot(environment);
  const restoreProxy = configureNodeEnvironmentProxy();
  try {
    await runSmoke(modelRoot, process.argv.slice(2).includes("--install-only"));
  } finally {
    restoreProxy();
  }
}

async function runSmoke(modelRoot: string, installOnly: boolean): Promise<void> {
  await withDeadline(
    "decision dependency build",
    BUILD_TIMEOUT_MS,
    $({ stdio: "inherit" })`pnpm exec moon run agentd:build`,
  );
  const native = await import("@lyonbrown/control-plane-native");
  const decision = await import("@lyonbrown/decision-engine/composition");
  const storage = native.createNativeModelBundleStorageCapability(modelRoot);
  let lastPercent = -1;
  const install = await withDeadline(
    "decision model install",
    INSTALL_TIMEOUT_MS,
    decision.installBundledDecisionModel(storage, {
      operationId: `real-decision-${String(Date.now())}`,
      signal: AbortSignal.timeout(INSTALL_TIMEOUT_MS),
      onProgress: ({ downloadedBytes, totalBytes }) => {
        const percent = Math.floor((downloadedBytes * 100) / totalBytes);
        if (percent === lastPercent || (percent < 100 && percent % 5 !== 0)) return;
        lastPercent = percent;
        process.stdout.write(`decision model ${String(percent)}%\n`);
      },
    }),
  );

  const cached = await decision.installBundledDecisionModel(storage, {
    operationId: `real-decision-cache-${String(Date.now())}`,
    signal: AbortSignal.timeout(INSTALL_TIMEOUT_MS),
    fetcher: () => Promise.reject(new Error("network access forbidden during cache verification")),
  });
  if (cached.kind !== "already_installed") throw new Error("DECISION_MODEL_CACHE_REUSE_FAILED");
  if (installOnly) {
    process.stdout.write(
      `${JSON.stringify({
        marker: "DECISION_MODEL_INSTALL_OK",
        modelRoot,
        install: install.kind,
        cache: cached.kind,
      })}\n`,
    );
    return;
  }

  const agentDomain = await import("@lyonbrown/agent-domain");
  const agentRuntime = await import("@lyonbrown/agent-runtime");
  const agentd = await import("@lyonbrown/agentd/composition");
  const eventStore = await import("@lyonbrown/event-store");
  const sessionId = agentDomain.asSessionId(`real-decision-smoke-${String(Date.now())}`);
  const store = eventStore.openEventStore({ testOnlyMemory: true });
  const service = agentd.createProductionDecisionService({
    ...process.env,
    [MODEL_ROOT_ENV]: modelRoot,
  });
  const decisions: RuntimePreKernelDecision[] = [];
  const kernelStarts: string[] = [];
  const runtime = await agentRuntime.createAgentRuntime({
    sessionId,
    eventStore: agentRuntime.createRuntimeEventStorePort(store),
    createEventId: sequentialEventIds(),
    decision: {
      async evaluate(request) {
        const outcome = await service.port.evaluate(request);
        decisions.push(outcome);
        return outcome;
      },
    },
    createKernelSession: ({ eventSink }) => ({
      async startTurn(input) {
        kernelStarts.push(input);
        await eventSink.append({ type: "run_started", sessionId });
        await eventSink.append({ type: "run_completed", sessionId });
      },
      steer: () => undefined,
      followUp: () => undefined,
      cancel: () => undefined,
      subscribe: () => () => undefined,
      dispose: () => undefined,
    }),
  });
  const results: SmokeResult[] = [];
  let peakRssBytes = process.memoryUsage().rss;
  try {
    for (const input of FIXTURES) {
      const previousRevision = runtime.snapshot().revision;
      const previousDecisionCount = decisions.length;
      const started = performance.now();
      await withDeadline(
        "decision inference",
        DECISION_TIMEOUT_MS,
        runtime.startTurn(input, { signal: AbortSignal.timeout(DECISION_TIMEOUT_MS) }),
      );
      await waitForCompleted(runtime);
      const latencyMs = performance.now() - started;
      peakRssBytes = Math.max(peakRssBytes, process.memoryUsage().rss);
      const outcome = decisions.at(-1);
      if (decisions.length !== previousDecisionCount + 1) {
        throw new Error("DECISION_MODEL_DECISION_COUNT_FAILED");
      }
      if (outcome?.kind !== "decided") {
        throw new Error("DECISION_MODEL_REAL_INFERENCE_FAILED");
      }
      const durableSequence = (
        await store.load(sessionId, { afterRevision: previousRevision, limit: 10 })
      ).map(({ type }) => type);
      if (
        durableSequence.join(",") !==
        "user_message_submitted,decision_recorded,run_started,run_completed"
      ) {
        throw new Error("DECISION_MODEL_DURABLE_ORDER_FAILED");
      }
      results.push({ input, latencyMs, outcome, durableSequence });
    }
  } finally {
    await runtime.dispose();
    await service.dispose();
    store.close();
  }
  if (kernelStarts.length !== FIXTURES.length)
    throw new Error("DECISION_MODEL_PI_CONTINUATION_FAILED");

  const latencies = results.map(({ latencyMs }) => latencyMs).sort((left, right) => left - right);
  process.stdout.write(
    `${JSON.stringify({
      marker: "REAL_DECISION_MODEL_SMOKE_OK",
      modelRoot,
      install: install.kind,
      cache: cached.kind,
      p50Ms: percentile(latencies, 0.5),
      p95Ms: percentile(latencies, 0.95),
      peakRssBytes,
      results,
    })}\n`,
  );
}

type HttpWithEnvironmentProxy = typeof http & {
  setGlobalProxyFromEnv(): () => void;
};

function configureNodeEnvironmentProxy(): () => void {
  return (http as HttpWithEnvironmentProxy).setGlobalProxyFromEnv();
}

async function loadLocalEnvironment(): Promise<NodeJS.ProcessEnv> {
  const environment: NodeJS.ProcessEnv = { ...process.env };
  try {
    const source = await readFile(".env.local", "utf8");
    populate(environment, parse(source), { override: false });
  } catch (error) {
    if (!isMissingFile(error)) throw error;
  }
  return environment;
}

function isMissingFile(error: unknown): boolean {
  return (
    error instanceof Error &&
    "code" in error &&
    typeof error.code === "string" &&
    error.code === "ENOENT"
  );
}

function requireFDriveRoot(environment: Readonly<NodeJS.ProcessEnv>): string {
  const modelRoot = environment[MODEL_ROOT_ENV];
  if (modelRoot === undefined) throw new Error("DECISION_MODEL_ROOT_MISSING");
  const normalized = process.platform === "win32" ? win32.normalize(modelRoot) : modelRoot;
  if (process.platform === "win32" && win32.parse(normalized).root.toUpperCase() !== "F:\\") {
    throw new Error("DECISION_MODEL_ROOT_MUST_USE_F_DRIVE");
  }
  return normalized;
}

async function withDeadline<T>(label: string, timeoutMs: number, task: Promise<T>): Promise<T> {
  let timeout: NodeJS.Timeout | undefined;
  const deadline = new Promise<never>((_resolve, reject) => {
    timeout = setTimeout(() => {
      reject(new Error(`${label} timed out`));
    }, timeoutMs);
    timeout.unref();
  });
  try {
    return await Promise.race([task, deadline]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
}

async function waitForCompleted(runtime: AgentRuntime): Promise<AgentRuntimeState> {
  const deadline = performance.now() + DECISION_TIMEOUT_MS;
  while (performance.now() < deadline) {
    const snapshot = runtime.snapshot();
    if (snapshot.phase === "completed") return snapshot;
    if (snapshot.phase === "failed_terminal") throw new Error("DECISION_MODEL_RUNTIME_FAILED");
    await new Promise<void>((resolve) => setTimeout(resolve, 10));
  }
  throw new Error("decision runtime completion timed out");
}

function sequentialEventIds(): () => string {
  let sequence = 0;
  return () => `real-decision-event-${String(++sequence)}`;
}

function percentile(sorted: readonly number[], quantile: number): number {
  if (sorted.length === 0) return 0;
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * quantile) - 1);
  return Number((sorted[index] ?? 0).toFixed(2));
}

const entry = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (entry === import.meta.url) {
  void main().catch((error: unknown) => {
    process.stderr.write(
      `${error instanceof Error ? error.message : "REAL_DECISION_MODEL_SMOKE_FAILED"}\n`,
    );
    process.exitCode = 1;
  });
}
