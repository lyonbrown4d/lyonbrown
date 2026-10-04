export interface CliMode {
  readonly kind: "chat" | "help" | "version";
}

export function parseCliArgs(args: readonly string[]): CliMode {
  if (args.length === 0) return { kind: "chat" };
  if (args.length === 1 && args[0] === "--help") return { kind: "help" };
  if (args.length === 1 && args[0] === "--version") return { kind: "version" };
  throw new Error(`unexpected argument: ${args[0] ?? ""}`);
}

export const HELP_TEXT = `Lyonbrown Agent

Usage: agent

Starts the authenticated terminal chat directly.
Options:
  --help       Show this help
  --version    Show the version`;

export const VERSION = "0.0.0";
