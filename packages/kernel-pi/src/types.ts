export type KernelJsonPrimitive = null | boolean | number | string;
export type KernelJsonValue =
  | KernelJsonPrimitive
  | readonly KernelJsonValue[]
  | { readonly [key: string]: KernelJsonValue };

export interface KernelModelDescriptor {
  readonly provider: string;
  readonly id: string;
}

export type KernelModelMessage =
  | { readonly role: "system" | "user"; readonly content: string }
  | {
      readonly role: "assistant";
      readonly content: string;
      readonly toolCalls: readonly {
        readonly callId: string;
        readonly name: string;
        readonly arguments: Readonly<Record<string, unknown>>;
      }[];
    }
  | {
      readonly role: "tool_result";
      readonly callId: string;
      readonly name: string;
      readonly content: string;
      readonly isError: boolean;
    };

export interface KernelModelTool {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
}

export interface KernelModelRequest {
  readonly sessionId: string;
  readonly model: KernelModelDescriptor;
  readonly messages: readonly KernelModelMessage[];
  readonly tools: readonly KernelModelTool[];
  readonly signal: AbortSignal;
  readonly maxOutputBytes: number;
}

export interface KernelUsage {
  readonly inputTokens: number;
  readonly outputTokens: number;
  readonly totalTokens: number;
}

export type KernelModelStreamEvent =
  | { readonly type: "text_delta"; readonly delta: string }
  | {
      readonly type: "tool_call";
      readonly callId: string;
      readonly name: string;
      readonly arguments: Readonly<Record<string, unknown>>;
    }
  | { readonly type: "usage"; readonly usage: KernelUsage }
  | { readonly type: "done"; readonly reason: "stop" | "length" | "tool_use" }
  | {
      readonly type: "error";
      readonly code: "aborted" | "model_error";
      readonly message: string;
    };

export interface KernelModelAdapter {
  readonly model: KernelModelDescriptor;
  stream(request: KernelModelRequest): AsyncIterable<KernelModelStreamEvent>;
}

export type KernelToolRisk = "low" | "medium" | "high";

export interface KernelSecretRef {
  readonly kind: "secretRef";
  readonly reference: string;
}

export interface KernelToolResult {
  readonly content: string;
  readonly details?: KernelJsonValue;
}

export interface KernelToolExecutionContext {
  readonly callId: string;
  readonly arguments: Readonly<Record<string, unknown>>;
  readonly signal: AbortSignal;
}

export interface KernelToolDefinition {
  readonly name: string;
  readonly label: string;
  readonly description: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
  readonly permissions: readonly string[];
  readonly risk: KernelToolRisk;
  execute(context: KernelToolExecutionContext): Promise<KernelToolResult>;
}

export interface KernelToolRegistry {
  list(): readonly KernelToolDefinition[];
  get(name: string): KernelToolDefinition | undefined;
  hasPermission(permission: string): boolean;
}

export interface KernelApprovalRequest {
  readonly sessionId: string;
  readonly callId: string;
  readonly toolName: string;
  readonly risk: Exclude<KernelToolRisk, "low">;
  readonly permissions: readonly string[];
  readonly argumentsPreview: KernelJsonValue;
  readonly signal: AbortSignal;
}

export interface KernelApprovalGate {
  request(request: KernelApprovalRequest): Promise<{ readonly decision: "approve" | "deny" }>;
}

export interface KernelSecretResolver {
  resolve(reference: string, signal: AbortSignal): Promise<string>;
}

export interface KernelResourceBudget {
  readonly maxInputBytes: number;
  readonly maxTurns: number;
  readonly maxSteps: number;
  readonly maxOutputBytes: number;
  readonly maxToolResultBytes: number;
  readonly maxValueDepth: number;
  readonly toolTimeoutMs: number;
}

export type KernelEvent =
  | { readonly type: "run_started"; readonly sessionId: string }
  | { readonly type: "run_completed"; readonly sessionId: string }
  | {
      readonly type: "run_failed";
      readonly sessionId: string;
      readonly code: string;
      readonly message: string;
    }
  | { readonly type: "run_cancelled"; readonly sessionId: string }
  | { readonly type: "turn_started"; readonly turn: number }
  | { readonly type: "turn_completed"; readonly turn: number }
  | { readonly type: "text_delta"; readonly delta: string }
  | { readonly type: "usage"; readonly usage: KernelUsage }
  | {
      readonly type: "tool_started";
      readonly callId: string;
      readonly toolName: string;
      readonly argumentsPreview: KernelJsonValue;
    }
  | {
      readonly type: "tool_succeeded";
      readonly callId: string;
      readonly toolName: string;
      readonly content: string;
      readonly details?: KernelJsonValue;
    }
  | {
      readonly type: "tool_failed";
      readonly callId: string;
      readonly toolName: string;
      readonly code: string;
      readonly message: string;
    }
  | {
      readonly type: "runtime_error";
      readonly code: string;
      readonly message: string;
    };

export interface KernelEventSink {
  append(event: KernelEvent): Promise<void>;
}

export type KernelEventListener = (event: KernelEvent) => Promise<void> | void;

export interface KernelSession {
  startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void>;
  steer(input: string): void;
  followUp(input: string): void;
  cancel(): void;
  subscribe(listener: KernelEventListener): () => void;
}

export interface PiKernelSessionOptions {
  readonly sessionId: string;
  readonly systemPrompt: string;
  readonly model: KernelModelAdapter;
  readonly toolRegistry: KernelToolRegistry;
  readonly approvalGate: KernelApprovalGate;
  readonly eventSink: KernelEventSink;
  readonly secretResolver: KernelSecretResolver;
  readonly budget: KernelResourceBudget;
}
