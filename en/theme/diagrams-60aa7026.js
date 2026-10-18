/* Browser-only bridge; diagram layout and image interaction belong to vendor libraries. */
(() => {
    const chinese = document.documentElement.lang.startsWith("zh");
    const labels = chinese
        ? { open: "查看大图 · 滚轮缩放，拖拽移动，Esc 关闭", source: "图表源码", download: "下载 SVG", error: "图表无法显示，可展开源码查看。", close: "关闭图表", zoom: "切换缩放" }
        : { open: "Explore diagram · scroll to zoom, drag to pan, Esc to close", source: "Diagram source", download: "Download SVG", error: "Diagram unavailable. Expand its source below.", close: "Close diagram", zoom: "Toggle zoom" };
    const figures = [...document.querySelectorAll("code.language-mermaid")].map((code, index) => {
        const source = code.textContent;
        const pre = code.closest("pre");
        const figure = document.createElement("figure");
        const button = document.createElement("button");
        button.type = "button";
        button.className = "diagram-preview";
        button.setAttribute("aria-label", labels.open);
        button.setAttribute("aria-haspopup", "dialog");
        button.disabled = true;
        const details = document.createElement("details");
        const summary = document.createElement("summary");
        summary.textContent = labels.source;
        const download = document.createElement("a");
        download.textContent = labels.download;
        download.download = `lyonbrown-diagram-${index + 1}.svg`;
        download.hidden = true;
        const footer = document.createElement("figcaption");
        footer.className = "diagram-footer";
        const status = document.createElement("p");
        status.className = "diagram-status";
        status.setAttribute("role", "status");
        pre.replaceWith(figure);
        details.append(summary, pre);
        footer.append(details, download);
        figure.append(button, footer, status);
        const view = { source, button, download, status, url: null, width: 0, height: 0 };
        button.addEventListener("click", () => {
            if (!view.url || !window.PhotoSwipe) return;
            const viewer = new PhotoSwipe({
                dataSource: [{ src: view.url, width: view.width, height: view.height, alt: labels.open }],
                mainClass: "diagram-lightbox", wheelToZoom: true, bgOpacity: 1,
                initialZoomLevel: "fit", secondaryZoomLevel: 1, maxZoomLevel: 6,
                counter: false, arrowPrev: false, arrowNext: false,
                closeOnVerticalDrag: false, pinchToClose: false,
                closeTitle: labels.close, zoomTitle: labels.zoom,
                showHideAnimationType: "none", zoomAnimationDuration: 0,
            });
            viewer.on("uiRegister", () => viewer.ui.registerElement({
                name: "diagram-download", order: 8, isButton: true, tagName: "a",
                title: labels.download, html: "↓",
                onInit(element) { element.href = view.url; element.download = download.download; },
            }));
            viewer.init();
        });
        return view;
    });
    if (!figures.length) return;
    let revision = 0;
    let queue = Promise.resolve();
    function render() {
        const requested = ++revision;
        queue = queue.then(async () => {
            if (requested !== revision) return;
            const style = getComputedStyle(document.documentElement);
            const background = style.getPropertyValue("--bg").trim();
            const foreground = style.getPropertyValue("--fg").trim();
            try {
                mermaid.initialize({
                    startOnLoad: false, securityLevel: "strict", suppressErrorRendering: true,
                    theme: "base", fontFamily: getComputedStyle(document.body).fontFamily,
                    themeVariables: {
                        primaryColor: background, primaryTextColor: foreground,
                        primaryBorderColor: foreground, lineColor: foreground,
                        secondaryColor: background, tertiaryColor: background,
                        clusterBkg: background, clusterBorder: foreground,
                        edgeLabelBackground: background,
                    },
                    flowchart: { htmlLabels: false, useMaxWidth: false },
                    sequence: { useMaxWidth: false },
                });
                for (const [index, view] of figures.entries()) {
                    const rendered = await mermaid.render(`book-diagram-${requested}-${index}`, view.source);
                    if (requested !== revision) return;
                    view.button.innerHTML = rendered.svg;
                    const svg = view.button.querySelector("svg");
                    const bounds = svg.viewBox.baseVal;
                    view.width = bounds.width || 1000;
                    view.height = bounds.height || 700;
                    svg.style.background = background;
                    if (view.url) URL.revokeObjectURL(view.url);
                    view.url = URL.createObjectURL(new Blob([new XMLSerializer().serializeToString(svg)], { type: "image/svg+xml" }));
                    view.download.href = view.url;
                    view.download.hidden = false;
                    view.button.disabled = false;
                    view.status.textContent = labels.open;
                }
            } catch {
                for (const view of figures) view.status.textContent = labels.error;
            }
        });
    }
    new MutationObserver(render).observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    render();
})();
