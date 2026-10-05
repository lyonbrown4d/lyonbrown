const OPTION_TO_FIELD = Object.freeze({
  "--workspace": "workspace",
  "--effective-config-fingerprint": "effectiveConfigFingerprint",
  "--scope-id": "scopeId",
} as const);

type AgentdOption = keyof typeof OPTION_TO_FIELD;
type AgentdOptionField = (typeof OPTION_TO_FIELD)[AgentdOption];

export interface AgentdCliOptions {
  readonly workspace: string;
  readonly effectiveConfigFingerprint: string;
  readonly scopeId: string;
}

export type AgentdCliErrorCode =
  | "CLI_ARGUMENT_MISSING"
  | "CLI_ARGUMENT_MISSING_VALUE"
  | "CLI_ARGUMENT_DUPLICATE"
  | "CLI_ARGUMENT_UNKNOWN"
  | "CLI_ARGUMENT_INVALID";

export class AgentdCliError extends Error {
  public override readonly name = "AgentdCliError";

  public constructor(public readonly code: AgentdCliErrorCode) {
    super("agentd command-line arguments are invalid");
  }
}

export function parseAgentdArguments(arguments_: readonly string[]): AgentdCliOptions {
  const values: Partial<Record<AgentdOptionField, string>> = {};
  for (let index = 0; index < arguments_.length; index += 2) {
    const option = arguments_[index];
    if (!isAgentdOption(option)) throw new AgentdCliError("CLI_ARGUMENT_UNKNOWN");
    const field = OPTION_TO_FIELD[option];
    if (values[field] !== undefined) throw new AgentdCliError("CLI_ARGUMENT_DUPLICATE");
    const value = arguments_[index + 1];
    if (value === undefined || value.startsWith("--")) {
      throw new AgentdCliError("CLI_ARGUMENT_MISSING_VALUE");
    }
    values[field] = requireValue(value);
  }
  if (
    values.workspace === undefined ||
    values.effectiveConfigFingerprint === undefined ||
    values.scopeId === undefined
  ) {
    throw new AgentdCliError("CLI_ARGUMENT_MISSING");
  }
  if (!/^[a-f0-9]{64}$/u.test(values.scopeId)) {
    throw new AgentdCliError("CLI_ARGUMENT_INVALID");
  }
  return Object.freeze({
    workspace: values.workspace,
    effectiveConfigFingerprint: values.effectiveConfigFingerprint,
    scopeId: values.scopeId,
  });
}

function isAgentdOption(value: string | undefined): value is AgentdOption {
  return value !== undefined && Object.hasOwn(OPTION_TO_FIELD, value);
}

function requireValue(value: string): string {
  if (value.trim().length === 0 || value.includes("\0")) {
    throw new AgentdCliError("CLI_ARGUMENT_INVALID");
  }
  return value;
}
