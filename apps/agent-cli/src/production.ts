import { createRequire } from "node:module";
import { homedir } from "node:os";
import { join } from "node:path";
import { createProductionAgentLauncher } from "@lyonbrown/agent-launch/composition";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";

import { VERSION } from "@lyonbrown/agent-cli/internal/cli";
import { createJsonPreferenceStore } from "@lyonbrown/agent-cli/internal/client-preferences";

const EFFECTIVE_CONFIG_FINGERPRINT = "production-default";
const require = createRequire(import.meta.url);

export function createProductionMainDependencies() {
  const workspace = process.cwd();
  const homeDirectory = homedir();
  const agentdEntry = require.resolve("@lyonbrown/agentd/daemon");
  return {
    preferenceStore: createJsonPreferenceStore(
      join(homeDirectory, ".lyonbrown", "client-preferences.json"),
    ),
    launcher: createProductionAgentLauncher({
      nativeProvider: createControlPlaneNativeProvider(),
      homeDirectory,
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
