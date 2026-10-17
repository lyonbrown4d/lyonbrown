import { existsSync, globSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { load, type CheerioAPI } from "cheerio";
import { describe, expect, it } from "vitest";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, "dist");
const content = join(root, "src/content/docs");
const markdown = [...globSync("**/*.md", { cwd: content })];
const html = [...globSync("**/*.html", { cwd: output })];
const pages = new Map<string, CheerioAPI>(
  html.map((file) => [join(output, file), load(readFileSync(join(output, file), "utf8"))]),
);

function pagePath(path: string): string {
  const target = join(output, decodeURIComponent(path));
  return existsSync(target) && statSync(target).isDirectory() ? join(target, "index.html") : target;
}

function route(file: string): string {
  return `/${file.replaceAll("\\", "/").replace(/index\.html$/u, "")}`;
}

describe("built bilingual documentation", () => {
  it("publishes every Markdown document once with a single page title", () => {
    expect(markdown.length).toBeGreaterThanOrEqual(22);
    expect(html.length).toBeGreaterThanOrEqual(markdown.length + 1);
    for (const file of markdown) {
      const path = file.replace(/(?:\/index)?\.md$/u, "");
      const page = pages.get(pagePath(`/${path}/`));
      expect(page, file).toBeDefined();
      expect(page?.("h1").length, file).toBe(1);
      expect(page?.("html").attr("lang"), file).toBe(file.startsWith("zh/") ? "zh-CN" : "en");
    }
    const home = pages.get(join(output, "index.html"));
    expect(home?.("h1").length).toBe(1);
    expect(home?.('a[href="/zh/"]').length).toBeGreaterThan(0);
    expect(home?.('a[href="/en/"]').length).toBeGreaterThan(0);
  });

  it("resolves every local page, anchor, image, script, and stylesheet", () => {
    const failures: string[] = [];
    for (const [file, page] of pages) {
      const from = new URL(route(file.slice(output.length + 1)), "https://docs.invalid");
      for (const element of page("a[href], img[src], script[src], link[href]").toArray()) {
        const value = page(element).attr("href") ?? page(element).attr("src");
        if (!value) continue;
        const target = new URL(value, from);
        if (target.origin !== from.origin) continue;
        const path = pagePath(target.pathname);
        if (!existsSync(path)) {
          failures.push(`${from.pathname}: ${value}`);
          continue;
        }
        if (target.hash && path.endsWith(".html")) {
          const destination = pages.get(path);
          const id = decodeURIComponent(target.hash.slice(1));
          if (
            !destination?.("[id]")
              .toArray()
              .some((node) => destination(node).attr("id") === id)
          )
            failures.push(`${from.pathname}: ${value}`);
        }
      }
    }
    expect(failures).toEqual([]);
  });

  it("retains Mermaid sources and builds local search indexes for both languages", () => {
    const sources = markdown.reduce(
      (count, file) =>
        count +
        [...readFileSync(join(content, file), "utf8").matchAll(/^```mermaid\s*$/gmu)].length,
      0,
    );
    const rendered = [...pages.values()].reduce(
      (count, page) => count + page("figure.mermaid-diagram pre.mermaid-source").length,
      0,
    );
    expect(sources).toBeGreaterThan(0);
    expect(rendered).toBe(sources);
    expect(existsSync(join(output, "pagefind/pagefind.js"))).toBe(true);
    expect(
      [...globSync("pagefind/*.{pf_index,pf_meta}", { cwd: output })].length,
    ).toBeGreaterThanOrEqual(2);
    for (const locale of ["zh", "en"]) {
      const page = pages.get(join(output, locale, "index.html"));
      expect(page?.("starlight-theme-select").length).toBeGreaterThan(0);
      expect(page?.("starlight-lang-select").length).toBeGreaterThan(0);
    }
  });
});
