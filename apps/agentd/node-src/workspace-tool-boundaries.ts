import type {
  KernelJsonValue,
  KernelToolDefinition,
  KernelToolExecutionContext,
} from "@lyonbrown/kernel-pi";

interface PiToolResultBoundary {
  readonly content: readonly unknown[];
  readonly details?: unknown;
  readonly isError?: boolean;
}

export interface PiToolBoundary {
  readonly name: string;
  readonly label: string;
  readonly description: string;
  readonly parameters: unknown;
  readonly execute: unknown;
}

export function adaptPiTool(
  tool: PiToolBoundary,
  risk: KernelToolDefinition["risk"],
  permissions: readonly string[],
): KernelToolDefinition {
  const execute = tool.execute as (
    callId: string,
    arguments_: Readonly<Record<string, unknown>>,
    signal?: AbortSignal,
  ) => Promise<PiToolResultBoundary>;
  return Object.freeze({
    name: tool.name,
    label: tool.label,
    description: tool.description,
    inputSchema: schemaRecord(tool.parameters),
    permissions: Object.freeze([...permissions]),
    risk,
    async execute(context: KernelToolExecutionContext) {
      const result = await execute(context.callId, context.arguments, context.signal);
      if (result.isError === true) throw new Error("workspace tool failed");
      return {
        content: textContent(result.content),
        ...(result.details === undefined ? {} : { details: copyJson(result.details) }),
      };
    },
  });
}

function schemaRecord(value: unknown): Readonly<Record<string, unknown>> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Pi tool schema is invalid");
  }
  return value as Readonly<Record<string, unknown>>;
}

function textContent(blocks: readonly unknown[]): string {
  return blocks
    .map((block) => {
      if (
        typeof block !== "object" ||
        block === null ||
        !("type" in block) ||
        !("text" in block) ||
        block.type !== "text" ||
        typeof block.text !== "string"
      ) {
        throw new Error("workspace tool returned unsupported content");
      }
      return block.text;
    })
    .join("\n");
}

function copyJson(value: unknown, depth = 0): KernelJsonValue {
  if (depth > 16) throw new Error("workspace tool details are invalid");
  if (value === null || typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (Array.isArray(value)) return value.map((item) => copyJson(item, depth + 1));
  if (typeof value !== "object" || Reflect.getPrototypeOf(value) !== Object.prototype) {
    throw new Error("workspace tool details are invalid");
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [key, copyJson(item, depth + 1)]),
  );
}
