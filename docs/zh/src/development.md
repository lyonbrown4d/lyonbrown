# 开发与验证

工作区使用 Rust 2024 edition，产品依赖由 Cargo workspace 与 `Cargo.lock` 管理。Rust 工具链最低版本见根 `Cargo.toml` 的 `rust-version`。工程约束统一维护在根 `AGENTS.md`。

## 检查命令

在仓库根目录执行：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
mdbook build docs/zh
mdbook build docs/en
```

这些命令分别检查格式、静态问题、测试、构建和文档生成。它们不能替代真实模型调用、GUI 启动、CEF 操作、系统权限或跨设备网络验收。验证结果应同时说明命令、环境、场景和未覆盖范围。

## 修改模块

客户端只修改呈现与输入；模型、工具、权限和持久状态应留在 daemon。新增字段先维护 `proto/agent/v1/agent.proto` 和两端消费者，避免把运行库内部类型放进协议。

单个手写源文件达到 300 行时评估按职责拆分。集成测试放入对应 crate 的 `tests/` 目录。生产代码避免 `panic`、`unwrap` 和未受控的 `unsafe`。必要 FFI 只保留在平台适配边界，并说明安全条件。

## 维护文档

```sh
mdbook serve docs/zh --open
mdbook serve docs/en --open
```

两种语言使用相同章节结构，各自维护正文。文档说明对应代码的机制、配置与限制；源码、编译、测试和真实环境验收分别陈述。主题沿用 mdBook 默认样式，图表扩展位于 `docs/theme`，第三方浏览器资源及许可证记录在 `docs/theme/VENDOR.md`。

新增图表使用 Mermaid 代码块。浏览器适配器负责渲染和大图交互，不需要额外的文档预处理器。修改共享图表资源后，构建两本书并检查明暗主题、缩放、移动、键盘关闭与 SVG 下载。

## 模型协议与决策回归

`agent-test-support` 提供共享本地 HTTP mock server。`agent-runtime` 与 `agent-decision` 的集成测试通过真实协议适配器验证 Responses、Chat Completions 和 Anthropic Messages，不调用付费模型。覆盖结构化输出、权限收缩、低置信度、保守回退、超时和取消；daemon 测试还核对实际工具行为和持久化事件，不能仅凭 mock 返回值认定执行成功。

```sh
cargo test -p agent-runtime -p agent-decision -p agentd --locked
cargo test -p agent-mcp --features test-fixtures --locked
```

本地推理真实模型测试显式忽略，需自行提供已有的聊天 GGUF 文件。每条命令只验收所选后端、模型与当前主机：

```sh
LYONBROWN_TEST_GGUF=/absolute/path/model.gguf cargo test -p agent-inference --features llama --test native_models -- --ignored --nocapture
LYONBROWN_TEST_GGUF=/absolute/path/model.gguf cargo test -p agent-inference --features mistral --test native_models -- --ignored --nocapture
```

## 原生服务验收

macOS 的以下测试使用真实 launchd 注册，只能在没有 Lyonbrown 服务注册且 `127.0.0.1:7447` 空闲的测试账号上执行。先构建产品 daemon，再提供其绝对路径：

```sh
cargo build -p agentd --bin agentd --locked
LYONBROWN_SERVICE_AGENTD=/absolute/path/agentd cargo test -p agent-launch --test native_service --locked -- --ignored --nocapture --test-threads=1
```

测试检查两个客户端复用同一服务、不同数据目录不能替换服务，以及原生服务重启后节点身份和会话记录保持一致。成功后会验证注册归属和空闲状态，再移除本次服务与测试数据。失败时保留数据，避免删除仍在运行的 daemon；先核对 `service.json` 与原生服务状态，再处理遗留注册。

此路径已在 macOS 27.0.1、arm64 上通过实际产品二进制验收，不涉及模型调用或钥匙串授权。它不替代 PKG 的操作系统安装验收，也不证明 Windows SCM 或 Linux systemd 的运行行为。

## 文档站点发布

`.github/workflows/pages.yml` 使用固定版本 mdBook 构建中英文书籍，将语言入口、`zh/` 与 `en/` 放入同一个 Pages artifact。仓库须将 GitHub Pages 的构建来源设为 GitHub Actions。工作流在 `main` 的文档变更或手动触发时运行，部署作业使用 `github-pages` 环境；实际站点可用性以部署结果与页面回读为准。
