import { createRequire } from "node:module";
import { homedir } from "node:os";
import type { AgentLauncher } from "@lyonbrown/agent-launch";
import { createProductionAgentLauncher } from "@lyonbrown/agent-launch/composition";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";

import { VERSION } from "@lyonbrown/agent-cli/internal/cli";

const EFFECTIVE_CONFIG_FINGERPRINT = "production-default";
const require = createRequire(import.meta.url);

export function createProductionMainDependencies(): { readonly launcher: AgentLauncher } {
  const workspace = process.cwd();
  const agentdEntry = require.resolve("@lyonbrown/agentd/daemon");
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
