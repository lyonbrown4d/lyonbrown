# AI Gateway

Gateway 内嵌在 agentd 中，负责模型请求路由、访问限制和请求元数据审计，不运行 Agent Loop。项目不提供独立 Gateway 进程或 Web 管理界面。Session、工具、权限和审批仍由 Agent 负责。

嵌入式 Gateway 目前处理 OpenAI Chat Completions。Responses 和 Anthropic 保留各自的原生适配器，不自动回退到 Gateway。原生本地推理也使用独立后端，不进入 Gateway 模型池。可选 HTTP 接口只接受 Chat Completions，不接受 Responses 或 Anthropic；接口拒绝图片，也不支持音频。

## 配置

Gateway 使用 agentd 配置。`http_listen` 默认不设置，即不启动 HTTP 监听。启用时只接受非零回环地址；当前未实现外部 TLS 和远程监听。`http_token_env` 指向专用环境变量，值须为 32–512 个可打印 ASCII 字符，且不得与 daemon 凭据或 Gateway 内部凭据相同。所有 HTTP 路由，包括健康检查和 OpenAPI JSON，均要求该 Bearer 令牌。

以下示例定义两个 Chat 模型 ID、两个上游账户和一个分组。部署时替换示例 HTTPS 地址、上游模型名和环境变量值。凭据可由进程环境或配置文件相邻的 `.env` 提供，不要将凭据值写入 TOML。

```toml
[gateway]
http_listen = "127.0.0.1:8099"
http_token_env = "GATEWAY_HTTP_TOKEN"

[[gateway.models]]
id = "chat-a"
upstream_model = "model-a"
api_base = "https://account-a.example.com/v1"
api_key_env = "ACCOUNT_A_KEY"
context_tokens = 128000
capabilities = ["text", "tools"]

[[gateway.models]]
id = "chat-b"
upstream_model = "model-b"
api_base = "https://account-b.example.com/v1"
api_key_env = "ACCOUNT_B_KEY"
context_tokens = 128000
capabilities = ["text", "tools"]

[[gateway.endpoints]]
id = "account-a"
provider_id = "provider-a"
api_base = "https://account-a.example.com/v1"
api_key_env = "ACCOUNT_A_KEY"
model_ids = ["chat-a"]
priority = 0

[[gateway.endpoints]]
id = "account-b"
provider_id = "provider-b"
api_base = "https://account-b.example.com/v1"
api_key_env = "ACCOUNT_B_KEY"
model_ids = ["chat-b"]
priority = 0

[[gateway.groups]]
id = "chat-group"
models = ["chat-a", "chat-b"]
```

端点必须列出已配置的模型 ID。远程上游必须使用 HTTPS；使用 HTTP 时，上游必须是字面量回环 IP。模型和分组选择仍受服务端授权与能力检查约束，请求不能扩大这些限制。启用并通过鉴权后，可使用模型路由 `GET /v1/models`、`POST /v1/chat/completions`，以及健康检查和 `/api-doc/openapi.json`。

## 隐私与持久化

共享隐私服务在模型使用和持久化前处理用户提示与工具输出，将检测到的敏感文本替换为作用域内的引用。映射使用宿主主密钥加密后保存在会话数据库中，写入时校验 Session 的 owner epoch。映射明文上限为 16 KiB；保护、加密或持久化失败时拒绝操作，不传递原文。

启动时会登记已保存的凭据；上传和轮换的新值在可用前登记，普通会话与 HTTP 入口共享识别结果。动态已知值上限为 4096 项、累计 1 MiB。Provider 凭据覆盖或删除前，会保留去重的加密识别历史，最多 1024 项，用于重启后继续遮盖旧值，不用于上游认证；达到容量上限时拒绝变更。

Runtime 会缓冲回复文本，待校验完成后再展示，因此不会逐步显示文本。工具引用默认拒绝，只有实际解析出的目标通过授权后才能使用。文本检测不保护原生 Runtime 中的图片。嵌入请求检测到敏感文本时会拒绝处理，直至具备稳定的投影方案；当前未加载真实本地隐私模型。

Gateway 使用 Rig 的公开 HTTP 传输与 SSE 分帧能力，并限制请求、响应、并发和时间，检查终态。连接中断或缺少终态不能证明上游没有执行，不得盲目重放。审计记录保存元数据，不保存提示词或回复正文。

Runtime 和 daemon 集成测试使用本地合成上游，不调用付费模型，覆盖模型选择、可选 HTTP 入口认证和隐私映射的重启恢复。这些测试不代表真实模型、原生 GUI、三平台或公网运行验收。
