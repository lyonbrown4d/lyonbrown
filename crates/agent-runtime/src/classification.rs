//! One bounded inference request for routing. This never creates an agent loop.
use rpi_ai::{
  AssistantMessageEvent as Event, Content, Context, DoneReason, SimpleStreamOptions, UserMessage,
};
use tokio_util::sync::CancellationToken;

use crate::{Result, RuntimeConfig, RuntimeError};

pub const CLASSIFICATION_OUTPUT_LIMIT: usize = 16 * 1024;

/// Uses the same configured protocol adapter as the main model, without tools,
/// session mutation, retries or a Harness. The caller owns the routing deadline.
pub async fn classify_json(
  config: &RuntimeConfig,
  prompt: &str,
  cancellation: CancellationToken,
) -> Result<String> {
  let stop = cancellation.child_token();
  let _cancel_on_drop = stop.clone().drop_guard();
  if stop.is_cancelled() {
    return Err(rejected("classification cancelled"));
  }
  let (model, provider) = crate::provider::configured(config)?;
  let context = Context {
    system_prompt: config.system_prompt.clone(),
    ..Context::new(vec![UserMessage::new(prompt, 0).into()])
  };
  let options = SimpleStreamOptions {
    api_key: Some(config.api_key.clone()),
    signal: stop.clone(),
    max_retries: Some(0),
    max_tokens: Some(u64::from(config.max_output_tokens)),
    ..Default::default()
  };
  let mut stream = tokio::select! {
    biased;
    _=stop.cancelled()=>return Err(rejected("classification cancelled")),
    result=provider.stream_simple(&model, &context, &options)=>result,
  };
  let mut streamed_bytes = 0usize;
  loop {
    let event = tokio::select! {
      biased;
      _=stop.cancelled()=>return Err(rejected("classification cancelled")),
      event=stream.next()=>event,
    };
    match event {
      Some(Event::TextDelta { delta, .. }) | Some(Event::ThinkingDelta { delta, .. }) => {
        streamed_bytes = streamed_bytes.saturating_add(delta.len());
        if streamed_bytes > CLASSIFICATION_OUTPUT_LIMIT {
          return Err(rejected("classification output exceeds limit"));
        }
      }
      Some(
        Event::ToolCallStart { .. } | Event::ToolCallDelta { .. } | Event::ToolCallEnd { .. },
      ) => {
        return Err(rejected("classifier attempted a tool call"));
      }
      Some(Event::Done {
        reason: DoneReason::Stop,
        message,
      }) => {
        let mut text = String::new();
        for content in message.content {
          match content {
            Content::Text(part) => text.push_str(&part.text),
            Content::ToolCall(_) => return Err(rejected("classifier attempted a tool call")),
            _ => {}
          }
          if text.len() > CLASSIFICATION_OUTPUT_LIMIT {
            return Err(rejected("classification output exceeds limit"));
          }
        }
        if text.trim().is_empty() {
          return Err(rejected("classifier returned no text"));
        }
        return Ok(text);
      }
      Some(Event::Done { .. } | Event::Error { .. }) | None => {
        return Err(rejected("classification did not complete successfully"));
      }
      _ => {}
    }
  }
}

fn rejected(reason: &str) -> RuntimeError {
  // Provider errors may echo credentials, prompts or private endpoints.
  RuntimeError::Harness(reason.into())
}
