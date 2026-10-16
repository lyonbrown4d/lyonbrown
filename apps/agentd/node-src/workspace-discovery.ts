import { posix } from "node:path";

import type { Root } from "@openclaw/fs-safe/root";
import type { KernelToolDefinition, KernelToolExecutionContext } from "@lyonbrown/kernel-pi";

import { toRelative } from "@lyonbrown/agentd/internal/workspace-paths";

const MAX_ENTRIES = 4_000;
const MAX_RESULTS = 100;
const MAX_SEARCH_FILE_BYTES = 256 * 1024;

function directory(arguments_: Readonly<Record<string, unknown>>, root: Root): string {
  const value = arguments_.path ?? ".";
  if (typeof value !== "string" || value.length > 4_096) throw new Error("workspace path invalid");
  return toRelative(root, value);
}

function limit(value: unknown, fallback: number): number {
  if (value === undefined) return fallback;
  if (!Number.isInteger(value) || typeof value !== "number" || value < 1 || value > MAX_RESULTS) {
    throw new Error("result limit invalid");
  }
  return value;
}

function tool(
  name: string,
  description: string,
  properties: Readonly<Record<string, unknown>>,
  required: readonly string[],
  execute: KernelToolDefinition["execute"],
): KernelToolDefinition {
  return Object.freeze({
    name,
    label: name,
    description,
    inputSchema: { type: "object", properties, required, additionalProperties: false },
    permissions: ["workspace.read"],
    risk: "low",
    execute,
  });
}

const pathSchema = { type: "string", maxLength: 4_096 };
const limitSchema = { type: "integer", minimum: 1, maximum: MAX_RESULTS };

export function createWorkspaceDiscoveryTools(root: Root): readonly KernelToolDefinition[] {
  const list = tool(
    "list_directory",
    "List immediate workspace directory entries. Symbolic links are omitted.",
    { path: pathSchema, limit: limitSchema },
    [],
    async (context: KernelToolExecutionContext) => {
      const path = directory(context.arguments, root);
      const max = limit(context.arguments.limit, 100);
      const entries: { path: string; kind: string; size: number }[] = [];
      let truncated = false;
      for await (const entry of root.entries(path || ".", {
        maxEntries: MAX_ENTRIES,
        order: "sorted",
        symlinks: "follow-within-root",
        signal: context.signal,
      })) {
        if (entry.isSymbolicLink) continue;
        if (entries.length >= max) {
          truncated = true;
          break;
        }
        entries.push({
          path: posix.join(path.replaceAll("\\", "/"), entry.name),
          kind: entry.isDirectory ? "directory" : entry.isFile ? "file" : "other",
          size: entry.size,
        });
      }
      return { content: JSON.stringify({ entries, truncated }) };
    },
  );
  const search = tool(
    "search_files",
    "Search UTF-8 workspace files for a literal string. Symbolic links and large files are omitted.",
    {
      path: pathSchema,
      query: { type: "string", minLength: 1, maxLength: 256 },
      limit: limitSchema,
    },
    ["query"],
    async (context: KernelToolExecutionContext) => {
      const path = directory(context.arguments, root);
      const query = context.arguments.query;
      if (typeof query !== "string" || query.length < 1 || query.length > 256) {
        throw new Error("search query invalid");
      }
      const max = limit(context.arguments.limit, 50);
      const matches: { path: string; line: number; text: string }[] = [];
      let truncated = false;
      let skippedLarge = 0;
      let skippedUnreadable = 0;
      for await (const entry of root.walk(path, {
        maxDepth: 32,
        maxEntries: MAX_ENTRIES,
        order: "sorted",
        symlinkPolicy: "skip",
        signal: context.signal,
      })) {
        if (entry.kind === "truncated") {
          truncated = true;
          break;
        }
        if (entry.kind !== "file") continue;
        if (entry.size > MAX_SEARCH_FILE_BYTES) {
          skippedLarge += 1;
          continue;
        }
        let bytes: Buffer;
        try {
          bytes = await root.readBytes(posix.join(".", entry.relativePath), {
            maxBytes: MAX_SEARCH_FILE_BYTES,
          });
        } catch {
          context.signal.throwIfAborted();
          // Files can disappear, change type, or grow during a bounded search.
          skippedUnreadable += 1;
          continue;
        }
        context.signal.throwIfAborted();
        if (bytes.includes(0)) continue;
        const lines = bytes.toString("utf8").split(/\r?\n/u);
        for (let index = 0; index < lines.length; index += 1) {
          context.signal.throwIfAborted();
          const line = lines[index] ?? "";
          if (!line.includes(query)) continue;
          matches.push({ path: entry.relativePath, line: index + 1, text: line.slice(0, 300) });
          if (matches.length >= max) {
            truncated = true;
            break;
          }
        }
        if (truncated) break;
      }
      return { content: JSON.stringify({ matches, truncated, skippedLarge, skippedUnreadable }) };
    },
  );
  return [list, search];
}
