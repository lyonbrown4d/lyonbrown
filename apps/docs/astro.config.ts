import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import type { Root } from "mdast";
import { visit } from "unist-util-visit";
import { unified } from "@astrojs/markdown-remark";

function remarkMermaid() {
  return (tree: Root) => {
    visit(tree, "code", (node, index, parent) => {
      if (node.lang !== "mermaid" || index === undefined || !parent) return;
      const source = node.value
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;");
      parent.children[index] = {
        type: "html",
        value: `<figure class="mermaid-diagram" data-pagefind-ignore><pre class="mermaid-source">${source}</pre></figure>`,
      };
    });
  };
}

export default defineConfig({
  output: "static",
  trailingSlash: "always",
  markdown: { processor: unified({ remarkPlugins: [remarkMermaid] }) },
  integrations: [
    starlight({
      title: "Lyonbrown",
      description: "Local-first agent runtime architecture, design, and roadmap.",
      disable404Route: true,
      defaultLocale: "en",
      locales: {
        en: { label: "English", lang: "en" },
        zh: { label: "简体中文", lang: "zh-CN" },
      },
      sidebar: [
        { slug: "index", label: "Introduction", translations: { "zh-CN": "项目文档" } },
        {
          label: "Architecture",
          translations: { "zh-CN": "架构" },
          items: [
            { slug: "architecture/overview" },
            { slug: "architecture/grpc-client-server" },
            { slug: "architecture/composition-root" },
            { slug: "architecture/electron-desktop" },
            { slug: "architecture/system-one-decision-layer" },
          ],
        },
        {
          label: "Design",
          translations: { "zh-CN": "设计" },
          items: [
            { slug: "design/tui-interaction-design" },
            { slug: "design/tui-black-box-validation" },
          ],
        },
        {
          label: "Plans",
          translations: { "zh-CN": "计划" },
          items: [{ slug: "plans/roadmap" }],
        },
        {
          label: "Reference",
          translations: { "zh-CN": "参考" },
          items: [
            { slug: "reference/configuration" },
            { slug: "reference/local-models-routing" },
            { slug: "reference/release-artifacts" },
          ],
        },
      ],
      components: { MarkdownContent: "./src/components/MarkdownContent.astro" },
      customCss: ["./src/styles/docs.css"],
    }),
  ],
});
