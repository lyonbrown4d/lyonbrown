# Lyonbrown 文档

本目录是项目设计与实施资料的统一入口。

## 文档导航

- [Runtime 架构设计](architecture/local-first-agent-runtime.md)：长期架构目标、核心原则、模块边界与阶段划分。
- [第一阶段 MVP 实施计划](plans/mvp-implementation-plan.md)：M0–M9 的交付内容、验收标准、风险与验证命令。

## 当前状态

- **已完成：M0 — Skeleton + agent-core**
- **已完成：M1 — Kernel + MockProvider（离线 E2E）**
- **下一阶段：M2 — Ollama Provider + 本地模型**
- M3–M8 待实施；M9 为可选 stretch goal。

里程碑的详细范围和验收口径以 MVP 实施计划为准；架构文档描述长期方向，不直接表示当前实现进度。

## 目录约定

```text
docs/
├── README.md       # 文档索引与当前阶段
├── architecture/   # 长期架构与设计原则
└── plans/          # 可执行的阶段计划与验收标准
```

新增文档应放入对应分类目录，并从本索引链接，避免在仓库根目录散落设计资料。
