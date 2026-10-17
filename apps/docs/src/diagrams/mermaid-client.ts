import type { Mermaid } from "mermaid";
import {
  createDiagramView,
  type DiagramView,
} from "@lyonbrown/docs/internal/diagrams/diagram-view";

let mermaid: Promise<Mermaid> | undefined;
let queue = Promise.resolve();
let sequence = 0;
let initialized = false;

function theme(): "dark" | "default" {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "default";
}

function schedule(view: DiagramView): void {
  const revision = ++view.revision;
  const selectedTheme = theme();
  queue = queue
    .then(async () => {
      if (revision !== view.revision) return;
      mermaid ??= import("mermaid").then((module) => module.default);
      const renderer = await mermaid;
      const colors = getComputedStyle(document.documentElement);
      const color = (name: string) => colors.getPropertyValue(name).trim();
      renderer.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        suppressErrorRendering: true,
        theme: "base",
        themeVariables: {
          darkMode: selectedTheme === "dark",
          primaryColor: color("--sl-color-accent-low"),
          primaryTextColor: color("--sl-color-text"),
          primaryBorderColor: color("--sl-color-accent"),
          lineColor: color("--sl-color-gray-2"),
          secondaryColor: color("--sl-color-bg-nav"),
          tertiaryColor: color("--sl-color-bg"),
          clusterBkg: color("--sl-color-bg-nav"),
          clusterBorder: color("--sl-color-hairline"),
          edgeLabelBackground: color("--sl-color-bg"),
          fontSize: "14px",
        },
        fontFamily: getComputedStyle(document.body).fontFamily,
        flowchart: { htmlLabels: false, useMaxWidth: false },
        sequence: { useMaxWidth: false },
      });
      const rendered = await renderer.render(
        `lyonbrown-diagram-${String(++sequence)}`,
        view.source,
      );
      if (revision !== view.revision || selectedTheme !== theme()) return;
      view.output.innerHTML = rendered.svg;
      const svg = view.output.querySelector("svg");
      if (!svg) throw new Error("Diagram SVG is missing");
      view.update(svg);
    })
    .catch(() => {
      if (revision !== view.revision) return;
      view.status.hidden = false;
      view.status.textContent = document.documentElement.lang.startsWith("zh")
        ? "图表暂时无法显示，可展开源码查看。"
        : "The diagram could not be displayed. Its source is available below.";
    });
}

export function initializeDiagrams(): void {
  if (initialized) return;
  initialized = true;
  const views = [...document.querySelectorAll<HTMLElement>(".mermaid-diagram")]
    .map(createDiagramView)
    .filter((view): view is DiagramView => view !== undefined);
  if (views.length === 0) return;
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        const view = views.find((candidate) => candidate.figure === entry.target);
        if (!entry.isIntersecting || !view || view.visible) continue;
        view.visible = true;
        schedule(view);
      }
    },
    { rootMargin: "200px" },
  );
  for (const view of views) observer.observe(view.figure);
  new MutationObserver(() => {
    for (const view of views) if (view.visible) schedule(view);
  }).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
}
