# 连接 MCP 工具

`agentd` 使用 `rmcp` 连接配置中列出的 MCP 服务，支持 stdio 和 Streamable HTTP。连接与工具目录属于 daemon；客户端负责展示审批和结果，模型不能新增服务或修改连接参数。

## 配置服务与凭证

在共享数据目录的 `config.toml` 中列出服务，并明确允许调用的工具：

```toml
[[mcp]]
id = "reference"
allowed_tools = ["lookup"]

[mcp.transport]
type = "streamable_http"
url = "https://mcp.example.com/rpc"
headers = { Authorization = "MCP_AUTHORIZATION" }
```

这里的服务地址和工具名为示例，须与实际服务相符。`headers` 的值是环境变量名。在相邻 `.env` 或进程环境中设置对应变量，内容为服务要求的完整请求头值，例如 `Bearer ...`。进程环境覆盖 `.env`；缺少引用的变量时，daemon 启动失败。凭证不进入模型工具 Schema 或审批参数。

远程 HTTP 服务必须使用 HTTPS。HTTP 仅允许回环地址；URL 不得含用户名、密码、查询串或片段。客户端不跟随重定向，也不继承代理环境变量。

本地进程使用绝对可执行路径：

```toml
[[mcp]]
id = "local-reference"
allowed_tools = ["lookup"]

[mcp.transport]
type = "stdio"
executable = "/absolute/path/to/mcp-server"
arguments = ["--stdio"]
environment = { SERVER_TOKEN = "LOCAL_MCP_TOKEN" }
```

路径示例适用于 macOS/Linux；Windows 应填写本机绝对路径，可用 TOML 单引号字符串保留反斜杠。`environment` 将子进程变量名映射到 `.env` 或进程环境中的变量名。daemon 只传递明确引用的变量和少量系统运行变量，不把模型提供方密钥或整个父进程环境传给 MCP 子进程。stderr 写入私有日志目录，按服务保留有大小限制的诊断文件。

最多配置 64 个服务，所有服务合计最多允许 64 个工具。工具必须存在于服务返回的目录中，且输入必须是受支持的、有大小限制的对象 Schema。外部 Schema 引用被拒绝。

## 从模型调用到实际执行

模型通过 `mcp_call` 调用已选工具。`call` 对象内的服务、工具和参数必须匹配目录中的同一个分支：

```json
{
  "call": {
    "server": "reference",
    "tool": "lookup",
    "arguments": { "query": "example" }
  }
}
```

参数同样是示例；实际字段由该服务的工具 Schema 决定。完整分支放在根级 `call` 属性内，使三个模型协议都能保留服务、工具与参数的对应关系。服务描述和结果均作为不可信数据处理。

daemon 先校验工具白名单和参数，再保存审批。审批绑定 Run、服务、工具、调用 ID、完整参数和有效期；批准后只消费一次，随后才派发请求。服务标注「只读」或模型给出高置信度，均不能跳过审批。未配置 MCP 时，执行计划和模型可见工具中不包含 `mcp_call`；决策分类器只能进一步收缩权限。

派发前取消不会发送工具调用。派发后取消、超时或连接丢失可能留下未知副作用，运行进入 `Uncertain`，不会自动重放。待处理审批随运行结束关闭；已经消费的审批保留原记录。恢复与核对规则见[会话、任务与恢复](persistence.md)。

MCP 返回的资源链接不自动读取；sampling 和 elicitation 不能借客户端扩大权限。输入消息、文本和图片输出均有大小限制。daemon 关闭时取消共享连接，已有 Run 的工具引用也不能再次调用。

源码：`crates/agent-mcp`、`crates/agent-config/src/mcp.rs`、`apps/agentd/src/mcp.rs` 与 `apps/agentd/src/authorization.rs`。
