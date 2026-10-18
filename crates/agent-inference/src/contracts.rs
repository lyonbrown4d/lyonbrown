use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
  #[error("model unavailable: {0}")]
  ModelUnavailable(String),
  #[error("invalid inference request: {0}")]
  InvalidRequest(String),
  #[error("inference cancelled")]
  Cancelled,
  #[error("inference consumer disconnected")]
  ConsumerDisconnected,
  #[error("inference backend: {0}")]
  Backend(String),
}
pub type Result<T> = std::result::Result<T, InferenceError>;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
  pub id: String,
  pub backend: String,
  pub context_window: u32,
  pub supports_tools: bool,
  pub supports_images: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
  System,
  User,
  Assistant,
  Tool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputContent {
  Text { text: String },
  Image { mime: String, data_base64: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
  pub role: MessageRole,
  pub content: Vec<InputContent>,
  pub tool_calls: Vec<ToolCall>,
  pub tool_call_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
  pub name: String,
  pub description: String,
  pub parameters: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
  pub id: String,
  pub name: String,
  pub arguments: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRequest {
  pub messages: Vec<ChatMessage>,
  pub tools: Vec<ToolDefinition>,
  pub max_tokens: u32,
  pub temperature: Option<f64>,
  /// Zero selects the backend's default. Backends reject an explicit seed
  /// when their public SDK cannot honor it for stochastic sampling.
  pub seed: u64,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
  Stop,
  Length,
  ToolCalls,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InferenceChunk {
  Text { text: String },
  Thinking { text: String },
  ToolCall { call: ToolCall },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationSummary {
  pub reason: FinishReason,
  pub input_tokens: u64,
  pub output_tokens: u64,
}

#[async_trait]
pub trait InferenceBackend: Send + Sync {
  fn info(&self) -> ModelInfo;
  async fn generate(
    &self,
    request: GenerationRequest,
    cancellation: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> Result<GenerationSummary>;
}
#[cfg(any(feature = "llama", feature = "mistral"))]
pub(crate) async fn emit(
  output: &mpsc::Sender<InferenceChunk>,
  cancellation: &CancellationToken,
  chunk: InferenceChunk,
) -> Result<()> {
  tokio::select! {
    biased;
    _=cancellation.cancelled()=>Err(InferenceError::Cancelled),
    result=output.send(chunk)=>result.map_err(|_|InferenceError::ConsumerDisconnected),
  }
}
pub(crate) fn validate(info: &ModelInfo, request: &GenerationRequest) -> Result<()> {
  if request.messages.is_empty()
    || request.max_tokens == 0
    || request.max_tokens >= info.context_window
  {
    return Err(InferenceError::InvalidRequest(
      "messages and valid output token limit are required".into(),
    ));
  }
  if request
    .temperature
    .is_some_and(|value| !value.is_finite() || value < 0.0)
  {
    return Err(InferenceError::InvalidRequest("invalid temperature".into()));
  }
  if !info.supports_tools
    && (!request.tools.is_empty()
      || request
        .messages
        .iter()
        .any(|message| !message.tool_calls.is_empty() || message.role == MessageRole::Tool))
  {
    return Err(InferenceError::InvalidRequest(
      "loaded model does not expose tool calling".into(),
    ));
  }
  if !info.supports_images
    && request.messages.iter().any(|message| {
      message
        .content
        .iter()
        .any(|part| matches!(part, InputContent::Image { .. }))
    })
  {
    return Err(InferenceError::InvalidRequest(
      "loaded model does not expose image input".into(),
    ));
  }
  Ok(())
}
