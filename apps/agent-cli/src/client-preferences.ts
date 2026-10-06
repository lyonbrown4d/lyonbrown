import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { THEME_IDS, type Locale, type ThemeId } from "@lyonbrown/ui-model";

export interface ClientPreferences {
  readonly locale: Locale;
  readonly themeId: ThemeId;
}

export interface ClientPreferenceStore {
  load(): Promise<ClientPreferences | null>;
  save(preferences: ClientPreferences): Promise<void>;
}

export function detectClientLocale(locale: string | undefined): Locale {
  if (locale === undefined) return "en-US";
  const normalized = locale.replace("_", "-").toLocaleLowerCase();
  return normalized === "zh" || normalized.startsWith("zh-cn") || normalized.startsWith("zh-hans")
    ? "zh-CN"
    : "en-US";
}

const MAX_PREFERENCE_BYTES = 8_192;

export function createJsonPreferenceStore(path: string): ClientPreferenceStore {
  return {
    load: async () => loadPreferences(path),
    save: async (preferences) => {
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, `${JSON.stringify(preferences, null, 2)}\n`, {
        encoding: "utf8",
        mode: 0o600,
      });
    },
  };
}

async function loadPreferences(path: string): Promise<ClientPreferences | null> {
  try {
    const metadata = await stat(path);
    if (!metadata.isFile() || metadata.size > MAX_PREFERENCE_BYTES) return null;
    const parsed: unknown = JSON.parse(await readFile(path, "utf8"));
    if (!isRecord(parsed)) return null;
    const locale = parsed.locale;
    const themeId = parsed.themeId;
    if (!isLocale(locale) || !isThemeId(themeId)) return null;
    return { locale, themeId };
  } catch (error: unknown) {
    if (isMissingFile(error) || error instanceof SyntaxError) return null;
    throw error;
  }
}

function isRecord(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isLocale(value: unknown): value is Locale {
  return value === "en-US" || value === "zh-CN";
}

function isThemeId(value: unknown): value is ThemeId {
  return typeof value === "string" && (THEME_IDS as readonly string[]).includes(value);
}

function isMissingFile(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { readonly code?: unknown }).code === "ENOENT"
  );
}
