use crate::{ClassificationRequest, ClassifierError, DecisionBackend};
use agent_inference::{
  ChatMessage, FinishReason, GenerationRequest, InferenceBackend, InferenceChunk, InputContent,
  MessageRole,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct RemoteClassifier {
  config: agent_runtime::RuntimeConfig,
}
impl RemoteClassifier {
  pub fn new(config: agent_runtime::RuntimeConfig) -> Self {
    Self { config }
  }
}
#[async_trait]
impl DecisionBackend for RemoteClassifier {
  async fn classify(
    &self,
    request: ClassificationRequest,
    cancellation: CancellationToken,
  ) -> Result<String, ClassifierError> {
    let mut config = self.config.clone();
    config.system_prompt = Some(request.system_prompt);
    config.max_output_tokens = config.max_output_tokens.min(2048);
    agent_runtime::classify_json(&config, &request.user_prompt, cancellation)
      .await
      .map_err(|_| ClassifierError)
  }
}

/// Local backends expose the same bounded, tool-free classification contract.
pub struct LocalClassifier {
  backend: Arc<dyn InferenceBackend>,
}
impl LocalClassifier {
  pub fn new(backend: Arc<dyn InferenceBackend>) -> Self {
    Self { backend }
  }
}
#[async_trait]
impl DecisionBackend for LocalClassifier {
  async fn classify(
    &self,
    request: ClassificationRequest,
    cancellation: CancellationToken,
  ) -> Result<String, ClassifierError> {
    let stop = cancellation.child_token();
    let _cancel_on_drop = stop.clone().drop_guard();
    let message = |role, text| ChatMessage {
      role,
      content: vec![InputContent::Text { text }],
      tool_calls: Vec::new(),
      tool_call_id: None,
    };
    let context = self.backend.info().context_window;
    if context < 2 {
      return Err(ClassifierError);
    }
    let request = GenerationRequest {
      messages: vec![
        message(MessageRole::System, request.system_prompt),
        message(MessageRole::User, request.user_prompt),
      ],
      tools: Vec::new(),
      max_tokens: 2048.min(context - 1),
      temperature: Some(0.0),
      seed: 0,
    };
    let (send, mut receive) = tokio::sync::mpsc::channel(16);
    let generate = self.backend.generate(request, stop.clone(), send);
    tokio::pin!(generate);
    let mut text = String::new();
    let mut total = 0usize;
    let mut summary = None;
    let mut closed = false;
    loop {
      tokio::select! {
        biased;
        _=stop.cancelled()=>return Err(ClassifierError),
        chunk=receive.recv(), if !closed=>match chunk {
          Some(InferenceChunk::Text { text: delta })=>{ total=total.saturating_add(delta.len()); text.push_str(&delta); },
          Some(InferenceChunk::Thinking { text })=>total=total.saturating_add(text.len()),
          Some(InferenceChunk::ToolCall { .. })=>return Err(ClassifierError),
          None=>closed=true,
        },
        result=&mut generate, if summary.is_none()=>{ summary=Some(result.map_err(|_| ClassifierError)?); },
      }
      if total > agent_runtime::CLASSIFICATION_OUTPUT_LIMIT {
        return Err(ClassifierError);
      }
      if closed && let Some(summary) = &summary {
        return if summary.reason == FinishReason::Stop && !text.trim().is_empty() {
          Ok(text)
        } else {
          Err(ClassifierError)
        };
      }
    }
  }
}
