use std::collections::HashMap;
use std::pin::Pin;

use agent_core::{StopReason, TokenUsage, ToolCallId, ToolInvocation};
use agent_model::{ModelError, ModelEvent};
use eventsource_stream::{EventStreamError, Eventsource};
use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

struct OpenToolBlock {
  id: ToolCallId,
  name: String,
  arguments: String,
}

pub(crate) fn into_model_stream(
  response: reqwest::Response,
) -> Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send>> {
  let (sender, receiver) = mpsc::channel(16);
  tokio::spawn(async move {
    let mut input_tokens = 0_u32;
    let mut cache_read = 0_u32;
    let mut output_tokens = 0_u32;
    let mut stop_reason: Option<StopReason> = None;
    let mut open_blocks: HashMap<usize, OpenToolBlock> = HashMap::new();
    let mut events = response.bytes_stream().eventsource();
    while let Some(result) = events.next().await {
      let event = match result {
        Ok(event) => event,
        Err(EventStreamError::Transport(error)) => {
          let _ = sender
            .send(Err(crate::client::map_reqwest_error(error)))
            .await;
          return;
        }
        Err(error) => {
          let _ = sender
            .send(Err(ModelError::Protocol(format!(
              "invalid Anthropic SSE stream: {error}"
            ))))
            .await;
          return;
        }
      };
      if emit_sse_event(
        &sender,
        &event.data,
        &mut input_tokens,
        &mut cache_read,
        &mut output_tokens,
        &mut stop_reason,
        &mut open_blocks,
      )
      .await
      .is_err()
      {
        return;
      }
    }
  });
  Box::pin(ReceiverStream::new(receiver))
}

async fn emit_sse_event(
  sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
  data: &str,
  input_tokens: &mut u32,
  cache_read: &mut u32,
  output_tokens: &mut u32,
  stop_reason: &mut Option<StopReason>,
  open_blocks: &mut HashMap<usize, OpenToolBlock>,
) -> Result<(), ()> {
  if data.is_empty() {
    return Ok(());
  }
  let event: Value = match serde_json::from_str(data) {
    Ok(event) => event,
    Err(error) => {
      return send_error(
        sender,
        ModelError::Protocol(format!("invalid Anthropic SSE payload: {error}")),
      )
      .await;
    }
  };
  match event.get("type").and_then(Value::as_str) {
    Some("ping") => Ok(()),
    Some("message_start") => {
      *input_tokens = event
        .pointer("/message/usage/input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
      *cache_read = event
        .pointer("/message/usage/cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
      Ok(())
    }
    Some("content_block_start") => {
      let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
      let block = event.get("content_block");
      if block
        .and_then(|block| block.get("type"))
        .and_then(Value::as_str)
        == Some("tool_use")
      {
        open_blocks.insert(
          index,
          OpenToolBlock {
            id: ToolCallId::new(),
            name: block
              .and_then(|block| block.get("name"))
              .and_then(Value::as_str)
              .unwrap_or_default()
              .to_owned(),
            arguments: String::new(),
          },
        );
      }
      Ok(())
    }
    Some("content_block_delta") => {
      let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
      let delta = event.get("delta");
      match delta
        .and_then(|delta| delta.get("type"))
        .and_then(Value::as_str)
      {
        Some("text_delta") => {
          let text = delta
            .and_then(|delta| delta.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default();
          if !text.is_empty() {
            send_event(sender, ModelEvent::Delta(text.to_owned())).await?;
          }
        }
        Some("input_json_delta") => {
          if let Some(block) = open_blocks.get_mut(&index) {
            block.arguments.push_str(
              delta
                .and_then(|delta| delta.get("partial_json"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
            );
          }
        }
        _ => {}
      }
      Ok(())
    }
    Some("content_block_stop") => {
      let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
      if let Some(block) = open_blocks.remove(&index) {
        let arguments = if block.arguments.trim().is_empty() {
          json!({})
        } else {
          match serde_json::from_str(&block.arguments) {
            Ok(value) => value,
            Err(error) => {
              return send_error(
                sender,
                ModelError::Protocol(format!("invalid Anthropic tool arguments: {error}")),
              )
              .await;
            }
          }
        };
        send_event(
          sender,
          ModelEvent::ToolCall(ToolInvocation {
            id: block.id,
            name: block.name,
            arguments,
          }),
        )
        .await?;
      }
      Ok(())
    }
    Some("message_delta") => {
      *output_tokens = event
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
      if let Some(reason) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
        *stop_reason = Some(map_stop_reason(reason));
      }
      Ok(())
    }
    Some("message_stop") => {
      send_event(
        sender,
        ModelEvent::Usage(TokenUsage {
          input: *input_tokens,
          output: *output_tokens,
          cache_read: *cache_read,
        }),
      )
      .await?;
      send_event(
        sender,
        ModelEvent::Done {
          stop_reason: stop_reason.clone().unwrap_or(StopReason::EndTurn),
        },
      )
      .await?;
      Ok(())
    }
    Some("error") => {
      let message = event
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("unknown Anthropic error");
      send_error(
        sender,
        ModelError::Protocol(format!("Anthropic error: {message}")),
      )
      .await
    }
    Some(other) => {
      send_error(
        sender,
        ModelError::Protocol(format!("unexpected Anthropic SSE event: {other}")),
      )
      .await
    }
    None => {
      send_error(
        sender,
        ModelError::Protocol("Anthropic SSE event missing type".into()),
      )
      .await
    }
  }
}

fn map_stop_reason(reason: &str) -> StopReason {
  match reason {
    "tool_use" => StopReason::ToolUse,
    "max_tokens" => StopReason::MaxTokens,
    _ => StopReason::EndTurn,
  }
}

async fn send_event(
  sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
  event: ModelEvent,
) -> Result<(), ()> {
  sender.send(Ok(event)).await.map_err(|_| ())
}

async fn send_error(
  sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
  error: ModelError,
) -> Result<(), ()> {
  sender.send(Err(error)).await.map_err(|_| ())
}
