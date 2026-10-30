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

可选的 `gateway.endpoints` 为公共模型 ID 配置一个或多个上游端点；未映射端点的模型仍使用 `gateway.models` 中的 `api_base` 和 `api_key_env`。只要某个模型 ID 已映射到端点，Gateway 就只从该模型的端点中选择；若这些端点全部禁用、冷却中或不可用，不会退回旧模型地址。`gateway.endpoints` 为空时，所有模型均沿用旧配置。

```toml
[[gateway.endpoints]]
id = "local-primary"
provider_id = "local-provider"
api_base = "http://127.0.0.1:8080/v1"
api_key_env = "LOCAL_MODEL_API_KEY"
enabled = true
priority = 10
max_concurrent = 8
model_ids = ["local-chat"]
```

`id` 与 `provider_id` 是配置标识；`api_base` 指向 Chat Completions API 基址；`api_key_env` 从环境或 `.env` 读取凭据。`model_ids` 必须列出该端点可服务的公共模型 ID。数值较小的 `priority` 优先，优先级相同时按 `id` 排序；`enabled` 默认开启，`priority` 默认 `0`，`max_concurrent` 默认 `1`。应用配置只接受 `api_key_env`，不接受端点明文凭据。远程端点使用 HTTPS；HTTP 只允许字面量回环 IP。

管理台使用用户名和密码登录。首次启动从下列环境变量或相邻 `.env` 初始化唯一的超级管理员；没有内置账户或默认密码。

| 环境变量 | 要求 |
| --- | --- |
| `LYONBROWN_GATEWAY_ADMIN_USERNAME` | 1–64 个 ASCII 字符，仅字母、数字、`_`、`.`、`@`、`-`；区分大小写 |
| `LYONBROWN_GATEWAY_ADMIN_PASSWORD` | 15–128 字节密码，不得与 API Key 相同 |
| `LYONBROWN_GATEWAY_TOKEN` | 独立生成的 32–512 字节可打印随机 API Key，仅用于模型接口 Bearer 鉴权 |

管理员用户名和密码不能写入 TOML 配置。数据库保存用户名、超级管理员角色和带随机盐的 Argon2id 密码哈希，不保存明文密码。再次启动时相同凭据通过校验；不同凭据使启动失败，不会覆盖原账户或重置密码。API Key 不能用于管理台登录，超级管理员密码也不能用于模型 API。超级管理员可查看模型目录与请求记录，并修改路由策略。

Gateway 的上游凭据、API 认证密钥、管理会话及新签发的 API token 使用 `secrecy` 0.10.3 的秘密容器保存；密码工作线程也使用该容器。读取秘密需要显式访问，`Debug` 隐去内容，容器释放时清零其持有的内存。这不保证配置解析、环境变量、HTTP 处理或临时 `String` 中的所有副本都已清零。

推荐将 Gateway TOML 和凭据 `.env` 放在独立的配置目录。启动器不将管理员密码或模型凭据放入构建命令参数，也不主动传给构建子进程；不过 cargo-leptos 自身会读取工作区或父目录的 `.env`，不能把启动器的环境过滤当作构建工具无法访问凭据的保证。

`local_inference = true` 是可信部署方对上游实际本地推理的声明，并要求使用字面量回环地址。不能将转发云模型的本地代理标记为本地推理。远程上游必须使用 HTTPS；客户端不能指定任意上游地址。

当请求或服务器策略要求 `local_only` 时，模型必须由可信配置标记为本地推理，且实际选中的端点地址也必须是字面量回环 IP。客户端提交 `routing.local_only` 只能收紧范围，不能替模型授予本地推理身份。即使端点地址是回环地址，也不能单独证明它运行的是本地模型。

## 构建资源与分发

开发启动器按工作区的 `gateway` cargo-leptos 配置构建，资源默认输出到 `.local/gateway-web/site/ui/pkg`。`--build-only` 只构建；`--release` 选择发布构建。启动器从 Cargo 的构建结果获取服务端程序路径，保留 `CARGO_TARGET_DIR` 等构建配置，并向 Gateway 传入实际的资源根目录。

分发时同时提供 `ai-gateway` 可执行文件和生成的静态资源。`LYONBROWN_GATEWAY_WEB_ROOT` 必须是绝对路径，指向包含 `ui/pkg` 的 site 根目录；未指定时使用可执行文件旁的 `web` 目录。标准构建生成 `ui/pkg/gateway_web.js` 和 `ui/pkg/gateway_web.wasm`，管理页面通过 `/ui/pkg/` 加载它们。仅执行 `cargo run -p ai-gateway` 不会生成浏览器资源；资源缺失或为空时，Gateway 在数据库初始化前明确报错。应使用标准全栈构建并一起分发同次构建的二进制与资源。

## HTTP 与 Swagger

Gateway 使用 `utoipa-axum` 0.3.0 的 `routes!` 和 `OpenApiRouter`，从同一组处理函数注册 Axum 路由并生成 OpenAPI。模型接口与公开健康检查分别组装；Bearer 鉴权中间件仍仅施加于受保护的模型接口。

