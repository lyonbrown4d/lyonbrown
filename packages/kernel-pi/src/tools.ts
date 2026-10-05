import type { AgentTool, AgentToolResult } from "@earendil-works/pi-agent-core";
import type { JsonValue } from "@earendil-works/pi-ai";
import type { TSchema } from "typebox";
import { Value } from "typebox/value";

import type { BudgetTracker } from "./budget.ts";
import { copyInternalKernelError, kernelError, type KernelRuntimeError } from "./errors.ts";
import type { SecretRedactor } from "./redaction.ts";
import {
  assertByteLimit,
  limitsFromBudget,
  redactSecretReferences,
  sanitizeJson,
  sanitizeText,
  type SanitizationLimits,
} from "./sanitization.ts";
import {
  abortCode,
  createDeadlineSignal,
  hasAborted,
  isJsonObject,
  isKnownRisk,
  raceAbort,
  replaceSecretRefsForValidation,
  resolveSecrets,
  toExternalToolError,
  toSafePreExecutionError,
  toSafeToolError,
} from "./tool-support.ts";
import type {
  KernelEvent,
  KernelJsonValue,
  KernelToolDefinition,
  PiKernelSessionOptions,
} from "./types.ts";

type EmitEvent = (event: KernelEvent) => Promise<void>;

export class ToolExecutor {
  readonly #limits: SanitizationLimits;
  readonly #resultLimits: SanitizationLimits;

