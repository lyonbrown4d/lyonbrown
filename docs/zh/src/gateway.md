# AI Gateway

`ai-gateway` 是独立 Rust 服务，默认监听 `127.0.0.1:8099`。它通过 FluxDI 的类型化模块与 Axum 请求作用域装配共享决策核心、SQLite 存储、模型转发和 Leptos 管理界面，不需要启动 agentd。Agent 使用网关时仍自行管理 Harness、Session、工具和审批。

## 启动与配置

```sh
cargo gateway --config /absolute/path/gateway.toml --data-dir /absolute/path/gateway-data
```

在仓库根目录运行。`cargo gateway` 调用独立的 `gateway-dev` 启动器，构建服务端与浏览器资源后启动 Gateway，不启动 agentd。首次使用会通过 rustup 为当前工具链准备官方 `wasm32-unknown-unknown` target，并将固定版本 cargo-leptos 0.3.11 安装到仓库的 `.local/tools`；无需手动全局安装。已有工具须通过版本检查。命令选项和构建流程见[开发与验证](development.md#gateway-全栈开发启动)。

配置库按默认值、TOML、相邻 `.env`、进程环境和 CLI 合并。`--listen` 最后覆盖地址；监听地址必须是本机非零端口。可用环境变量包括 `LYONBROWN_GATEWAY_CONFIG`、`LYONBROWN_GATEWAY_DATA_DIR`、`LYONBROWN_GATEWAY_LISTEN`、`LYONBROWN_GATEWAY_WEB_ROOT`、`LYONBROWN_GATEWAY_TOKEN`、`LYONBROWN_GATEWAY_ADMIN_USERNAME` 和 `LYONBROWN_GATEWAY_ADMIN_PASSWORD`。

```toml
listen = "127.0.0.1:8099"

[gateway]

[[gateway.models]]
id = "local-chat"
upstream_model = "deployed-model-id"
api_base = "http://127.0.0.1:8080/v1"
api_key_env = "LOCAL_MODEL_API_KEY"
tier = "balanced"
local_inference = true
capabilities = ["text", "tools"]
context_tokens = 4096
```

`api_base` 须替换为已部署服务。上游凭据通过 `api_key_env` 指向环境变量或 `.env`，不写在模型配置中。

管理台使用用户名和密码登录。首次启动从下列环境变量或相邻 `.env` 初始化唯一的超级管理员；没有内置账户或默认密码。

| 环境变量 | 要求 |
| --- | --- |
| `LYONBROWN_GATEWAY_ADMIN_USERNAME` | 1–64 个 ASCII 字符，仅字母、数字、`_`、`.`、`@`、`-`；区分大小写 |
| `LYONBROWN_GATEWAY_ADMIN_PASSWORD` | 15–128 字节密码，不得与 API Key 相同 |
| `LYONBROWN_GATEWAY_TOKEN` | 独立生成的 32–512 字节可打印随机 API Key，仅用于模型接口 Bearer 鉴权 |

管理员用户名和密码不能写入 TOML 配置。数据库保存用户名、超级管理员角色和带随机盐的 Argon2id 密码哈希，不保存明文密码。再次启动时相同凭据通过校验；不同凭据使启动失败，不会覆盖原账户或重置密码。API Key 不能用于管理台登录，超级管理员密码也不能用于模型 API。超级管理员可查看模型目录与请求记录，并修改路由策略。

推荐将 Gateway TOML 和凭据 `.env` 放在独立的配置目录。启动器不将管理员密码或模型凭据放入构建命令参数，也不主动传给构建子进程；不过 cargo-leptos 自身会读取工作区或父目录的 `.env`，不能把启动器的环境过滤当作构建工具无法访问凭据的保证。

`local_inference = true` 是可信部署方对上游实际本地推理的声明，并要求使用字面量回环地址。不能将转发云模型的本地代理标记为本地推理。远程上游必须使用 HTTPS；客户端不能指定任意上游地址。

## 构建资源与分发

开发启动器按工作区的 `gateway` cargo-leptos 配置构建，资源默认输出到 `.local/gateway-web/site/ui/pkg`。`--build-only` 只构建；`--release` 选择发布构建。启动器从 Cargo 的构建结果获取服务端程序路径，保留 `CARGO_TARGET_DIR` 等构建配置，并向 Gateway 传入实际的资源根目录。

分发时同时提供 `ai-gateway` 可执行文件和生成的静态资源。`LYONBROWN_GATEWAY_WEB_ROOT` 必须是绝对路径，指向包含 `ui/pkg` 的 site 根目录；未指定时使用可执行文件旁的 `web` 目录。标准构建生成 `ui/pkg/gateway_web.js` 和 `ui/pkg/gateway_web.wasm`，管理页面通过 `/ui/pkg/` 加载它们。仅执行 `cargo run -p ai-gateway` 不会生成浏览器资源；资源缺失或为空时，Gateway 在数据库初始化前明确报错。应使用标准全栈构建并一起分发同次构建的二进制与资源。

## HTTP 与 Swagger

| 方法与路径 | 用途与访问方式 |
| --- | --- |
| `GET /health/live` | 进程存活 |
| `GET /health/ready` | 已验证配置与接收状态；不代表上游健康探测 |
| `GET /v1/models` | 配置的公共模型 ID；需要 API Bearer Key |
| `POST /v1/chat/completions` | 文本与函数工具的 Chat Completions 转发；需要 API Bearer Key |
| `GET /swagger-ui/` | 官方 Swagger UI，只读文档入口 |
| `GET /api-doc/openapi.json` | `utoipa` 从代码生成的 OpenAPI |
| `GET /ui/login` | 管理登录 |

Swagger 页面与规范在本机地址可直接打开，业务 API 仍需通过 Swagger 的 Authorize 提供 API Key。规范不包含实际模型目录或凭据，Swagger 资源随程序内嵌，不要求访问 CDN。当前入站协议为 Chat Completions；图片、音频、Responses 和 Anthropic 入站请求不在此接口的兼容范围内。

Gateway 上游传输复用 [Rig-Core 0.44.0](https://docs.rs/rig-core/0.44.0/rig_core/) 的公开 raw HTTP 接口与 `SseFramer`，以及 [Rig-Reqwest 0.44.0](https://docs.rs/rig-reqwest/0.44.0/rig_reqwest/) 的 `ReqwestMiddlewareClient`。Gateway 保留 `n` 等已开放的请求参数，并按原始字节转发 JSON 和 SSE 响应，包括多候选结果、工具调用片段和 usage。中间件在非 2xx 时立即释放原始响应正文，只向 Rig 提供相同状态的空正文；Gateway 继续负责字节限制、严格 `[DONE]` 检查、时限、取消和审计。这里不使用 Rig Agent 或 Agent Loop；Agent 与 `model-provider` 继续复用 rpi-ai 的 Responses、Chat Completions 和 Anthropic 三协议适配。

指定 `model` 为公共模型 ID 时按配置直连，不因不存在或能力不足切换模型。`model = "router/auto"` 时调用 [共享决策引擎](decision-engine.md)，选择实际模型后再次核对工具能力、上下文容量、本地范围及服务器约束。请求中的 `routing.local_only` 和 `routing.allowed_tiers` 只能进一步收紧服务器限制，转发前移除。

转发保留 SSE 的工具调用片段、usage 与数据字节；维护中的 SSE 解析库另行检查结束标记。请求大小、响应大小、并发、首字节和总时限均有限制。并发许可持续到审计写入结束且回复正文耗尽、释放或到达总时限；提前收到结束标记不会绕过未读正文的容量限制。断开连接会释放上游响应；中断或缺少结束标记进入未知结果，不自动重试。错误响应不回显上游错误正文、Prompt 或凭据。

首字节预算从请求接收时开始，包含自动路由；路由超过该预算返回 `504 routing_timeout`，不发送上游请求。

## SQLite 与管理界面

默认数据库位于数据目录的 `gateway.sqlite`，由 Diesel 管理，启用 WAL、busy timeout 和完整同步。单进程持有数据库所有权锁，启动时将遗留 Pending 请求标记为 Uncertain。模型快照、策略历史、API Key 摘要、超级管理员账户、管理会话和请求元数据保存在该库；没有原始请求或回复正文的存储字段。

发送上游请求前先保存 Pending 意图，再更新完成或未知状态。Succeeded 表示已收到并验证上游成功终态，不证明调用方已消费回复。数据库写入责任独立于 HTTP 等待期限；请求超时后仍完成已开始的意图或终态写入。实际写库失败留下的 Pending 在重启时恢复为 Uncertain，不能因为 HTTP 连接关闭就认定推理未发生。

管理页面复用同一套 Rust Leptos 组件：服务端先渲染 HTML，浏览器加载生成的 JavaScript 引导代码与 WASM，再为现有页面接入事件和响应式状态。没有独立的 Node 前端栈。页面包括模型目录、路由策略和分页请求记录，主题跟随系统浅色或深色偏好。

布局、表单字段、提交状态和提示是共享组件。Leptos Signal 管理策略草稿、修改状态、即时校验和重置；官方 `ServerAction` / `ActionForm` 提交登录、保存和退出。表格复用 `leptos-struct-table` 的公开 `TableRow` 派生与行渲染接口，表头、语义化表格和分页由页面组件组织，可在 SSR 时直接输出行内容，不依赖浏览器虚拟列表完成首屏。

`gateway-validation` 使用 [Garde 0.23.0](https://docs.rs/garde/0.23.0/garde/) 的派生宏与 regex 规则定义 `Credentials`、`PolicyDraft` 基础校验，供 SSR、WASM、应用配置及存储入口共享；授权、CSRF、`PolicySnapshot` 语义校验与 CAS 仍由服务端执行。

浏览器校验只提供交互反馈。敏感读取、密码验证、超级管理员角色、会话有效期、Origin 和会话绑定 CSRF 仍在服务端核验。登录会话使用带期限的 HttpOnly / SameSite=Strict Cookie；策略保存与退出同时验证来源和 CSRF。策略更新校验完整文档并比较旧 revision，保存成功后发布到内存；并发编辑冲突要求刷新。模型上游凭据和密码哈希不进入浏览器 DTO。

模型目录来自部署配置，启动时保存不可变版本快照；管理页面不提供上游凭据编辑。此服务骨架不包含多租户配额账本、实际计费、Provider 账户管理或公网部署。共享日志组件将结构化日志滚动保存在数据目录，指标快照提供请求、拒绝、耗时和结果计数。

## 验证

本地 HTTP 测试使用合成上游，检查实际模型改写、Bearer 校验、能力与本地范围、流式取消和超时、未知结果、SQLite 恢复、管理登录、CSRF、策略版本及生成的 OpenAPI。SSR 测试与 WASM hydrate 编译检查命令见[开发与验证](development.md#可复用决策与-gateway-检查)。运行截图与范围见[运行证据](runtime-evidence.md)；旧截图不自动证明新的 hydrate 交互已验收。这些测试不调用付费模型，也不代表公网、多租户或三平台运行验收。
