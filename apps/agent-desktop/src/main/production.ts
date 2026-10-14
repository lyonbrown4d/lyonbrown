import { createRequire } from "node:module";
import { createProductionAgentLauncher } from "@lyonbrown/agent-launch/composition";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";
import type { AgentLauncher } from "@lyonbrown/agent-launch";
import { runtimeConfiguration } from "@lyonbrown/agent-desktop/internal/main/environment";

export function productionLauncher(): AgentLauncher {
  const configuration = runtimeConfiguration();
  const require = createRequire(import.meta.url);
  const launcher = createProductionAgentLauncher({
    nativeProvider: createControlPlaneNativeProvider(),
    homeDirectory: configuration.homeDirectory,
    workspace: configuration.workspace,
    effectiveConfigFingerprint: "production-default",
    agentd: {
      command: configuration.nodeExecutable,
      arguments: [require.resolve("@lyonbrown/agentd/daemon")],
      workingDirectory: configuration.workspace,
    },
    client: { name: "agent-desktop", version: "0.0.0" },
  });
  return { connect: (input) => launcher.connect({ ...input, autoStart: configuration.autoStart }) };
}
