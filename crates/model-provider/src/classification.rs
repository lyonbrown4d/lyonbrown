//! One bounded inference request for routing. This never creates an agent loop.
use futures::StreamExt;
use rig_core::completion::{CompletionRequest, FinishReason};
use rig_core::message::AssistantContent;
use rig_core::streaming::{Item, PartKind, StreamEvent};
use rig_core::{DynModel, operation::Completion};
use tokio_util::sync::CancellationToken;

use crate::{ModelConfig, ProviderError, Result};

pub const CLASSIFICATION_OUTPUT_LIMIT: usize = 16 * 1024;

/// Uses Rig's configured protocol wire without tools or retries. The caller
/// owns the deadline; dropping this future also drops the provider stream.
pub async fn classify_json(
  config: &ModelConfig,
  prompt: &str,
  cancellation: CancellationToken,
) -> Result<String> {
  let model = crate::configured(config)?;
  let mut request = CompletionRequest::new(prompt).max_tokens(u64::from(config.max_output_tokens));
  if let Some(system) = &config.system_prompt {
    request = request.preamble(system);
  }
  classify(model, request, cancellation).await
}

pub(crate) async fn classify(
  model: DynModel<Completion>,
  request: CompletionRequest,
  cancellation: CancellationToken,
) -> Result<String> {
  if cancellation.is_cancelled() {
    return Err(rejected("classification cancelled"));
  }
  let mut stream = model.stream(request).map_err(|_| unavailable())?;
  let mut total = 0usize;
  loop {
    let item = tokio::select! {
      biased;
      _ = cancellation.cancelled() => return Err(rejected("classification cancelled")),
      item = stream.next() => item,
    };
    match item {
      Some(Ok(Item::Event(event))) => match event {
        StreamEvent::Text { text, .. } | StreamEvent::Reasoning { text, .. } => {
          total = total.saturating_add(text.len());
          if total > CLASSIFICATION_OUTPUT_LIMIT {
            return Err(rejected("classification output exceeds limit"));
          }
        }
        StreamEvent::Start {
          kind: PartKind::ToolCall,
          ..
        }
        | StreamEvent::Arguments { .. }
        | StreamEvent::End {
          content: AssistantContent::ToolCall(_),
          ..
        } => {
          return Err(rejected("classifier attempted a tool call"));
        }
        _ => {}
      },
      Some(Ok(Item::Unknown(_))) => {}
      Some(Err(_)) => return Err(unavailable()),
      None => break,
    }
  }
  let response = tokio::select! {
    biased;
    _ = cancellation.cancelled() => return Err(rejected("classification cancelled")),
    response = stream.finish() => response.map_err(|_| unavailable())?,
  };
  if response.finish_reason() != Some(FinishReason::Stop)
    || response.error.is_some()
    || response.aborted.is_some()
  {
    return Err(unavailable());
  }
  let mut text = String::new();
  for content in response.choice {
    match content {
      AssistantContent::Text(part) => text.push_str(&part.text),
      AssistantContent::ToolCall(_) => return Err(rejected("classifier attempted a tool call")),
      _ => {}
    }
    if text.len() > CLASSIFICATION_OUTPUT_LIMIT {
      return Err(rejected("classification output exceeds limit"));
    }
  }
  if text.trim().is_empty() {
    return Err(rejected("classifier returned no text"));
  }
  Ok(text)
}

fn unavailable() -> ProviderError {
  rejected("classification did not complete successfully")
}

fn rejected(reason: &str) -> ProviderError {
  // Provider errors may echo credentials, prompts or private endpoints.
  ProviderError::Request(reason.into())
}
