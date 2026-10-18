# 开发与验证

工作区使用 Rust 2024 edition，产品依赖由 Cargo workspace 与 `Cargo.lock` 管理。Rust 工具链最低版本见根 `Cargo.toml` 的 `rust-version`。工程约束统一维护在根 `AGENTS.md`。

异步入口优先使用 `#[tokio::main]`，参数解析、序列化和依赖注入复用 Clap、Serde、fluxdi 的宏。CEF helper 和 Windows SCM 先完成平台分派；GPUI 保留主线程事件循环，需要持有或共享的运行时显式装配。

## 本地开发启动

仓库的 `.cargo/config.toml` 定义了 `dev` 别名，直接使用 Cargo，无需安装额外任务工具。在仓库根目录执行一条命令，自动构建、启动开发 agentd 并打开客户端：

```sh
cargo dev desktop
# 或启动终端界面
cargo dev tui
```

可以在不同终端多次执行 `cargo dev tui`，同时打开 Desktop。使用相同数据目录、地址和配置的客户端共用一个开发 agentd；命令自动传入连接地址和认证信息，无需先手动启动 daemon。

默认数据目录为 `.local/dev/agentd`，地址为 `http://127.0.0.1:7547`。开发目录在 `cargo clean` 后保留。命令使用前台开发进程，不注册或替换操作系统服务。首次启动 daemon 的终端保留其监督进程；关闭该终端中的客户端后，daemon 继续运行。在这个终端按 Ctrl+C 停止开发 daemon。后续终端只复用 daemon，退出客户端不会停止 daemon。

启动时会串行检查同一数据目录，避免多个客户端同时创建 daemon。复用前通过认证握手检查节点身份、协议、构建和配置。端口被其他进程占用、凭据不匹配或已有 daemon 与本次构建、配置不一致时，命令报错并保留已有进程。需要使用新构建或配置时，先在原终端停止开发 daemon，再重新执行启动命令。

路径相对于仓库根目录解析，可包含空格。多个客户端应使用相同参数；开发端口必须是非零回环端口，且不能使用产品默认端口 7447：

```sh
cargo dev desktop --dry-run
cargo dev tui --data-dir .local/dev/demo --listen 127.0.0.1:7548 --config /absolute/path/dev.toml
cargo dev desktop --data-dir .local/dev/demo --listen 127.0.0.1:7548 --config /absolute/path/dev.toml
cargo dev --help
```

Cargo 别名使用参数数组调用 `apps/agent-dev`，不依赖平台 Shell，也无需安装额外的任务运行器。根 `README.md` 列出常用命令，`cargo --list` 也会展示 `dev` 别名。

需要单独运行前台 daemon 时，使用 `cargo dev daemon`。其他开发客户端仍可自动连接并复用它。

默认配置为开发数据目录中的 `config.toml`，dotenv 文件位于配置文件旁边。`--config` 选择已有 TOML 文件；命令不创建或覆盖配置和 dotenv。模型、工具和日志使用共享 `agent-config` 的分层规则。agentd、TUI、Desktop 和开发启动器都通过共享 `agent-telemetry` 写入日志；默认位于开发数据目录的 `logs` 子目录，按进程前缀区分，并沿用配置中的滚动保留规则。启动命令强制使用所选开发地址和数据目录，并从该目录读取 `auth.token`，忽略继承的连接凭据。token 仅通过客户端子进程环境传递，不写入命令参数或启动输出。

`--dry-run` 展示启动方案，不创建目录、不读取 token、不启动产品进程；外层 Cargo 仍可能编译 `agent-dev`。默认 Desktop/TUI 构建不启用 `native-cef`，浏览器执行能力会报告不可用。完整 CEF SDK、helper 和应用包布局见 [本地分发说明](https://github.com/lyonbrown4d/lyonbrown/blob/main/packaging/README.md)。

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

`apps/agentd/tests/computer_tools.rs` 使用本地模型 mock、三个标准协议的真实适配器、Harness 与可注入的合成 `attach_host`，检查观察、准确审批的批准与拒绝、模型及审批延迟期间续租、配置/宿主/输入权限限制，以及丢失或损坏回执后的持久 `Uncertain` 与恢复不重放。断言同时检查输入次数、审批、执行回执、事件和 Outbox。这些测试不执行操作系统输入，不能作为 CLI/Desktop 原生连接入口或三平台桌面控制验收。

```sh
cargo test -p agentd --test computer_tools --locked
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

文档使用本机构建、分支发布的方式部署到 GitHub Pages。执行 `mdbook build docs/zh` 和 `mdbook build docs/en`，将 `docs/index.html` 放到发布根目录，两套构建结果分别放到 `zh/` 和 `en/`，根目录另放一个空的 `.nojekyll` 文件。构建目录不包含 Cargo 产物、本机配置或凭据。

静态文件提交到独立的 `gh-pages` 分支；Pages 的构建类型设为 `legacy`，发布来源为该分支的 `/`。发布前检查两种语言的本地资源与链接，推送后检查 Pages 构建结果，并回读公开站点。分支推送成功不等于站点已发布成功。

`.github/workflows/pages.yml` 保留为支持 GitHub Actions 的环境使用的手动发布入口；使用该方式前须将 Pages 来源切换为 GitHub Actions。当前手动分支发布不调用这个工作流。
