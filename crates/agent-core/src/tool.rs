//! 工具类型：定义、调用、结果。Tool trait 本体在 agent-tool（需要 async + broker）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::ToolCallId;
use crate::secret::SecretRef;

/// 风险级别，驱动审批策略（§17）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
  /// 只读，自动放行。
  Read,
  /// 写文件。
  Write,
  /// 执行命令 / 任意代码。
  Execute,
  /// 网络。
  Network,
}

/// 工具来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSource {
  BuiltIn,
  Mcp { server: String },
}

/// 工具静态定义（进 provider `tools` 参数 + 审批展示）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
  pub name: String,
  pub description: String,
  /// JSON Schema（三种 wire 格式的公分母）。
  pub input_schema: Value,
  pub source: ToolSource,
  pub risk: RiskLevel,
  /// 仅声明；执行时才 resolve（late binding，§18.2）。
  pub requires_secrets: Vec<SecretRef>,
}

/// 一次工具调用（provider 提议，kernel 执行）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
  pub id: ToolCallId,
  pub name: String,
  pub arguments: Value,
}

/// 工具执行结果。`output` 构造前必须已 redact（§23）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
  pub invocation_id: ToolCallId,
  pub ok: bool,
  pub output: String,
  pub truncated: bool,
  pub duration_ms: u64,
  pub error: Option<String>,
}
