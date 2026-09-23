//! 统一错误类型。各 crate 的具体错误（ModelError/ToolError/SecretError…）都能转成 `AgentError`。

use std::fmt;

/// 顶层错误。Kernel 与 CLI 用它表达任何环节失败。
#[derive(Debug)]
pub enum AgentError {
  /// 模型 provider 错误（连接、超时、4xx/5xx、协议解析）。
  Model(String),
  /// 工具执行错误。
  Tool(String),
  /// Secret 解析错误。
  Secret(String),
  /// 上下文 / prompt 编译错误。
  Context(String),
  /// 状态机非法转换。
  Transition(String),
  /// 循环防护触发（max steps / max escalations）。
  LimitExceeded(String),
  /// 取消。
  Cancelled,
  /// 存储错误。
  Storage(String),
  /// 其它（`anyhow` 透传）。
  Other(String),
}

impl fmt::Display for AgentError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Model(m) => write!(f, "model error: {m}"),
      Self::Tool(m) => write!(f, "tool error: {m}"),
      Self::Secret(m) => write!(f, "secret error: {m}"),
      Self::Context(m) => write!(f, "context error: {m}"),
      Self::Transition(m) => write!(f, "illegal state transition: {m}"),
      Self::LimitExceeded(m) => write!(f, "limit exceeded: {m}"),
      Self::Cancelled => write!(f, "cancelled"),
      Self::Storage(m) => write!(f, "storage error: {m}"),
      Self::Other(m) => write!(f, "{m}"),
    }
  }
}

impl std::error::Error for AgentError {}
