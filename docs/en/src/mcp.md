# Connect MCP tools

`agentd` uses `rmcp` to connect to explicitly configured MCP servers over stdio or Streamable HTTP. The daemon owns connections and cached tool catalogs. Clients present approvals and results; model input cannot add servers or alter transports.

## Servers and credentials

List servers and allowed tools in the shared data directory's `config.toml`:

```toml
[[mcp]]
id = "reference"
allowed_tools = ["lookup"]

[mcp.transport]
type = "streamable_http"
url = "https://mcp.example.com/rpc"
headers = { Authorization = "MCP_AUTHORIZATION" }
```

The endpoint and tool name are examples and must match the real server. Header values name environment variables. Set each variable in the adjacent `.env` or process environment to the complete header value required by the server, such as `Bearer ...`. Process values override dotenv values. A missing reference prevents startup. Credentials do not enter model tool schemas or approval arguments.

Remote HTTP servers require HTTPS; plain HTTP is limited to loopback addresses. URLs cannot include credentials, queries or fragments. The HTTP client neither follows redirects nor inherits proxy environment settings.

Local processes require an absolute executable path:

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

The path example is for macOS/Linux. On Windows, use a native absolute path; TOML single-quoted strings preserve backslashes. Each `environment` entry maps a child variable to a dotenv/process variable name. Only explicit references and a small set of operating-system variables reach the child, rather than the full parent environment or implicit model credentials. Bounded stderr diagnostics are retained in the private log directory.

Configuration permits at most 64 servers and 64 selected tools in total. Every allowed tool must exist in the returned catalog and provide a supported, bounded object schema. External schema references are rejected.

## Invocation and approval

Models invoke selected tools through `mcp_call`. The server, tool and arguments inside `call` must match the same catalog branch:

```json
{
  "call": {
    "server": "reference",
    "tool": "lookup",
    "arguments": { "query": "example" }
  }
}
```

These arguments are illustrative; the selected server's schema determines the real fields. Nesting the complete branch inside a root property preserves the identity/schema association through all three model protocol adapters. Server descriptions and results remain untrusted data.

The daemon validates the allowlist and schema before persisting approval. Approval binds the Run, server, tool, call ID, exact arguments and expiry; it can be consumed once, before dispatch. Read-only annotations and classifier confidence do not waive approval. Without configured MCP servers, `mcp_call` appears in neither the effective plan nor the model's tools. Classification can only reduce that authority.

Cancellation before dispatch sends no tool invocation. Cancellation, timeout or disconnection after dispatch may leave unknown side effects. Such runs become `Uncertain` and are never automatically replayed. Pending approvals close when the run ends; consumed approvals retain their record. See [Sessions, tasks and recovery](persistence.md).

Returned resource links are not fetched automatically. Sampling and elicitation cannot expand client authority. Protocol messages, text and image outputs have size limits. Daemon shutdown cancels shared connections; retained run tools cannot issue later requests.

Source: `crates/agent-mcp`, `crates/agent-config/src/mcp.rs`, `apps/agentd/src/mcp.rs` and `apps/agentd/src/authorization.rs`.
