use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
  #[error("runtime configuration: {0}")]
  Configuration(String),
  #[error("session persistence: {0}")]
  Persistence(String),
  #[error("harness execution: {0}")]
  Harness(String),
  #[error("tool execution: {0}")]
  Tool(String),
  #[error("session has an unresolved execution outcome: {0}")]
  Uncertain(String),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Secrets are supplied by the daemon and never serialized into session state.
#[derive(Clone)]
pub struct RuntimeConfig {
  pub session_id: String,
  pub owner_epoch: u64,
  pub provider: String,
  pub model: String,
  pub api_key: String,
  pub base_url: Option<String>,
  pub system_prompt: Option<String>,
  pub context_window: u32,
  pub max_output_tokens: u32,
  pub supports_images: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMutation {
  pub kind: String,
  pub payload: Value,
}

/// Implemented by the daemon's transactional store. A successful commit must
/// atomically fence the owner epoch and write snapshot, event and outbox.
/// Errors must mean no state was committed; ambiguous storage outcomes must
/// fence the writer until the store reconciles them.
#[async_trait]
pub trait SessionPersistence: Send + Sync {
  async fn load(&self, session_id: &str) -> Result<Option<Vec<u8>>>;
  async fn commit(
    &self,
    session_id: &str,
    owner_epoch: u64,
    snapshot: Vec<u8>,
    mutation: SessionMutation,
  ) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
  pub name: String,
  pub description: String,
  pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
  pub id: String,
  pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
  pub text: String,
  pub is_error: bool,
}

/// Authorization, workspace confinement and side effects belong to the host.
/// The harness durably records tool intent before calling this interface.
#[async_trait]
pub trait RuntimeTool: Send + Sync {
  fn definition(&self) -> ToolDefinition;
  async fn execute(&self, call: ToolCall, cancellation: CancellationToken) -> Result<ToolOutput>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunOutcome {
  Completed,
  Cancelled,
  Failed { message: String },
  Suspended { id: String },
  Uncertain { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeResult {
  pub run_id: String,
  pub outcome: RunOutcome,
  pub text: String,
  /// Counters reported for the final assistant turn, excluding earlier tool turns.
  pub usage: TokenUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
  pub input_tokens: u64,
  pub output_tokens: u64,
  pub cache_read_tokens: u64,
  pub cache_write_tokens: u64,
  pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
  RunStarted {
    run_id: String,
  },
  TextDelta {
    text: String,
  },
  ThinkingDelta {
    text: String,
  },
  ToolStarted {
    call_id: String,
    name: String,
    arguments: Value,
  },
  ToolFinished {
    call_id: String,
    name: String,
    text: String,
    is_error: bool,
  },
  RunFinished {
    run_id: String,
    outcome: String,
  },
  Compaction {
    active: bool,
  },
}

pub type EventListener = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeImage {
  pub mime_type: String,
  pub data_base64: String,
}
