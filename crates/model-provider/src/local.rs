use crate::ProviderError;
use agent_inference::{
  ChatMessage, FinishReason, GenerationRequest, InferenceBackend, InferenceChunk, InputContent,
  MessageRole,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub async fn classify_local_json(
  backend: &Arc<dyn InferenceBackend>,
  system_prompt: String,
  user_prompt: String,
  cancellation: CancellationToken,
) -> crate::Result<String> {
  struct Input {
    system_prompt: String,
    user_prompt: String,
  }
  let request = Input {
    system_prompt,
    user_prompt,
  };
  let stop = cancellation.child_token();
  let _cancel_on_drop = stop.clone().drop_guard();
  let message = |role, text| ChatMessage {
    role,
    content: vec![InputContent::Text { text }],
    tool_calls: Vec::new(),
    tool_call_id: None,
  };
  let context = backend.info().context_window;
  if context < 2 {
    return Err(ProviderError::Request(
      "local classification unavailable".into(),
    ));
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
  let generate = backend.generate(request, stop.clone(), send);
  tokio::pin!(generate);
  let mut text = String::new();
  let mut total = 0usize;
  let mut summary = None;
  let mut closed = false;
  loop {
    tokio::select! {
      biased;
      _=stop.cancelled()=>return Err(ProviderError::Request("local classification unavailable".into())),
      chunk=receive.recv(), if !closed=>match chunk {
        Some(InferenceChunk::Text { text: delta })=>{ total=total.saturating_add(delta.len()); text.push_str(&delta); },
        Some(InferenceChunk::Thinking { text })=>total=total.saturating_add(text.len()),
        Some(InferenceChunk::ToolCall { .. })=>return Err(ProviderError::Request("local classification unavailable".into())),
        None=>closed=true,
      },
      result=&mut generate, if summary.is_none()=>{ summary=Some(result.map_err(|_| ProviderError::Request("local classification unavailable".into()))?); },
    }
    if total > crate::CLASSIFICATION_OUTPUT_LIMIT {
      return Err(ProviderError::Request(
        "local classification unavailable".into(),
      ));
    }
    if closed && let Some(summary) = &summary {
      return if summary.reason == FinishReason::Stop && !text.trim().is_empty() {
        Ok(text)
      } else {
        Err(ProviderError::Request(
          "local classification unavailable".into(),
        ))
      };
    }
  }
}
