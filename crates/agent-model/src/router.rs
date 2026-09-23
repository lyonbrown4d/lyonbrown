use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;

use crate::{ChatMessage, Complexity, ModelEvent, ModelProvider, ModelRequest, Role, StepContext};

const ROUTER_SYSTEM_PROMPT: &str = "You are a task router. Classify the user's request by the capability required from the execution model. Reply with exactly one token: LOW for short conversational or formatting work, MEDIUM for bounded analysis, or HIGH for coding, tools, long context, planning, debugging, or multi-step work.";

#[async_trait]
pub trait TaskRouter: Send + Sync {
  async fn classify(&self, context: &StepContext) -> Result<Complexity, TaskRouterError>;
}

pub struct ModelTaskRouter {
  provider: Arc<dyn ModelProvider>,
  model: String,
}

impl ModelTaskRouter {
  pub fn new(provider: Arc<dyn ModelProvider>, model: impl Into<String>) -> Self {
    Self {
      provider,
      model: model.into(),
    }
  }
}

#[async_trait]
impl TaskRouter for ModelTaskRouter {
  async fn classify(&self, context: &StepContext) -> Result<Complexity, TaskRouterError> {
    let request = ModelRequest {
      model: self.model.clone(),
      system: Some(ROUTER_SYSTEM_PROMPT.into()),
      messages: vec![ChatMessage {
        role: Role::User,
        content: context.prompt.clone(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: Vec::new(),
      }],
      tools: Vec::new(),
      max_tokens: 8,
      temperature: Some(0.0),
      json_mode: false,
      thinking: Some(false),
      stop: vec!["\n".into()],
    };
    let mut stream = self.provider.stream(request).await?.events;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
      if let ModelEvent::Delta(delta) = event? {
        output.push_str(&delta);
      }
    }
    parse_complexity(&output)
  }
}

fn parse_complexity(output: &str) -> Result<Complexity, TaskRouterError> {
  match output.trim().to_ascii_uppercase().as_str() {
    "LOW" => Ok(Complexity::Low),
    "MEDIUM" => Ok(Complexity::Medium),
    "HIGH" => Ok(Complexity::High),
    other => Err(TaskRouterError::InvalidDecision(other.into())),
  }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskRouterError {
  #[error(transparent)]
  Model(#[from] crate::ModelError),
  #[error("router must return LOW, MEDIUM, or HIGH; received '{0}'")]
  InvalidDecision(String),
}
