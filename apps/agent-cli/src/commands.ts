import { THEME_IDS, type Locale, type ThemeId } from "@lyonbrown/ui-model";

export type ParsedInput =
  | { readonly kind: "chat"; readonly text: string }
  | { readonly kind: "language"; readonly locale: Locale | null }
  | { readonly kind: "theme"; readonly themeId: ThemeId }
  | { readonly kind: "cancel" }
  | { readonly kind: "download" | "retry" }
  | { readonly kind: "steer" | "follow-up"; readonly text: string }
  | { readonly kind: "approve"; readonly scope: "once" | "session" }
  | { readonly kind: "deny"; readonly scope: "once" | "remaining" }
  | { readonly kind: "palette" | "activity" | "help" };

const REMOTE_TEXT = new Set(["steer", "follow-up"]);

export function parseInput(raw: string): ParsedInput {
  const text = raw.trim();
  if (!text.startsWith("/")) {
    if (text.length === 0) throw new Error("message cannot be empty");
    return { kind: "chat", text };
  }

  const [command = "", ...rest] = text.slice(1).split(/\s+/u);
  const argument = rest.join(" ").trim();
  if (REMOTE_TEXT.has(command)) {
    if (argument.length === 0) throw new Error(`/${command} requires text`);
    return { kind: command as "steer" | "follow-up", text: argument };
  }
  switch (command) {
    case "language":
      return { kind: "language", locale: parseLocale(argument) };
    case "theme":
      return { kind: "theme", themeId: parseTheme(argument) };
    case "cancel":
      return { kind: "cancel" };
    case "download":
    case "retry":
    case "activity":
    case "help":
      requireNoArgument(command, argument);
      return { kind: command };
    case "commands":
      requireNoArgument(command, argument);
      return { kind: "palette" };
    case "approve":
      return { kind: "approve", scope: parseChoice(argument, "once", "session") };
    case "deny":
      return { kind: "deny", scope: parseChoice(argument, "once", "remaining") };
    default:
      throw new Error(`unknown command: /${command}`);
  }
}

function parseLocale(value: string): Locale | null {
  if (value.length === 0) return null;
  if (value === "en" || value === "en-US") return "en-US";
  if (value === "zh" || value === "zh-CN") return "zh-CN";
  throw new Error("/language expects en or zh");
}

function parseTheme(value: string): ThemeId {
  if ((THEME_IDS as readonly string[]).includes(value)) return value as ThemeId;
  throw new Error(`/theme expects one of: ${THEME_IDS.join(", ")}`);
}

function parseChoice<A extends string, B extends string>(
  value: string,
  first: A,
  second: B,
): A | B {
  if (value === first || value === second) return value as A | B;
  throw new Error(`expected ${first} or ${second}`);
}

function requireNoArgument(command: string, argument: string): void {
  if (argument.length > 0) throw new Error(`/${command} does not accept arguments`);
}
