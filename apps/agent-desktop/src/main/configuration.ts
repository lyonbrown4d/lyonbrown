import { isAbsolute } from "node:path";
export interface DesktopConfiguration {
  readonly workspace: string;
  readonly homeDirectory: string;
  readonly nodeExecutable: string;
  readonly autoStart: boolean;
}
export function loadDesktopConfiguration(
  environment: Readonly<NodeJS.ProcessEnv>,
  defaults: { readonly workspace: string; readonly homeDirectory: string },
): DesktopConfiguration {
  const workspace = environment.LYONBROWN_DESKTOP_WORKSPACE ?? defaults.workspace;
  const homeDirectory = environment.LYONBROWN_DESKTOP_HOME ?? defaults.homeDirectory;
  const nodeExecutable = environment.LYONBROWN_DESKTOP_NODE_EXECUTABLE ?? "node";
  const autoStart = environment.LYONBROWN_DESKTOP_AUTOSTART ?? "true";
  if (
    !isAbsolute(workspace) ||
    !isAbsolute(homeDirectory) ||
    workspace.includes("\0") ||
    homeDirectory.includes("\0") ||
    !nodeExecutable.trim() ||
    nodeExecutable.includes("\0") ||
    !["true", "false"].includes(autoStart)
  )
    throw new Error("INVALID_DESKTOP_CONFIGURATION");
  return { workspace, homeDirectory, nodeExecutable, autoStart: autoStart === "true" };
}
