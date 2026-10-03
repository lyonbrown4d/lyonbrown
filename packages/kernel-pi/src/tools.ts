import type { AgentTool, AgentToolResult } from "@earendil-works/pi-agent-core";
import type { JsonValue } from "@earendil-works/pi-ai";
import type { TSchema } from "typebox";
import { Value } from "typebox/value";

import type { BudgetTracker } from "./budget.js";
import { KernelRuntimeError, asSafeError } from "./errors.js";
import type { SecretRedactor } from "./sanitization.js";
import {
  assertByteLimit,
  isSecretReference,
  redactSecretReferences,
  sanitizeJson,
  sanitizeText,
} from "./sanitization.js";
import type {
  KernelEvent,
  KernelJsonValue,
  KernelToolDefinition,
  PiKernelSessionOptions,
} from "./types.js";

type EmitEvent = (event: KernelEvent) => Promise<void>;

export class ToolExecutor {
  constructor(
    private readonly options: PiKernelSessionOptions,
    private readonly tracker: () => BudgetTracker,
    private readonly redactor: SecretRedactor,
    private readonly emit: EmitEvent,
  ) {}

  createAgentTool(tool: KernelToolDefinition): AgentTool {
    return {
      name: tool.name,
      label: tool.label,
      description: tool.description,
      // Pi validates before execute; keep that layer structural so Lyonbrown can
      // validate symbolic SecretRef values against the real schema itself.
      parameters: { type: "object", additionalProperties: true } as TSchema,
      executionMode: "sequential",
      replay: "never",
      execute: async (callId, arguments_, signal) =>
        this.execute(tool, callId, arguments_ as Readonly<Record<string, unknown>>, signal),
    };
  }

  async reportUnknown(callId: string, toolName: string): Promise<void> {
    await this.emit({
      type: "tool_failed",
      callId: sanitizeText(callId),
      toolName: sanitizeText(toolName),
      code: "TOOL_DENIED",
      message: "Unknown tool denied",
    });
  }

  private async execute(
    tool: KernelToolDefinition,
    callId: string,
    arguments_: Readonly<Record<string, unknown>>,
    parentSignal?: AbortSignal,
  ): Promise<AgentToolResult> {
    const safeCallId = sanitizeText(callId);
    const safeToolName = sanitizeText(tool.name);
    try {
      this.tracker().step();
      this.assertPolicy(tool, arguments_);
      const preview = redactSecretReferences(arguments_, this.options.budget.maxValueDepth);
      const signal = createDeadlineSignal(parentSignal, this.options.budget.toolTimeoutMs);
      await this.requestApprovalIfNeeded(tool, safeCallId, preview, signal);
      await this.emit({
        type: "tool_started",
        callId: safeCallId,
        toolName: safeToolName,
        argumentsPreview: preview,
      });

      const resolved = await resolveSecrets(
        arguments_,
        this.options.budget.maxValueDepth,
        signal,
        this.options.secretResolver.resolve.bind(this.options.secretResolver),
        this.redactor,
      );
      const result = await raceAbort(
        tool.execute({ callId: safeCallId, arguments: resolved, signal }),
        signal,
      );
      const content = this.redactor.text(result.content);
      const details =
        result.details === undefined
          ? undefined
          : this.redactor.json(result.details, this.options.budget.maxValueDepth);
      assertByteLimit(
        JSON.stringify({ content, details }),
        this.options.budget.maxToolResultBytes,
        "TOOL_RESULT_BUDGET_EXCEEDED",
      );
      await this.emit({
        type: "tool_succeeded",
        callId: safeCallId,
        toolName: safeToolName,
        content,
        ...(details === undefined ? {} : { details }),
      });
      return {
        content: [{ type: "text", text: content }],
        details: details as JsonValue | undefined,
      };
    } catch (error: unknown) {
      const safe = this.toSafeToolError(error);
      await this.emit({
        type: "tool_failed",
        callId: safeCallId,
        toolName: safeToolName,
        code: safe.code,
        message: safe.message,
      });
      throw safe;
    }
  }

  private assertPolicy(
    tool: KernelToolDefinition,
    arguments_: Readonly<Record<string, unknown>>,
  ): void {
    if (!isKnownRisk(tool.risk)) {
      throw new KernelRuntimeError("TOOL_DENIED", "Tool has no recognized risk classification");
    }
    if (
      !tool.permissions.every((permission) => this.options.toolRegistry.hasPermission(permission))
    ) {
      throw new KernelRuntimeError("TOOL_DENIED", "Required tool permission is not granted");
    }
    const sanitized = sanitizeJson(arguments_, this.options.budget.maxValueDepth);
    if (!Value.Check(tool.inputSchema as TSchema, replaceSecretRefsForValidation(sanitized))) {
      throw new KernelRuntimeError(
        "TOOL_ARGUMENTS_INVALID",
        "Tool arguments do not match the declared schema",
      );
    }
    assertByteLimit(
      JSON.stringify(sanitized),
      this.options.budget.maxInputBytes,
      "INPUT_BUDGET_EXCEEDED",
    );
  }

