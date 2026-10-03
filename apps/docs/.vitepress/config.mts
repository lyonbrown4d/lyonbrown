import { defineConfig } from "vitepress";

const englishSidebar = [
  { text: "Introduction", link: "/en/" },
  {
    text: "Architecture",
    items: [
      { text: "Local-First Agent Runtime", link: "/en/architecture/local-first-agent-runtime" },
      { text: "Local gRPC Client/Server", link: "/en/architecture/grpc-client-server" },
      { text: "Composition Root", link: "/en/architecture/composition-root" },
      { text: "Cross-Platform Sandbox", link: "/en/architecture/cross-platform-sandbox" },
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
      { text: "Phase-One MVP", link: "/en/plans/mvp-implementation-plan" },
      { text: "Phase-Two Product", link: "/en/plans/phase-two-product-plan" },
    ],
  },
  {
    text: "Reference",
    items: [
      { text: "Configuration", link: "/en/reference/configuration" },
      { text: "Local Models and Routing", link: "/en/reference/local-models-routing" },
    ],
  },
];

const chineseSidebar = [
  { text: "项目文档", link: "/zh/" },
  {
    text: "架构",
    items: [
      { text: "本地优先 Agent Runtime", link: "/zh/architecture/local-first-agent-runtime" },
      { text: "本地 gRPC C/S", link: "/zh/architecture/grpc-client-server" },
      { text: "组合根", link: "/zh/architecture/composition-root" },
      { text: "跨平台 Sandbox", link: "/zh/architecture/cross-platform-sandbox" },
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
      { text: "第一阶段 MVP", link: "/zh/plans/mvp-implementation-plan" },
      { text: "第二阶段产品", link: "/zh/plans/phase-two-product-plan" },
    ],
  },
  {
    text: "参考",
    items: [
      { text: "配置", link: "/zh/reference/configuration" },
      { text: "本地模型与路由", link: "/zh/reference/local-models-routing" },
    ],
  },
];

export default defineConfig({
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
});
