//! `agent-tool` — Tool trait、registry、执行器、审批、redaction 钩子 + 内置 Shell/FS 工具（§17）。
//!
//! M1 提供 registry、带超时/取消的执行器、审批接缝和测试用 echo/read_file 工具。

mod approval;
mod builtins;
mod execute;
mod registry;
mod runtime;

pub use approval::{
    AllowAllApproval, ApprovalDecision, ApprovalPolicy, ApprovalPrompt, SessionApprovalPolicy,
};
pub use builtins::{EchoTool, ListFilesTool, ReadFileTool, ShellTool, WriteFileTool};
pub use execute::{ToolExecutionError, ToolExecutor};
pub use registry::ToolRegistry;
pub use runtime::{Tool, ToolContext, ToolError};
