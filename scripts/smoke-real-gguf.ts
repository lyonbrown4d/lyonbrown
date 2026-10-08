import { access, mkdtemp, mkdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import type { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";
import type { deriveScopedDaemonPaths } from "@lyonbrown/local-control-plane";
import type { LocalModelDescriptor } from "@lyonbrown/local-model";
import { config as loadEnvironment } from "dotenv";
import { $, usePowerShell } from "zx";

import { runRealGgufSmokeFlow } from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";
import { parseRealGgufSmokeAuthorization } from "@lyonbrown/workspace/internal/smoke-real-gguf-lib";
import type { IndependentGgufSmokePort } from "@lyonbrown/workspace/internal/smoke-real-gguf-port";
import {
  childEnvironment,
  removeOwnedRunDirectory,
  requiredAbsoluteEnvironmentPath,
} from "@lyonbrown/workspace/internal/real-tui-blackbox-process";
import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";

const FINGERPRINT = "real-gguf-release-smoke-v2";
const DEPENDENCY_BUILD_TIMEOUT_MS = 15 * 60 * 1_000;
const PHASE_TIMEOUT_MS = 120_000;
const DOWNLOAD_TIMEOUT_MS = 60 * 60_000;
const CLOSE_TIMEOUT_MS = 30_000;
const require = createRequire(import.meta.url);
configureZxPlatform(process.platform, usePowerShell);

export async function main(
  arguments_: readonly string[] = process.argv.slice(2),
  environment: Readonly<Record<string, string | undefined>> = process.env,
): Promise<void> {
  let scratchRoot: string | undefined;
  let temporaryRoot: string | undefined;
  let port: IndependentGgufSmokePort | undefined;
  let failure: unknown;
  try {
    parseRealGgufSmokeAuthorization(environment, arguments_);
    scratchRoot = requiredAbsoluteEnvironmentPath(environment, "LYONBROWN_BLACKBOX_ROOT");
    const modelsDirectory = requiredAbsoluteEnvironmentPath(
      environment,
      "LYONBROWN_LOCAL_MODEL_ROOT",
    );
    await buildRuntimeDependencies();
    const agentdEntry = require.resolve("@lyonbrown/agentd/daemon");
    await access(agentdEntry);
    const runtimeModules = await loadRuntimeModules();
    await mkdir(scratchRoot, { recursive: true });
    temporaryRoot = await mkdtemp(join(scratchRoot, "real-gguf-"));
    const homeDirectory = join(temporaryRoot, "home");
    const workspace = join(temporaryRoot, "workspace");
    await Promise.all([mkdir(homeDirectory), mkdir(workspace)]);
    const provider = runtimeModules.createControlPlaneNativeProvider();
    const paths = await runtimeModules.deriveScopedDaemonPaths(
      homeDirectory,
      workspace,
      FINGERPRINT,
      provider,
    );
    const environmentForDaemon = childEnvironment({ ...environment }, homeDirectory);
    environmentForDaemon.LYONBROWN_LOCAL_MODEL_ROOT = modelsDirectory;
    port = new runtimeModules.IndependentGgufSmokePort({
      agentdEntry,
      environment: environmentForDaemon,
      fingerprint: FINGERPRINT,
      homeDirectory,
      modelsDirectory,
      nativeProvider: provider,
      runDirectory: paths.runDirectory,
      scopeId: paths.scopeId,
      scratchRoot,
      cleanupRoot: temporaryRoot,
      workspace: paths.canonicalWorkspace,
      startupTimeoutMs: PHASE_TIMEOUT_MS,
      closeTimeoutMs: CLOSE_TIMEOUT_MS,
    });
    await port.start();
    await runRealGgufSmokeFlow(port, {
      phaseTimeoutMs: PHASE_TIMEOUT_MS,
      downloadTimeoutMs: DOWNLOAD_TIMEOUT_MS,
      pollIntervalMs: 100,
      cancelAfterBytes: 1024n * 1024n,
      closeTimeoutMs: CLOSE_TIMEOUT_MS,
      trustedModelDigests: Object.fromEntries(
        runtimeModules.trustedModels.map((model) => [model.id, model.sha256]),
      ),
    });
  } catch (error: unknown) {
    failure = error;
  }
  try {
    if (port !== undefined) await port.cleanup();
    else if (scratchRoot !== undefined && temporaryRoot !== undefined) {
      await removeOwnedRunDirectory(scratchRoot, temporaryRoot);
    }
  } catch (cleanupError: unknown) {
    failure =
      failure === undefined
        ? cleanupError
        : new AggregateError([failure, cleanupError], "real GGUF smoke cleanup failed", {
            cause: failure,
          });
  }
  if (failure !== undefined) {
    process.stderr.write(
      `${JSON.stringify({
        code: failure instanceof Error ? failure.message : "REAL_GGUF_SMOKE_FAILED",
        errors: smokeFailureMessages(failure),
        snapshot: port?.safeLastSnapshot() ?? null,
        daemonOutput: port?.safeDaemonOutput() ?? "",
      })}\n`,
    );
    throw failure instanceof Error ? failure : new Error("REAL_GGUF_SMOKE_FAILED");
  }
  process.stdout.write(
    `${JSON.stringify({
      marker: "REAL_GGUF_SMOKE_OK",
      transport: "independent-agentd-grpc",
      modelsDirectory: requiredAbsoluteEnvironmentPath(environment, "LYONBROWN_LOCAL_MODEL_ROOT"),
    })}\n`,
  );
}

function smokeFailureMessages(value: unknown): readonly string[] {
  const messages: string[] = [];
  const visit = (current: unknown): void => {
    if (current instanceof AggregateError) {
      messages.push(current.message);
      for (const nested of current.errors) visit(nested);
      return;
    }
    messages.push(current instanceof Error ? current.message : "REAL_GGUF_UNKNOWN_FAILURE");
  };
  visit(value);
  return messages;
}

async function buildRuntimeDependencies(): Promise<void> {
  await $({
    stdio: "inherit",
    timeout: DEPENDENCY_BUILD_TIMEOUT_MS,
  })`pnpm dependencies:build`;
}

interface RuntimeModules {
  readonly IndependentGgufSmokePort: typeof IndependentGgufSmokePort;
  readonly createControlPlaneNativeProvider: typeof createControlPlaneNativeProvider;
  readonly deriveScopedDaemonPaths: typeof deriveScopedDaemonPaths;
  readonly trustedModels: readonly LocalModelDescriptor[];
}

async function loadRuntimeModules(): Promise<RuntimeModules> {
  const [port, native, controlPlane, localModel] = await Promise.all([
    import("@lyonbrown/workspace/internal/smoke-real-gguf-port"),
    import("@lyonbrown/control-plane-native"),
    import("@lyonbrown/local-control-plane"),
    import("@lyonbrown/local-model"),
  ]);
  return {
    IndependentGgufSmokePort: port.IndependentGgufSmokePort,
    createControlPlaneNativeProvider: native.createControlPlaneNativeProvider,
    deriveScopedDaemonPaths: controlPlane.deriveScopedDaemonPaths,
    trustedModels: localModel.TRUSTED_LOCAL_MODELS,
  };
}

const entryUrl = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (entryUrl === import.meta.url) {
  loadEnvironment({ path: resolve(".env.local"), override: false, quiet: true });
  await main().then(
    () => {
      process.stdout.write("", () => process.exit(0));
    },
    (error: unknown) => {
      process.stderr.write(`${JSON.stringify({ entryFailure: safeErrorMessage(error) })}\n`, () =>
        process.exit(1),
      );
    },
  );
}

function safeErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "REAL_GGUF_UNKNOWN_FAILURE";
}
