import { defineConfig } from "vitepress";
import { withMermaid } from "vitepress-plugin-mermaid";

const englishSidebar = [
  { text: "Introduction", link: "/en/" },
  {
    text: "Architecture",
    items: [
      { text: "Local gRPC Client/Server", link: "/en/architecture/grpc-client-server" },
      { text: "Composition Root", link: "/en/architecture/composition-root" },
      { text: "Electron Desktop", link: "/en/architecture/electron-desktop" },
      { text: "System One Decision Layer", link: "/en/architecture/system-one-decision-layer" },
    ],
  },
  {
    text: "Design",
    items: [{ text: "TUI Interaction", link: "/en/design/tui-interaction-design" }],
  },
  {
    text: "Plans",
    items: [
      { text: "Node/TypeScript + Pi Migration", link: "/en/plans/node-typescript-pi-migration" },
      { text: "Migration Pause Checkpoint", link: "/en/plans/migration-status" },
    ],
  },
  {
    text: "Reference",
    items: [
      { text: "Configuration", link: "/en/reference/configuration" },
      { text: "Local Models and Routing", link: "/en/reference/local-models-routing" },
      { text: "Release Artifacts", link: "/en/reference/release-artifacts" },
    ],
  },
];

const chineseSidebar = [
  { text: "项目文档", link: "/zh/" },
  {
    text: "架构",
    items: [
      { text: "本地 gRPC C/S", link: "/zh/architecture/grpc-client-server" },
      { text: "组合根", link: "/zh/architecture/composition-root" },
      { text: "Electron Desktop", link: "/zh/architecture/electron-desktop" },
      { text: "System One 决策层", link: "/zh/architecture/system-one-decision-layer" },
    ],
  },
  {
    text: "设计",
    items: [{ text: "TUI 交互设计", link: "/zh/design/tui-interaction-design" }],
  },
  {
    text: "计划",
    items: [
      { text: "Node/TypeScript + Pi 迁移", link: "/zh/plans/node-typescript-pi-migration" },
      { text: "迁移暂停检查点", link: "/zh/plans/migration-status" },
    ],
  },
  {
    text: "参考",
    items: [
      { text: "配置", link: "/zh/reference/configuration" },
      { text: "本地模型与路由", link: "/zh/reference/local-models-routing" },
      { text: "发布制品", link: "/zh/reference/release-artifacts" },
    ],
  },
];

export default withMermaid(
  defineConfig({
    title: "Lyonbrown",
    description: "Local-first agent runtime documentation",
    cleanUrls: true,
    locales: {
      root: {
        label: "Languages",
        lang: "en-US",
        themeConfig: {
          search: { provider: "local" },
          nav: [
            { text: "English", link: "/en/" },
            { text: "简体中文", link: "/zh/" },
          ],
        },
      },
      en: {
        label: "English",
        lang: "en-US",
        link: "/en/",
        themeConfig: {
          search: { provider: "local" },
          nav: [
            { text: "Home", link: "/en/" },
            { text: "Migration", link: "/en/plans/node-typescript-pi-migration" },
          ],
          sidebar: englishSidebar,
        },
      },
      zh: {
        label: "简体中文",
        lang: "zh-CN",
        link: "/zh/",
        themeConfig: {
          search: { provider: "local" },
          nav: [
            { text: "首页", link: "/zh/" },
            { text: "迁移计划", link: "/zh/plans/node-typescript-pi-migration" },
          ],
          sidebar: chineseSidebar,
        },
      },
    },
  }),
);