| 方法与路径 | 用途与访问方式 |
| --- | --- |
| `GET /health/live` | 进程存活 |
| `GET /health/ready` | 已验证配置与接收状态；不代表上游健康探测 |
| `GET /v1/models` | 当前 API Key 获准访问的公共模型 ID；需要 API Bearer Key |
| `POST /v1/chat/completions` | 文本与函数工具的 Chat Completions 转发；需要 API Bearer Key |
| `GET /swagger-ui/` | 官方 Swagger UI，只读文档入口 |
| `GET /api-doc/openapi.json` | `utoipa` 从代码生成的 OpenAPI |
| `GET /ui/login` | 管理登录 |
| `GET /ui/keys` | API Key 管理；需要超级管理员会话 |

Swagger 页面与规范在本机地址可直接打开，业务 API 仍需通过 Swagger 的 Authorize 提供 API Key。规范不包含实际模型目录或凭据，Swagger 资源随程序内嵌，不要求访问 CDN。当前入站协议为 Chat Completions；图片、音频、Responses 和 Anthropic 入站请求不在此接口的兼容范围内。

Gateway 上游传输复用 [Rig-Core 0.45.0](https://docs.rs/rig-core/0.45.0/rig_core/) 的公开 raw HTTP 接口与 `SseFramer`，以及 [Rig-Reqwest 0.45.0](https://docs.rs/rig-reqwest/0.45.0/rig_reqwest/) 的 `ReqwestMiddlewareClient`。Gateway 保留 `n` 等已开放的请求参数，并按原始字节转发 JSON 和 SSE 响应，包括多候选结果、工具调用片段和 usage。中间件在非 2xx 时立即释放原始响应正文，只向 Rig 提供相同状态的空正文；Gateway 继续负责字节限制、严格 `[DONE]` 检查、时限、取消和审计。这里不使用 Rig Agent 或 Agent Loop；Agent 与 `model-provider` 继续复用 rpi-ai 的 Responses、Chat Completions 和 Anthropic 三协议适配。

指定 `model` 为公共模型 ID 时按配置直连，不因不存在或能力不足切换模型。`model = "router/auto"` 时调用 [共享决策引擎](decision-engine.md)，选择实际模型后再次核对工具能力、上下文容量、本地范围及服务器约束。请求中的 `routing.local_only` 和 `routing.allowed_tiers` 只能进一步收紧服务器限制，转发前移除。

转发保留 SSE 的工具调用片段、usage 与数据字节；维护中的 SSE 解析库另行检查结束标记。请求大小、响应大小、并发、首字节和总时限均有限制。并发许可持续到审计写入结束且回复正文耗尽、释放或到达总时限；提前收到结束标记不会绕过未读正文的容量限制。断开连接会释放上游响应；中断或缺少结束标记进入未知结果，不自动重试。错误响应不回显上游错误正文、Prompt 或凭据。

首字节预算从请求接收时开始，包含自动路由；路由超过该预算返回 `504 routing_timeout`，不发送上游请求。

显式上游 `429` 会让该端点在当前进程的后续选择中冷却 30 秒；上游 `5xx` 冷却 5 秒。冷却只影响之后的请求，不会重放或切换当前请求。选中的端点并发许可与 Gateway 请求许可一样，持续到终态审计完成且响应正文耗尽、释放或到达总时限。

## SQLite 与管理界面

默认数据库位于数据目录的 `gateway.sqlite`，由 Diesel 管理，启用 WAL、busy timeout 和完整同步。单进程持有数据库所有权锁，启动时将遗留 Pending 请求标记为 Uncertain。模型快照、策略历史、API Key 哈希及范围、超级管理员账户、管理会话和请求元数据保存在该库；没有原始请求或回复正文的存储字段。

数据库脚本通过 `diesel_migrations` 2.3.2 的 `embed_migrations!` 内嵌，由 Diesel 迁移记录表管理。启动时在同一 SQLite 立即事务中衔接旧 `gateway_migrations` 的版本 1 或版本 1、2 记录、执行待运行脚本并恢复 Pending 请求；已执行脚本不重复运行。未知版本、不连续记录、两套记录不一致或没有记录的既有业务表使启动失败，事务回滚。

发送上游请求前先保存 Pending 意图，再更新完成或未知状态。Succeeded 表示已收到并验证上游成功终态，不证明调用方已消费回复。数据库写入责任独立于 HTTP 等待期限；请求超时后仍完成已开始的意图或终态写入。实际写库失败留下的 Pending 在重启时恢复为 Uncertain，不能因为 HTTP 连接关闭就认定推理未发生。

### API Key 与 Token 预算

超级管理员在 `/ui/keys` 创建或撤销 API Key。创建操作要求有效的管理员会话、正确的 `Origin` 和与该会话绑定的 CSRF 值；新密钥只在创建成功后显示一次。数据库保留密钥哈希与管理元数据，不会再次显示密钥内容。每个 Key 可设置项目标识、允许的模型列表、每个候选结果的输出上限、累计 Token 预算和有效期（1 到 31,536,000 秒；表单默认 30 天）。允许模型列表留空表示不按模型 ID 限制；输出上限或预算留空表示不设置该项限制。

`project_id` 用于标记请求归属，不会把同一项目下多个 Key 合并成共享预算。预算按单个 Key 累计，统计输入与输出 Token 之和，不按月或其他周期重置，也不是货币计费。

转发前，Gateway 在同一本地 SQLite 立即事务中写入 Pending 审计记录并预留该 Key 的预算；预留值按校验后请求 JSON 的 UTF-8 字节长度，加上单次输出上限乘以 `n` 个候选结果保守估算。预算不足时返回 `429`，不会发送上游请求。收到并验证终态 usage 后，已知的输入与输出 Token 合计会替换预留；缓存输入和推理 Token 是各自总量的子集，只作为审计信息，不重复计入。

上游不提供可用 usage 时，Gateway 保留预留，不把未知计为零；Uncertain 结果也保留全部预留。进程重启时，未终结的 Pending 被标记为 Uncertain，预算预留继续计入。上游实际 usage 可能高于请求预留，结算后按实际合计记录，即使超过原预算也不会篡改为预估值。请求记录仅保存项目、Key 标识、模型、状态、预留与 usage 元数据，不保存 Prompt 或回复正文。

管理页面复用同一套 Rust Leptos 组件：服务端先渲染 HTML，浏览器加载生成的 JavaScript 引导代码与 WASM，再为现有页面接入事件和响应式状态。没有独立的 Node 前端栈。页面包括模型目录、路由策略、分页请求记录和 API Key 管理，主题跟随系统浅色或深色偏好。

布局、表单字段、提交状态和提示是共享组件。Leptos Signal 管理策略草稿、修改状态、即时校验和重置；官方 `ServerAction` / `ActionForm` 提交登录、保存和退出。表格复用 `leptos-struct-table` 的公开 `TableRow` 派生与行渲染接口，表头、语义化表格和分页由页面组件组织，可在 SSR 时直接输出行内容，不依赖浏览器虚拟列表完成首屏。

`gateway-validation` 使用 [Garde 0.23.0](https://docs.rs/garde/0.23.0/garde/) 的派生宏与 regex 规则定义 `Credentials`、`PolicyDraft` 基础校验，供 SSR、WASM、应用配置及存储入口共享；授权、CSRF、`PolicySnapshot` 语义校验与 CAS 仍由服务端执行。

浏览器校验只提供交互反馈。敏感读取、密码验证、超级管理员角色、会话有效期、Origin 和会话绑定 CSRF 仍在服务端核验。登录会话使用带期限的 HttpOnly / SameSite=Strict Cookie；策略保存与退出同时验证来源和 CSRF。策略更新校验完整文档并比较旧 revision，保存成功后发布到内存；并发编辑冲突要求刷新。模型上游凭据和密码哈希不进入浏览器 DTO。

Cookie 的解析与构造复用 `cookie` 0.18.2。服务端检查所有 Cookie 请求头，拒绝重复的会话或 CSRF Cookie，以及格式无效的值；库解析不代替 Origin、角色、有效期和 CSRF 校验。

登录尝试由 `governor` 0.10.4 的直接 GCRA 限流器按当前进程共用一份额度：速率为每分钟 10 次，可突发 10 次，每 6 秒恢复一次机会，不按固定 60 秒窗口重置。它不根据客户端 IP 或转发头划分额度。额度不足时，响应携带向上取整为秒且至少为 1 的动态 `Retry-After`；增强表单收到 429，原生 HTML 表单重定向回登录页。

模型目录来自部署配置，启动时保存不可变版本快照；管理页面不提供上游凭据编辑。当前提供单 Key 的累计 Token 预算，不提供项目级共享预算或货币计费；也不包含 Provider 账户管理或公网部署。`Gateway::observations()` 还提供进程内指标：成功终态中有 usage 和没有 usage 的请求数，以及输入、输出、缓存输入和推理 Token 累计值。这些 usage 计数和 Token 累计值采用饱和加法，只有终态审计写入成功后才更新；缺失 usage 与实际 0 可区分。进程重启会清零这些内存指标，持久化请求审计记录不受影响。共享日志组件将结构化日志滚动保存在数据目录。

## 验证

Gateway 后端测试使用本地合成上游，覆盖 JSON/SSE usage 解析与原始字节保留、Token 预算预留和结算、端点选择及冷却、认证和范围；不调用付费模型。SSR 测试与 WASM hydrate 编译检查命令见[开发与验证](development.md#可复用决策与-gateway-检查)。[运行证据](runtime-evidence.md)中的截图只证明截图标注版本和页面当时的状态；本次没有新增 API Key 管理界面的运行截图。测试不代表公网、项目级共享预算或三平台运行验收。
