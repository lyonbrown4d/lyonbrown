# 开发与验证

工作区使用 Rust 2024 edition，产品依赖由 Cargo workspace 与 `Cargo.lock` 管理。Rust 工具链最低版本见根 `Cargo.toml` 的 `rust-version`。工程约束统一维护在根 `AGENTS.md`。

异步入口优先使用 `#[tokio::main]`，参数解析、序列化和依赖注入复用 Clap、Serde、fluxdi 的宏。`RunStatus` 和 `Tool` 通过固定版本 `strum` 0.28.0 的派生宏生成 snake_case 静态字符串转换；错误类型复用现有 `thiserror` 派生。CEF helper 和 Windows SCM 先完成平台分派；GPUI 保留主线程事件循环，需要持有或共享的运行时显式装配。

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

`cargo dev tui` 等价于 `cargo run --locked -p agent-dev -- tui`。首次执行时，Cargo 按 `Cargo.lock` 下载并编译 Rust 依赖，无需通过 `cargo install` 安装开发启动器、`just` 或 `cargo-make`。GPUI 和可选原生 SDK 所需的平台构建依赖仍需准备，见[本地分发说明](https://github.com/lyonbrown4d/lyonbrown/blob/main/packaging/README.md)。

产品和 Gateway 构建共用 `cargo_metadata` 0.23.1 的类型化 Cargo 消息解析：只采纳名称匹配的二进制目标，转发编译器诊断，读完输出后再检查进程退出状态。构建子进程复用 `process-wrap` 的 Unix 进程组或 Windows JobObject；取消构建时清理该构建拥有的进程树，交互客户端仍保留前台终端输入方式。

需要单独运行前台 daemon 时，使用 `cargo dev daemon`。其他开发客户端仍可自动连接并复用它。

默认配置为开发数据目录中的 `config.toml`，dotenv 文件位于配置文件旁边。`--config` 选择已有 TOML 文件；命令不创建或覆盖配置和 dotenv。模型、工具和日志使用共享 `agent-config` 的分层规则。agentd、TUI、Desktop 和开发启动器都通过共享 `agent-telemetry` 写入日志；默认位于开发数据目录的 `logs` 子目录，按进程前缀区分，并沿用配置中的滚动保留规则。启动命令强制使用所选开发地址和数据目录，并从该目录读取 `auth.token`，忽略继承的连接凭据。token 仅通过客户端子进程环境传递，不写入命令参数或启动输出。

`--dry-run` 展示启动方案，不创建目录、不读取 token、不启动产品进程；外层 Cargo 仍可能编译 `agent-dev`。默认 Desktop/TUI 构建不启用 `native-cef` 或 `native-computer`，对应的浏览器执行或原生计算机宿主能力会报告不可用。完整 CEF SDK、helper 和应用包布局见 [本地分发说明](https://github.com/lyonbrown4d/lyonbrown/blob/main/packaging/README.md)。

## Gateway 全栈开发启动

Gateway 使用同一仓库的独立 `cargo gateway` 别名，调用 `apps/agent-dev` 的 `gateway-dev` 程序。它不启动 agentd，也不注册操作系统服务。先按 [AI Gateway](gateway.md#启动与配置)准备已有 TOML、管理员用户名和密码，以及独立 API Key，再从仓库根目录执行：

```sh
cargo gateway --config /absolute/path/gateway.toml --data-dir /absolute/path/gateway-data
cargo gateway --help
cargo gateway --dry-run --config /absolute/path/gateway.toml
cargo gateway --build-only
cargo gateway --build-only --release
```

启动器先检查当前 Rust 工具链的 `wasm32-unknown-unknown` target，缺失时通过 rustup 准备；再检查仓库 `.local/tools/bin` 下的 cargo-leptos，缺失时以 `cargo install --locked --root <repo>/.local/tools cargo-leptos --version 0.3.11` 安装。已有工具必须匹配固定版本，不要求用户手动全局安装。Rust、平台编译工具和下载所需的网络仍需可用。

构建通过标准 cargo-leptos 的 `leptos build --project gateway` 完成；`--release` 同时选择服务端和 WASM 的发布配置。启动器再用对应配置运行增量 Cargo 构建，通过上述类型化 `compiler-artifact` 消息获取可执行文件路径，保留 Cargo 的 target 目录和目标配置。启动器通过 `cargo_metadata::Metadata` 读取工作区 metadata，校验 Gateway 项目字段及资源目录范围。工作区配置的默认资源根目录为 `.local/gateway-web/site`。Gateway 收到绝对 `LYONBROWN_GATEWAY_WEB_ROOT`，从 `/ui/pkg/` 提供生成的 JavaScript 与 WASM。

`--build-only` 生成服务端和资源后退出，不启动 Gateway。`--dry-run` 只打印方案，不执行工具、读取凭据或创建运行数据；外层 Cargo 仍可能编译启动器。`--config`、`--data-dir` 的相对路径从仓库根目录解析；默认运行数据为 `.local/gateway-web/data`。`--listen` 可选择非零回环端口。在启动终端按 Ctrl+C，只清理该启动器创建的 Gateway 进程。

Gateway 在运行时读取配置文件旁的 `.env`，进程环境优先。推荐将运行配置和凭据放在独立目录：启动器不主动把 Gateway 凭据传给构建命令，但官方 cargo-leptos 自身会读取工作区或父目录 `.env`，不能据此保证构建工具绝对接触不到运行凭据。不要将密码或 API Key 写进源码、构建参数或静态资源。

管理界面共用 Rust Leptos 组件，分别以 `ssr` 和 `hydrate` feature 构建服务端与浏览器 WASM。Signal 管理策略草稿和提交反馈，官方 `ServerAction` / `ActionForm` 处理表单，表格使用 `leptos-struct-table` 公开 `TableRow` 渲染接口直接支持 SSR。生成的 JavaScript 负责引导 WASM；不需要独立的 Node 前端栈。鉴权、CSRF、角色和策略版本校验仍在服务端完成。

分发时同时携带标准全栈构建的 `ai-gateway` 与 site 静态资源。`LYONBROWN_GATEWAY_WEB_ROOT` 指向包含 `ui/pkg` 的绝对目录，默认是可执行文件旁的 `web`。只有 `cargo run -p ai-gateway` 不会生成浏览器资源；缺少或空的 JS / WASM 文件会使启动明确失败。构建成功不等于已验证浏览器 hydrate、表单交互或跨平台运行，运行验收范围见[运行证据](runtime-evidence.md)。

## 原生计算机客户端构建

CLI 与 Desktop 的 `native-computer` feature 默认关闭，显式转发到 `agent-computer-runtime/native`，再启用 `agent-computer/native`。`cargo dev desktop` 和 `cargo dev tui` 的默认构建不自动启用它；`native-cef` 也不会启用原生计算机能力。开发启动器支持显式启用客户端 feature：

```sh
cargo dev desktop --native-computer
cargo dev tui --native-computer
cargo dev desktop --native-computer --dry-run
```

该参数只转发到客户端构建，不传给 agentd，也不修改 `computer_tools` 配置；`cargo dev daemon --native-computer` 会拒绝。也可以单独构建和启动客户端：

```sh
cargo build -p agent-cli -p agent-desktop --features agent-cli/native-computer,agent-desktop/native-computer --locked
cargo run -p agent-cli --features native-computer -- computer targets
cargo run -p agent-desktop --features native-computer
```

客户端通过共享 launcher 连接或复用原生 daemon 服务；开发前台 daemon 的使用方式见上方。原生 feature 不改变 daemon 的 `computer_tools = false` 默认值，也不代替系统权限、交互会话检查或准确动作审批。默认依赖和测试依赖均不通过 Cargo feature 合并隐式启用原生 SDK。目标选择、会话挂接和断开操作见[终端与桌面客户端](clients.md#原生计算机连接)。

构建通过只说明对应主机上的代码可编译。原生目标发现、GUI 选择、客户端到 Harness 的连接、屏幕观察与获批输入均需实际运行验收。macOS 按[会话身份与锁字段策略](protocol.md#原生桌面执行边界)检查，无法确认状态时返回 `SessionStateUnknown`；不得将只读权限检查、拒绝路径或合成宿主回归称为原生控制验收。真实捕获、获批输入、锁屏与会话切换仍需实际验收；Windows 和 Linux 的实际运行验收也尚未完成。

## 检查命令

在仓库根目录执行：

Unix 上先在当前终端执行 `umask 077`，让测试创建的数据库父目录仅对当前用户开放。网关持久化会拒绝组或其他用户可写的父目录；默认掩码允许组写入时，测试也会被该检查拒绝，不应为通过测试放宽持久化检查。

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

客户端视图按 MVVM 组织。TUI 的 `ViewModel` 从 `App` 生成只读呈现数据，`view.rs` 复用 Ratatui，输入区复用 `ratatui-textarea`，Markdown 复用 `tui-markdown` 的公开样式接口。Desktop 的 `DesktopViewModel` 保存当前聊天呈现状态，`operations.rs` 负责协议请求，`projection.rs` 负责应用回复，`views/`、`panes.rs` 和 `tabs/view.rs` 组合 `gpui-kit` 组件。网络客户端、运行时、凭据与原生宿主不放入 ViewModel。变更共享控件时复用组件库的输入、Markdown、Tab、停靠和调整尺寸能力；后台回复仍须按标签页及代次隔离。界面操作与限制见[终端与桌面客户端](clients.md)。

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

`apps/agentd/tests/computer_tools.rs` 使用本地模型 mock、三个标准协议的真实适配器、Harness 与可注入的合成 `attach_host`，检查观察、准确审批的批准与拒绝、模型及审批延迟期间续租、配置/宿主/输入权限限制，以及丢失或损坏回执后的持久 `Uncertain` 与恢复不重放。断言同时检查输入次数、审批、执行回执、事件和 Outbox。这些测试不执行操作系统输入，不能替代 CLI/Desktop 原生连接入口的实际运行或三平台桌面控制验收。

```sh
cargo test -p agentd --test computer_tools --locked
```

本地推理真实模型测试显式忽略，需自行提供已有的聊天 GGUF 文件。每条命令只验收所选后端、模型与当前主机：

```sh
LYONBROWN_TEST_GGUF=/absolute/path/model.gguf cargo test -p agent-inference --features llama --test native_models -- --ignored --nocapture
LYONBROWN_TEST_GGUF=/absolute/path/model.gguf cargo test -p agent-inference --features mistral --test native_models -- --ignored --nocapture
```

## 隔离的 Computer 测试窗口

```sh
cargo run --locked -p agent-desktop --example desktop-computer-target
```

该示例只提供合成按钮和文字输入框，不启动 daemon、模型或 ComputerRuntime，也不请求录屏或辅助功能权限。启动时输出本次 `.local/acceptance/computer-target-*` 目录，`state.json` 保存 `click_count` 和 `text`，退出后保留。每次运行使用新目录，适合作为原生控制验收的独立目标；操作此窗口不等于已验收 Harness 和审批链路。

## 可复用决策与 Gateway 检查

```sh
cargo test --locked -p decision-core -p decision-model -p decision-semantic
cargo test --locked -p gateway-core -p gateway-storage -p ai-gateway
cargo test --locked -p gateway-web --no-default-features --features ssr
cargo test --locked -p agent-dev --test gateway_commands
cargo check --locked -p gateway-web --no-default-features --features hydrate --target wasm32-unknown-unknown
cargo gateway --build-only
cargo check --locked -p decision-core --no-default-features
cargo run --locked -p decision-core --no-default-features --example embedded
cargo check --locked -p decision-semantic --features semantic-local
```

WASM 检查需要当前工具链已有 `wasm32-unknown-unknown` target，`cargo gateway` 会在构建前准备它。SSR 测试检查服务端页面、Server Functions 与安全边界；hydrate 检查验证浏览器构建，不能替代真实浏览器中的事件、表单和资源加载验收。

真实 Embedding 验收通过 `LYONBROWN_TEST_EMBEDDING_CONFIG` 显式指定 `OfflineEmbeddingConfig` JSON，其中每个资产都有绝对路径和 SHA-256。只有下面的显式测试会加载这些权重，默认工作区测试不会下载或加载模型：

```sh
LYONBROWN_TEST_EMBEDDING_CONFIG=/absolute/path/offline-embedding.json cargo test --locked -p decision-semantic --features semantic-local --test offline caller_supplied_real_model_replays_bilingual_queries_with_measured_thresholds -- --ignored --exact --nocapture
```

该测试使用独立的校准与回放样本，检查真实模型加载、向量维度、阈值和 Unknown 行为；小样本结果不能当作生产多语言准确率。引擎接口见[可复用决策引擎](decision-engine.md)，服务启动、密钥和 Swagger 见 [AI Gateway](gateway.md)。

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
