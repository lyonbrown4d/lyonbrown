# AGENTS.md

Lyonbrown 是 Rust monorepo。agentd、CLI/TUI、GPUI Desktop、BrowserRuntime 和公网控制面统一使用 Rust。文档使用 mdBook，维护中文和英文正文。

## 工程约束

1. Cargo workspace 与 Cargo.lock 管理产品依赖。优先复用维护中的第三方库，通过公开接口集成；不得恢复 Node Runtime、Electron、Tauri 或第二套 Agent Loop。
2. 使用 Rust 2024 edition、rustfmt 和 Clippy，统一 2 空格缩进，不使用 Tab。生产代码避免 panic、unwrap 和未受控 unsafe；必要的 FFI 仅放在平台适配边界，说明安全条件。
3. 单个手写源文件达到 300 行时评估按职责拆分。集成测试放在独立 tests/ 目录；不要为压缩行数损害内聚性。
4. Client 只拥有呈现、输入和连接状态。agentd 持有 Session、Task、模型、Tools、MCP、权限和事件；GPUI Desktop 与 TUI 均通过认证协议访问。
5. rpi-harness 是唯一 Agent Loop。rpi、CEF、推理库的具体类型不得泄漏到公共协议或 UI 模型。
6. SessionStore 保存 Harness 会话语义，TaskStore 保存编排状态；关键边界在同一本地 SQLite 事务中写入状态、事件和 Outbox。先记录执行意图，再产生副作用。未知结果进入 Uncertain，禁止因断线盲目重放。
7. Worker Attempt 成功不等于 Master 已采纳 Task。Session 单 Master 写入，持久化校验 owner_epoch；mDNS 发现不授信。
8. BrowserRuntime 是嵌入 CLI/Desktop 的可复用执行环境 crate，不是单独产品 app；CEF 内部 helper 通过客户端可执行文件的私有运行模式启动。GPUI UI 与浏览器执行状态隔离。UI 关闭不默认取消 daemon 作业。网页内容不可信，不能访问应用特权接口；Profile 不等于活页面恢复。
9. 操作能力面向 Windows、macOS、Linux。平台差异集中在适配层，统一授权、取消、超时和错误语义；缺失能力明确报告，不得把占位实现视为支持。
10. 本地推理通过 mistral.rs 与 llama-cpp-2 适配；两者只负责推理。优先使用成熟库实现持久作业、网络、协议、编辑器和 UI 组件，避免重复开发。
11. 文档只说明对应代码的机制、配置和限制，不写迁移流水或 M 阶段计划，不把声明和编译通过当成运行验收。
12. 数据库操作优先使用成熟 ORM，通过共享事务维护 Session、Task、事件和 Outbox 的原子性；原生 SQL 仅用于 ORM 无法表达的数据库配置或明确评审的特殊查询。
13. CLI 和 Desktop 安装包均包含 agentd，通过共享 launcher 检测并复用同一用户的原生服务；启动前检查 daemon 版本、协议兼容范围和能力。不得重复注册、静默降级或用普通后台子进程冒充原生服务；升级须处理活动任务和回滚。
14. 配置使用成熟库分层合并配置文件、dotenv、环境变量和 CLI，凭据不得进入日志或 UI 模型；所有产品进程使用共享日志组件将结构化日志持久化到用户数据目录，保留滚动和清理策略。
15. 业务服务按模块和依赖注入组织，使用固定版本的 fluxdi 及其 macros feature，优先类型化 static_module! 装配；构造函数显式接收端口，不把 Injector 或全局 service locator 传入业务逻辑。上游缺口或缺陷先最小复现并提交安全 issue，不复制、fork 或长期下游修补库。
16. 文件监听优先复用 notify 及成熟 debounce 组件，统一事件、重命名和重新扫描语义，缺失监听能力明确报告；产品文档须包含经过实际运行验收的截图并标注平台和版本。
17. 新工程约束统一维护在本文件。并行 agent 按目录分工，根配置、共享协议与最终集成由主 agent 协调。
18. 模型接入测试使用本地 mock server，经真实协议适配器验证各个已开放的标准协议，不调用付费模型。重点验证决策模型的结构化输出、置信度、授权约束、保守回退、超时和取消；决策不得扩大工具、模型或节点权限，也不得绕过人工审批。测试必须检查最终执行行为与持久化状态，不能只断言 mock 返回值。
19. 异步进程入口优先使用 `#[tokio::main]`，参数解析、序列化和依赖注入优先复用成熟库的宏，减少样板代码。CEF helper 与 Windows SCM 分派先于异步运行时初始化；GPUI 保留主线程事件循环。显式运行时仅用于需要持有、共享或定制运行时的边界，不为局部简化引入隐藏控制流的自定义宏。
20. ComputerRuntime 由交互客户端显式选择本机范围后绑定单一 Session；目标发现不截图、输入或请求系统权限。窗口选择同时绑定窗口 ID 和进程 ID，不因标识复用采用新目标。切换会话不转移控制权，重连和退出撤销原宿主，不重放原生输入。原生 SDK 通过客户端的 `native-computer` 功能显式启用，普通 agentd 构建不启用该功能。
21. `decision-core` 是默认嵌入 Agent 的可复用决策库，不依赖 Harness、Agent 内部类型、Provider SDK、HTTP 或数据库。语义与结构化推理通过适配层接入；建议不能授予工具、模型或节点权限，实际调度与长期绑定由调用方持有。
22. AI Gateway 独立于 agentd，优先使用 FluxDI 的 Axum 请求作用域、Diesel SQLite 和 Rust Leptos 全栈 Web UI，不引入独立 Node 前端栈。前端优先使用 `#[component]`、`view!`、Signal 和维护中的组件库，通过 SSR 与 WASM hydration 提供交互，表单复用 ServerAction / ActionForm，授权与持久化保持在服务端。OpenAPI 与 Swagger 优先使用 `utoipa` 生态从代码生成；API Key 与管理会话分离；环境变量初始化唯一超级管理员用户名和密码，密码使用成熟库的 Argon2id 加盐哈希，无默认密码，不以环境变量覆盖已有账户；写操作校验来源、超级管理员身份和 CSRF。 基础输入校验优先复用成熟库的派生宏，并在 Rust 前后端共享规则；权限、CSRF、PolicySnapshot 语义校验和 CAS 仍由服务端执行，不以前端校验代替服务端判定。 Gateway 上游仅复用固定版本 `rig-core` / `rig-reqwest` 0.45.0 的公开 raw HTTP transport 与 SseFramer，保留原始 JSON/SSE；资源限制、严格终态检查、取消和审计由 Gateway 持有，不引入 Rig Agent 或第二套 Agent Loop。Agent 与 model-provider 继续复用现有 rpi-ai 三协议适配，不要求全项目统一为 Rig。
23. `apps/model-mock` 提供独立、仅回环监听的合成模型服务，优先复用 HTTP Mock 与协议 SDK。远程标准协议、Ollama 协议和本地 `InferenceBackend` Mock 均不得调用付费模型或隐式加载权重；Mock 通过不代表真实推理 SDK、模型质量或性能验收。
24. TUI 与 Desktop 按 MVVM 组织客户端：View 组合成熟组件并呈现状态，ViewModel 持有或派生客户端展示状态，协议与命令适配层负责连接 daemon；不将凭据、原生执行句柄或 Agent 调度权限放入 ViewModel。优先复用组件库的输入、Markdown、Tab、折叠、布局、主题和可访问性能力，再补产品所需的状态与组合逻辑。

## 验证

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
mdbook build docs/zh
mdbook build docs/en
```

涉及依赖原生 SDK、GUI、真实模型或跨设备环境的能力，还需对应运行验收。只通过单机检查时不得声称三平台或公网场景已验收。
