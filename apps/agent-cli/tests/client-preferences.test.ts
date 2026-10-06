import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import {
  createJsonPreferenceStore,
  detectClientLocale,
  type ClientPreferences,
} from "@lyonbrown/agent-cli/internal/client-preferences";

const roots: string[] = [];

afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("TUI client preferences", () => {
  it.each([
    ["zh-CN", "zh-CN"],
    ["zh_CN", "zh-CN"],
    ["zh-Hans-SG", "zh-CN"],
    ["en-GB", "en-US"],
    [undefined, "en-US"],
  ] as const)("maps system locale %s to %s", (input, expected) => {
    expect(detectClientLocale(input)).toBe(expected);
  });

  it("round-trips only the typed locale and theme fields", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-tui-preferences-"));
    roots.push(root);
    const path = join(root, "client.json");
    const store = createJsonPreferenceStore(path);
    const preferences: ClientPreferences = { locale: "zh-CN", themeId: "graphite-coral" };

    await store.save(preferences);

    await expect(store.load()).resolves.toEqual(preferences);
    expect(JSON.parse(await readFile(path, "utf8"))).toEqual(preferences);
  });

  it("rejects malformed, unknown and oversized preference files without merging keys", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-tui-preferences-"));
    roots.push(root);
    const path = join(root, "client.json");
    const store = createJsonPreferenceStore(path);

    await writeFile(path, '{"locale":"zh-CN","themeId":"__proto__","admin":true}', "utf8");
    await expect(store.load()).resolves.toBeNull();
    await writeFile(path, "x".repeat(8_193), "utf8");
    await expect(store.load()).resolves.toBeNull();
  });
});
