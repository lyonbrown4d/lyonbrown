//! 模型能力描述（provider 中立）。scheduler 用它做静态过滤与选择。

use serde::{Deserialize, Serialize};

pub const MAX_MODEL_CATALOG_SIZE: usize = 128;
pub const MAX_MODEL_PROVIDER_BYTES: usize = 128;
pub const MAX_MODEL_ID_BYTES: usize = 256;

/// 三层模型（§9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelTier {
  /// 本地小模型：分类、路由、简单任务。
  Control,
  /// 本地大模型：工具调用、代码、复杂推理。
  Worker,
  /// 远程前沿模型：升级兜底。
  Frontier,
}

/// 本地 / 远程。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Locality {
  Local,
  Remote,
}

/// 一个具体模型实例的能力与成本描述。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCapabilities {
  pub provider: String,
  pub model: String,
  pub tier: ModelTier,
  pub locality: Locality,
  pub context_window: u32,
  pub max_output_tokens: u32,
  pub supports_tools: bool,
  pub supports_streaming: bool,
  pub supports_json: bool,
  pub supports_thinking: bool,
  pub cost_per_mtok_in: f64,
  pub cost_per_mtok_out: f64,
}

impl ModelCapabilities {
  /// `(provider, model)` 唯一标识。
  pub fn key(&self) -> String {
    format!("{}/{}", self.provider, self.model)
  }
}

/// token 用量（provider 回报）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
  pub input: u32,
  pub output: u32,
  pub cache_read: u32,
}

/// 模型停止原因。放在 core（而非 agent-model）是因为 `StepOutcome`（core::event）需要它，
/// 而 DAG 要求 core 是依赖终点。agent-model 再导出它。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StopReason {
  EndTurn,
  ToolUse,
  MaxTokens,
  Stop,
  Error(String),
}
