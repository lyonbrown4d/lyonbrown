import { createEditTool, createReadTool, createWriteTool } from "@earendil-works/pi-coding-agent";
import type { KernelToolRegistry } from "@lyonbrown/kernel-pi";

import { adaptPiTool } from "@lyonbrown/agentd/internal/workspace-tool-boundaries";
import {
  createWorkspaceFileOperations,
  resolveWorkspaceRoot,
} from "@lyonbrown/agentd/internal/workspace-paths";

export async function createWorkspaceToolRegistry(workspace: string): Promise<KernelToolRegistry> {
  const canonicalWorkspace = await resolveWorkspaceRoot(workspace);
  const operations = await createWorkspaceFileOperations(canonicalWorkspace);
  const definitions = [
    adaptPiTool(
      createReadTool(canonicalWorkspace, {
        autoResizeImages: false,
        operations: operations.read,
      }),
      "low",
      ["workspace.read"],
    ),
    adaptPiTool(createWriteTool(canonicalWorkspace, { operations: operations.write }), "medium", [
      "workspace.write",
    ]),
    adaptPiTool(
      createEditTool(canonicalWorkspace, {
        operations: {
          ...operations.read,
          writeFile: (path, content) => operations.write.writeFile(path, content),
        },
      }),
      "medium",
      ["workspace.write"],
    ),
  ] as const;
  const byName = new Map(definitions.map((definition) => [definition.name, definition]));
  const permissions = new Set(["workspace.read", "workspace.write"]);
  return Object.freeze({
    list: () => definitions,
    get: (name: string) => byName.get(name),
    hasPermission: (permission: string) => permissions.has(permission),
  });
}