  private async requestApprovalIfNeeded(
    tool: KernelToolDefinition,
    callId: string,
    preview: KernelJsonValue,
    signal: AbortSignal,
  ): Promise<void> {
    if (tool.risk === "low") return;
    try {
      const response = await raceAbort(
        this.options.approvalGate.request({
          sessionId: this.options.sessionId,
          callId,
          toolName: sanitizeText(tool.name),
          risk: tool.risk,
          permissions: tool.permissions.map(sanitizeText),
          argumentsPreview: preview,
          signal,
        }),
        signal,
      );
      if (response.decision !== "approve") {
        throw new KernelRuntimeError("TOOL_DENIED", "Tool approval denied");
      }
    } catch (error: unknown) {
      if (error instanceof KernelRuntimeError) throw error;
      throw new KernelRuntimeError("TOOL_DENIED", "Tool approval unavailable", { cause: error });
    }
  }

  private toSafeToolError(error: unknown): KernelRuntimeError {
    if (error instanceof KernelRuntimeError) {
      return new KernelRuntimeError(error.code, this.redactor.text(error.message));
    }
    const source = asSafeError(error);
    const timedOut = source.name === "TimeoutError" || source.name === "AbortError";
    return new KernelRuntimeError(
      timedOut ? "TOOL_TIMEOUT" : "MODEL_ERROR",
      timedOut ? "Tool execution timed out or was cancelled" : this.redactor.text(source.message),
    );
  }
}

function replaceSecretRefsForValidation(value: KernelJsonValue): KernelJsonValue {
  if (isSecretReference(value)) return "[SECRET]";
  if (Array.isArray(value)) {
    const entries = value as readonly KernelJsonValue[];
    return entries.map(replaceSecretRefsForValidation);
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [key, replaceSecretRefsForValidation(entry)]),
    );
  }
  return value;
}

function isKnownRisk(risk: unknown): risk is KernelToolDefinition["risk"] {
  return risk === "low" || risk === "medium" || risk === "high";
}

function createDeadlineSignal(parent: AbortSignal | undefined, timeoutMs: number): AbortSignal {
  const timeout = AbortSignal.timeout(timeoutMs);
  return parent ? AbortSignal.any([parent, timeout]) : timeout;
}

async function raceAbort<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) throw signal.reason;
  return new Promise<T>((resolve, reject) => {
    const onAbort = (): void => {
      reject(signal.reason instanceof Error ? signal.reason : new Error("Operation aborted"));
    };
    signal.addEventListener("abort", onAbort, { once: true });
    promise.then(resolve, reject).finally(() => {
      signal.removeEventListener("abort", onAbort);
    });
  });
}

async function resolveSecrets(
  value: unknown,
  maximumDepth: number,
  signal: AbortSignal,
  resolve: (reference: string, signal: AbortSignal) => Promise<string>,
  redactor: SecretRedactor,
  depth = 0,
): Promise<Readonly<Record<string, unknown>>> {
  const resolved = await resolveValue(value, maximumDepth, signal, resolve, redactor, depth);
  if (typeof resolved !== "object" || resolved === null || Array.isArray(resolved)) {
    throw new KernelRuntimeError("TOOL_ARGUMENTS_INVALID", "Tool arguments must be an object");
  }
  return resolved as Readonly<Record<string, unknown>>;
}

async function resolveValue(
  value: unknown,
  maximumDepth: number,
  signal: AbortSignal,
  resolve: (reference: string, signal: AbortSignal) => Promise<string>,
  redactor: SecretRedactor,
  depth: number,
): Promise<unknown> {
  if (depth > maximumDepth) {
    throw new KernelRuntimeError(
      "UNTRUSTED_VALUE_INVALID",
      "Value exceeds the allowed nesting depth",
    );
  }
  if (isSecretReference(value)) {
    const secret = await raceAbort(resolve(value.reference, signal), signal);
    redactor.add(secret);
    return secret;
  }
  if (Array.isArray(value)) {
    const output: unknown[] = [];
    for (const entry of value)
      output.push(await resolveValue(entry, maximumDepth, signal, resolve, redactor, depth + 1));
    return output;
  }
  if (typeof value === "object" && value !== null) {
    const output = Object.create(null) as Record<string, unknown>;
    for (const [key, entry] of Object.entries(value)) {
      output[key] = await resolveValue(entry, maximumDepth, signal, resolve, redactor, depth + 1);
    }
    return output;
  }
  return value;
}
