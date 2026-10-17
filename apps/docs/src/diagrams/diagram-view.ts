import type PhotoSwipe from "photoswipe";

export interface DiagramView {
  readonly figure: HTMLElement;
  readonly source: string;
  readonly output: HTMLButtonElement;
  readonly status: HTMLParagraphElement;
  visible: boolean;
  revision: number;
  update(svg: SVGSVGElement): void;
}

export function createDiagramView(figure: HTMLElement, index: number): DiagramView | undefined {
  const source = figure.querySelector("pre");
  if (!source?.textContent) return undefined;
  const chinese = document.documentElement.lang.startsWith("zh");
  const labels = chinese
    ? {
        open: "查看大图",
        close: "关闭图表",
        zoom: "切换缩放",
        download: "下载 SVG",
        source: "图表源码",
        hint: "点击查看大图",
        gesture: "滚轮缩放 · 拖拽移动 · Esc 关闭",
        loading: "图表加载中…",
        error: "图表暂时无法显示，可展开源码查看。",
      }
    : {
        open: "View diagram",
        close: "Close diagram",
        zoom: "Toggle zoom",
        download: "Download SVG",
        source: "Diagram source",
        hint: "Click to explore",
        gesture: "Scroll to zoom · Drag to pan · Esc to close",
        loading: "Loading diagram…",
        error: "The diagram could not be displayed. Its source is available below.",
      };
  const output = document.createElement("button");
  output.type = "button";
  output.className = "diagram-preview";
  output.setAttribute("aria-label", labels.open);
  output.setAttribute("aria-haspopup", "dialog");
  output.disabled = true;
  const hint = document.createElement("span");
  hint.className = "diagram-open-hint";
  hint.textContent = "⤢ " + labels.hint;
  const details = document.createElement("details");
  const summary = document.createElement("summary");
  summary.textContent = labels.source;
  details.append(summary, source);
  const status = document.createElement("p");
  status.className = "diagram-status";
  status.setAttribute("role", "status");
  status.textContent = labels.loading;
  const download = document.createElement("a");
  download.textContent = labels.download;
  download.download = "lyonbrown-diagram-" + String(index + 1) + ".svg";
  download.hidden = true;
  const footer = document.createElement("figcaption");
  footer.className = "diagram-footer";
  footer.append(details, download);
  figure.replaceChildren(output, footer, status);
  let viewer: PhotoSwipe | undefined;
  let opening = false;
  let downloadUrl: string | undefined;
  let width = 0;
  let height = 0;
  output.addEventListener("click", () => {
    if (opening || viewer || !downloadUrl) return;
    opening = true;
    void import("photoswipe")
      .then(({ default: Viewer }) => {
        const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
        const instance = new Viewer({
          dataSource: [{ src: downloadUrl, width, height, alt: labels.open }],
          index: 0,
          mainClass: "diagram-lightbox",
          bgOpacity: 1,
          wheelToZoom: true,
          initialZoomLevel: "fit",
          secondaryZoomLevel: 1,
          maxZoomLevel: 4,
          padding: { top: 64, bottom: 48, left: 24, right: 24 },
          closeOnVerticalDrag: false,
          pinchToClose: false,
          counter: false,
          arrowPrev: false,
          arrowNext: false,
          closeTitle: labels.close,
          zoomTitle: labels.zoom,
          errorMsg: labels.error,
          showHideAnimationType: "fade",
          showAnimationDuration: reducedMotion ? 0 : 180,
          hideAnimationDuration: reducedMotion ? 0 : 180,
          zoomAnimationDuration: reducedMotion ? 0 : 180,
        });
        viewer = instance;
        instance.on("firstUpdate", () => {
          instance.element?.setAttribute("aria-label", labels.open);
          instance.element?.setAttribute("aria-modal", "true");
        });
        instance.on("uiRegister", () => {
          instance.ui?.registerElement({
            name: "diagram-download",
            order: 8,
            isButton: true,
            tagName: "a",
            title: labels.download,
            html: '<svg aria-hidden="true" class="pswp__icn" viewBox="0 0 32 32"><path d="M15 5h2v13l5-5 1.5 1.5L16 22l-7.5-7.5L10 13l5 5V5ZM7 24h18v3H7z"/></svg>',
            onInit(element) {
              if (element instanceof HTMLAnchorElement && downloadUrl) {
                element.href = downloadUrl;
                element.download = download.download;
              }
            },
          });
          instance.ui?.registerElement({
            name: "diagram-help",
            appendTo: "wrapper",
            onInit(element) {
              element.textContent = labels.gesture;
            },
          });
        });
        instance.on("destroy", () => {
          viewer = undefined;
          output.focus({ preventScroll: true });
        });
        instance.init();
      })
      .catch(() => {
        status.hidden = false;
        status.textContent = labels.error;
      })
      .finally(() => {
        opening = false;
      });
  });
  return {
    figure,
    source: source.textContent,
    output,
    status,
    visible: false,
    revision: 0,
    update(svg) {
      viewer?.close();
      width = svg.viewBox.baseVal.width;
      height = svg.viewBox.baseVal.height;
      svg.setAttribute("aria-hidden", "true");
      svg.style.width = "100%";
      svg.style.height = "auto";
      output.replaceChildren(svg, hint);
      output.disabled = false;
      if (downloadUrl) URL.revokeObjectURL(downloadUrl);
      const exported = svg.cloneNode(true) as SVGSVGElement;
      exported.style.width = "";
      exported.style.height = "";
      exported.setAttribute("width", String(width));
      exported.setAttribute("height", String(height));
      const background = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      background.setAttribute("x", String(svg.viewBox.baseVal.x));
      background.setAttribute("y", String(svg.viewBox.baseVal.y));
      background.setAttribute("width", String(width));
      background.setAttribute("height", String(height));
      background.setAttribute("fill", getComputedStyle(figure).backgroundColor);
      exported.prepend(background);
      downloadUrl = URL.createObjectURL(
        new Blob([new XMLSerializer().serializeToString(exported)], { type: "image/svg+xml" }),
      );
      download.href = downloadUrl;
      download.hidden = false;
      status.hidden = true;
    },
  };
}
