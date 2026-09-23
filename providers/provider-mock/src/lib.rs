//! `provider-mock` — 确定性的脚本化 provider，用于离线 Kernel/E2E 测试。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_core::{
  Locality, ModelCapabilities, ModelTier, StopReason, TokenUsage, ToolCallId, ToolInvocation,
};
use agent_model::{ModelError, ModelEvent, ModelProvider, ModelRequest, ModelStream};
use async_trait::async_trait;
use futures::stream;
use serde_json::Value;

#[derive(Debug)]
pub enum ScriptedStep {
  Text(String),
  ToolCall { name: String, arguments: Value },
  Error(String),
  Pending,
  PendingStart,
  EmptyToolUse,
}

impl ScriptedStep {
  pub fn text(text: impl Into<String>) -> Self {
    Self::Text(text.into())
  }

  pub fn tool_call(name: impl Into<String>, arguments: Value) -> Self {
    Self::ToolCall {
      name: name.into(),
      arguments,
    }
  }

  pub fn error(message: impl Into<String>) -> Self {
    Self::Error(message.into())
  }

  pub fn pending() -> Self {
    Self::Pending
  }

  pub fn pending_start() -> Self {
    Self::PendingStart
  }

  pub fn empty_tool_use() -> Self {
    Self::EmptyToolUse
  }
}

pub struct MockProvider {
  steps: Mutex<VecDeque<ScriptedStep>>,
  requests: Mutex<Vec<ModelRequest>>,
  request_count: AtomicUsize,
  discovery_count: AtomicUsize,
  discovery_pending: bool,
}

impl MockProvider {
  pub fn scripted(steps: Vec<ScriptedStep>) -> Self {
    Self {
      steps: Mutex::new(steps.into()),
      requests: Mutex::new(Vec::new()),
      request_count: AtomicUsize::new(0),
      discovery_count: AtomicUsize::new(0),
      discovery_pending: false,
    }
  }

  pub fn pending_discovery() -> Self {
    Self {
      steps: Mutex::new(VecDeque::new()),
      requests: Mutex::new(Vec::new()),
      request_count: AtomicUsize::new(0),
      discovery_count: AtomicUsize::new(0),
      discovery_pending: true,
    }
  }

  pub fn requests(&self) -> Vec<ModelRequest> {
    self
      .requests
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .clone()
  }

  pub fn request_count(&self) -> usize {
    self.request_count.load(Ordering::Acquire)
  }

  pub fn discovery_count(&self) -> usize {
    self.discovery_count.load(Ordering::Acquire)
  }
}

#[async_trait]
impl ModelProvider for MockProvider {
  fn provider_id(&self) -> &'static str {
    "mock"
  }

  async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError> {
    self.discovery_count.fetch_add(1, Ordering::Release);
    if self.discovery_pending {
      std::future::pending::<()>().await;
    }
    Ok(vec![ModelCapabilities {
      provider: "mock".into(),
      model: "mock-model".into(),
      tier: ModelTier::Worker,
      locality: Locality::Local,
      context_window: 32_768,
      max_output_tokens: 4_096,
      supports_tools: true,
      supports_streaming: true,
      supports_json: true,
      supports_thinking: false,
      cost_per_mtok_in: 0.0,
      cost_per_mtok_out: 0.0,
    }])
  }

  async fn stream(&self, request: ModelRequest) -> Result<ModelStream, ModelError> {
    self
      .requests
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .push(request);
    self.request_count.fetch_add(1, Ordering::Release);

    let step = self
      .steps
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .pop_front()
      .ok_or_else(|| ModelError::Protocol("mock script exhausted".into()))?;

    let events = match step {
      ScriptedStep::Text(text) => Box::pin(stream::iter(vec![
        Ok(ModelEvent::Delta(text)),
        Ok(ModelEvent::Usage(TokenUsage::default())),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::EndTurn,
        }),
      ])) as _,
      ScriptedStep::ToolCall { name, arguments } => {
        let invocation = ToolInvocation {
          id: ToolCallId::new(),
          name,
          arguments,
        };
        Box::pin(stream::iter(vec![
          Ok(ModelEvent::Delta(String::new())),
          Ok(ModelEvent::ToolCall(invocation)),
          Ok(ModelEvent::Usage(TokenUsage::default())),
          Ok(ModelEvent::Done {
            stop_reason: StopReason::ToolUse,
          }),
        ])) as _
      }
      ScriptedStep::Error(message) => return Err(ModelError::Other(message)),
      ScriptedStep::Pending => Box::pin(stream::pending()) as _,
      ScriptedStep::PendingStart => {
        std::future::pending::<()>().await;
        unreachable!("pending future never completes")
      }
      ScriptedStep::EmptyToolUse => Box::pin(stream::iter(vec![
        Ok(ModelEvent::Delta(String::new())),
        Ok(ModelEvent::Usage(TokenUsage::default())),
        Ok(ModelEvent::Done {
          stop_reason: StopReason::ToolUse,
        }),
      ])) as _,
    };

    Ok(ModelStream {
      model: "mock-model".into(),
      events,
    })
  }
}
