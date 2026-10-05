import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import type { AgentLauncher } from "@lyonbrown/agent-launch";
import { createProductionAgentLauncher } from "@lyonbrown/agent-launch/composition";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";

import { VERSION } from "./cli.ts";

const EFFECTIVE_CONFIG_FINGERPRINT = "production-default";

export function createProductionMainDependencies(): { readonly launcher: AgentLauncher } {
  const workspace = process.cwd();
  const agentdEntry = fileURLToPath(new URL("../../agentd/dist/main.js", import.meta.url));
  return {
    launcher: createProductionAgentLauncher({
      nativeProvider: createControlPlaneNativeProvider(),
      homeDirectory: homedir(),
      workspace,
      effectiveConfigFingerprint: EFFECTIVE_CONFIG_FINGERPRINT,
      agentd: {
        command: process.execPath,
        arguments: [agentdEntry],
        workingDirectory: workspace,
      },
      client: { name: "agent-cli", version: VERSION },
    }),
  };
}
