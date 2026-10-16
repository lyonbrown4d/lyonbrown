import { realpath } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";

import { configureFsSafeNative } from "@openclaw/fs-safe/config";
import { root as createSafeRoot, type Root } from "@openclaw/fs-safe/root";

export const MAX_WORKSPACE_FILE_BYTES = 1024 * 1024;

export interface WorkspaceFileOperations {
  readonly read: {
    access(path: string): Promise<void>;
    readFile(path: string): Promise<Buffer>;
  };
  readonly write: {
    mkdir(path: string): Promise<void>;
    writeFile(path: string, content: string): Promise<void>;
  };
}

configureFsSafeNative({ mode: "require" });

export async function createWorkspaceFileOperations(
  workspace: string,
): Promise<WorkspaceFileOperations> {
  const workspaceRoot = await createWorkspaceRoot(workspace);

  return Object.freeze({
    read: Object.freeze({
      async access(candidate: string): Promise<void> {
        const opened = await workspaceRoot.open(toRelative(workspaceRoot, candidate));
        await opened.handle.close();
      },
      async readFile(candidate: string): Promise<Buffer> {
        return workspaceRoot.readBytes(toRelative(workspaceRoot, candidate), {
          maxBytes: MAX_WORKSPACE_FILE_BYTES,
        });
      },
    }),
    write: Object.freeze({
      async mkdir(candidate: string): Promise<void> {
        const path = toRelative(workspaceRoot, candidate);
        if (path === "") {
          await workspaceRoot.ensureRoot();
          return;
        }
        await workspaceRoot.mkdir(path);
      },
      async writeFile(candidate: string, content: string): Promise<void> {
        if (Buffer.byteLength(content, "utf8") > MAX_WORKSPACE_FILE_BYTES) {
          throw new Error("workspace file exceeds size limit");
        }
        await workspaceRoot.write(toRelative(workspaceRoot, candidate), content, {
          encoding: "utf8",
          overwrite: true,
        });
      },
    }),
  });
}

export function createWorkspaceRoot(workspace: string): Promise<Root> {
  return createSafeRoot(workspace, {
    durable: true,
    hardlinks: "reject",
    maxBytes: MAX_WORKSPACE_FILE_BYTES,
    mkdir: true,
    mutationSymlinks: "follow-parents-within-root",
    symlinks: "follow-within-root",
  });
}

export function resolveWorkspaceRoot(workspace: string): Promise<string> {
  return realpath(resolve(workspace));
}

export function toRelative(workspaceRoot: Root, candidate: string): string {
  const absolute = isAbsolute(candidate)
    ? resolve(candidate)
    : resolve(workspaceRoot.rootReal, candidate);
  const pathFromRoot = relative(workspaceRoot.rootReal, absolute);
  if (pathFromRoot === "") return "";
  if (
    pathFromRoot === ".." ||
    pathFromRoot.startsWith("../") ||
    pathFromRoot.startsWith(`..\\`) ||
    isAbsolute(pathFromRoot)
  ) {
    throw new Error("workspace path denied");
  }
  return pathFromRoot;
}
