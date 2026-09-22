//! ModelProvider：供应商中立接缝（§4.1）。所有 provider 与 mock 都实现它。

use std::pin::Pin;

use agent_core::{ModelCapabilities, StopReason, TokenUsage, ToolCallId, ToolDefinition};
use async_trait::async_trait;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 一条对话消息。工具结果用 `Role::Tool` + `tool_call_id` 回喂。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub tool_call_id: Option<ToolCallId>,
    pub tool_name: Option<String>,
    /// assistant 消息携带的 tool call（provider 提议）。
    pub tool_calls: Vec<agent_core::ToolInvocation>,
}

/// 发给 provider 的请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub json_mode: bool,
    /// qwen worker MVP 传 `Some(false)` 禁用 thinking。
    pub thinking: Option<bool>,
    pub stop: Vec<String>,
}

/// provider 归一化后的流式事件。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelEvent {
    Delta(String),
    ToolCall(agent_core::ToolInvocation),
    Usage(TokenUsage),
    Done { stop_reason: StopReason },
}

/// 流式响应。
pub struct ModelStream {
    pub model: String,
    pub events: Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send>>,
}

/// provider 错误。
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("connection: {0}")]
    Connection(String),
    #[error("http {0}: {1}")]
    Http(u16, String),
    #[error("timeout")]
    Timeout,
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("other: {0}")]
    Other(String),
}

impl From<agent_core::AgentError> for ModelError {
    fn from(e: agent_core::AgentError) -> Self {
        Self::Other(e.to_string())
    }
}

/// 供应商中立接缝。禁止写死 provider enum（§4.1）。
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError>;
    async fn stream(&self, req: ModelRequest) -> Result<ModelStream, ModelError>;
}