  constructor(
    private readonly options: PiKernelSessionOptions,
    private readonly tracker: () => BudgetTracker,
    private readonly redactor: SecretRedactor,
    private readonly emit: EmitEvent,
    private readonly markFatal: (error: KernelRuntimeError) => void,
  ) {
    this.#limits = limitsFromBudget(options.budget);
    this.#resultLimits = limitsFromBudget(
      options.budget,
      options.budget.maxToolResultBytes,
      "TOOL_RESULT_BUDGET_EXCEEDED",
    );
  }

  createAgentTool(tool: KernelToolDefinition): AgentTool {
    return {
      name: tool.name,
      label: tool.label,
      description: tool.description,
      // Pi only validates structure; Lyonbrown validates symbolic SecretRefs.
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
    const signal = createDeadlineSignal(parentSignal, this.options.budget.toolTimeoutMs);
    let symbolic: Readonly<Record<string, KernelJsonValue>>;
    let persistingIntent = false;
    try {
      this.tracker().step();
      symbolic = this.assertPolicy(tool, arguments_);
      const preview = redactSecretReferences(symbolic, this.#limits);
      await this.requestApprovalIfNeeded(tool, safeCallId, preview, signal);
      signal.throwIfAborted();
      persistingIntent = true;
      await this.emit({
        type: "tool_started",
        callId: safeCallId,
        toolName: safeToolName,
        argumentsPreview: preview,
      });
    } catch (error: unknown) {
      if (persistingIntent) throw error;
      const safe = toSafePreExecutionError(error, this.redactor);
      await this.emitFailure(safeCallId, safeToolName, safe);
      throw safe;
    }

    try {
      if (hasAborted(signal)) return await this.failUnknown(safeCallId, safeToolName, signal);
      let resolved: Readonly<Record<string, unknown>>;
      try {
        resolved = await resolveSecrets(
          symbolic,
          signal,
          this.options.secretResolver.resolve.bind(this.options.secretResolver),
          this.redactor,
          this.#limits,
        );
      } catch (error: unknown) {
        if (hasAborted(signal)) return await this.failUnknown(safeCallId, safeToolName, signal);
        const safe = toSafeToolError(error, this.redactor);
        await this.emitFailure(safeCallId, safeToolName, safe);
        throw safe;
      }
      signal.throwIfAborted();

      let execution: Promise<Awaited<ReturnType<KernelToolDefinition["execute"]>>>;
      try {
        // This check must happen before evaluating the effectful expression.
        signal.throwIfAborted();
        execution = tool.execute({ callId: safeCallId, arguments: resolved, signal });
      } catch {
        if (hasAborted(signal)) return await this.failUnknown(safeCallId, safeToolName, signal);
        const safe = toExternalToolError();
        await this.emitFailure(safeCallId, safeToolName, safe);
        throw safe;
      }

      let result: Awaited<ReturnType<KernelToolDefinition["execute"]>>;
      try {
        result = await raceAbort(execution, signal);
      } catch {
        if (hasAborted(signal)) return await this.failUnknown(safeCallId, safeToolName, signal);
        const safe = toExternalToolError();
        await this.emitFailure(safeCallId, safeToolName, safe);
        throw safe;
      }
      if (hasAborted(signal)) return await this.failUnknown(safeCallId, safeToolName, signal);

      let content: string;
      let details: KernelJsonValue | undefined;
      try {
        const normalizedResult = this.redactor.json(result, this.#resultLimits);
        if (!isJsonObject(normalizedResult) || typeof normalizedResult.content !== "string") {
          throw kernelError("UNTRUSTED_VALUE_INVALID", "Tool result shape is invalid");
        }
        content = normalizedResult.content;
        details = normalizedResult.details;
        assertByteLimit(
          JSON.stringify({ content, details }),
          this.options.budget.maxToolResultBytes,
          "TOOL_RESULT_BUDGET_EXCEEDED",
        );
      } catch (error: unknown) {
        return await this.failUnknown(
          safeCallId,
          safeToolName,
          signal,
          copyInternalKernelError(error)?.code ?? "TOOL_RESULT_INVALID",
        );
      }
      if (hasAborted(signal)) return await this.failUnknown(safeCallId, safeToolName, signal);

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
      const internal = copyInternalKernelError(error);
      if (internal?.code === "TOOL_OUTCOME_UNKNOWN") {
        this.markFatal(internal);
      }
      throw error;
    }
  }

  private assertPolicy(
    tool: KernelToolDefinition,
    arguments_: Readonly<Record<string, unknown>>,
  ): Readonly<Record<string, KernelJsonValue>> {
    if (!isKnownRisk(tool.risk)) {
      throw kernelError("TOOL_DENIED", "Tool has no recognized risk classification");
    }
    if (
      !tool.permissions.every((permission) => this.options.toolRegistry.hasPermission(permission))
    ) {
      throw kernelError("TOOL_DENIED", "Required tool permission is not granted");
    }
    const sanitized = sanitizeJson(arguments_, this.#limits);
    if (!isJsonObject(sanitized)) {
      throw kernelError("TOOL_ARGUMENTS_INVALID", "Tool arguments must be an object");
    }
    if (
      !Value.Check(
        tool.inputSchema as TSchema,
        replaceSecretRefsForValidation(sanitized, this.options.budget.maxCollectionEntries),
      )
    ) {
      throw kernelError(
        "TOOL_ARGUMENTS_INVALID",
        "Tool arguments do not match the declared schema",
      );
    }
    assertByteLimit(
      JSON.stringify(sanitized),
      this.options.budget.maxInputBytes,
      "INPUT_BUDGET_EXCEEDED",
    );
    return sanitized;
  }

  private async requestApprovalIfNeeded(
    tool: KernelToolDefinition,
    callId: string,
    preview: KernelJsonValue,
    signal: AbortSignal,
  ): Promise<void> {
    if (tool.risk === "low") return;
    try {
      signal.throwIfAborted();
      const pending = this.options.approvalGate.request({
        sessionId: this.options.sessionId,
        callId,
        toolName: sanitizeText(tool.name),
        risk: tool.risk,
        permissions: tool.permissions.map(sanitizeText),
        argumentsPreview: preview,
        signal,
      });
      const response = await raceAbort(pending, signal);
      signal.throwIfAborted();
      const normalizedResponse = sanitizeJson(response, this.#limits);
      if (!isJsonObject(normalizedResponse) || normalizedResponse.decision !== "approve") {
        throw kernelError("TOOL_DENIED", "Tool approval denied");
      }
    } catch {
      throw kernelError("TOOL_DENIED", "Tool approval unavailable");
    }
  }

  private async emitFailure(
    callId: string,
    toolName: string,
    error: KernelRuntimeError,
  ): Promise<void> {
    await this.emit({
      type: "tool_failed",
      callId,
      toolName,
      code: error.code,
      message: error.message,
    });
  }

  private async failUnknown(
    callId: string,
    toolName: string,
    signal: AbortSignal,
    code = abortCode(signal),
  ): Promise<never> {
    const fatal = kernelError(
      "TOOL_OUTCOME_UNKNOWN",
      "Tool outcome cannot be confirmed after durable intent",
    );
    await this.emit({
      type: "tool_outcome_unknown",
      callId,
      toolName,
      code,
      message: fatal.message,
    });
    this.markFatal(fatal);
    throw fatal;
  }
}
