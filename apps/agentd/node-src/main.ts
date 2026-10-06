import { homedir } from "node:os";
import { pathToFileURL } from "node:url";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";
import { ControlPlaneError } from "@lyonbrown/local-control-plane";
import {
  AgentdBootstrapError,
  startProductionAgentd,
  type RunningProductionAgentd,
} from "./bootstrap.ts";
import { AgentdCliError, parseAgentdArguments } from "./cli-arguments.ts";
import { waitForShutdownSignal } from "./process-lifecycle.ts";
import { createProductionAgentdRuntime } from "./production-runtime.ts";

export interface AgentdMainDependencies {
  readonly homeDirectory: () => string;
  readonly createNativeProvider: typeof createControlPlaneNativeProvider;
  readonly startAgentd: typeof startProductionAgentd;
  readonly waitForShutdown: (running: RunningProductionAgentd) => Promise<void>;
}

const DEFAULT_DEPENDENCIES: AgentdMainDependencies = Object.freeze({
  homeDirectory: homedir,
  createNativeProvider: createControlPlaneNativeProvider,
  startAgentd: startProductionAgentd,
  waitForShutdown: waitForShutdownSignal,
});

export async function main(
  arguments_: readonly string[] = process.argv.slice(2),
  dependencies: AgentdMainDependencies = DEFAULT_DEPENDENCIES,
): Promise<void> {
  const options = parseAgentdArguments(arguments_);
  const running = await dependencies.startAgentd({
    workspace: options.workspace,
    homeDirectory: dependencies.homeDirectory(),
    effectiveConfigFingerprint: options.effectiveConfigFingerprint,
    expectedScopeId: options.scopeId,
    nativeProvider: dependencies.createNativeProvider(),
    createRuntime: createProductionAgentdRuntime,
  });
  await dependencies.waitForShutdown(running);
}

const entryUrl = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (entryUrl === import.meta.url) {
  void main().catch((error: unknown) => {
    const code =
      error instanceof ControlPlaneError ||
      error instanceof AgentdBootstrapError ||
      error instanceof AgentdCliError
        ? error.code
        : "AGENTD_START_FAILED";
    process.stderr.write(`${code}\n`);
    process.exitCode = 1;
  });
}
