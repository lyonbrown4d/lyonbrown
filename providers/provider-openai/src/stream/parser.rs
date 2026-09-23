use std::collections::HashMap;
use std::pin::Pin;

use agent_core::{StopReason, TokenUsage, ToolCallId, ToolInvocation};
use agent_model::{ModelError, ModelEvent};
use futures::{Stream, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::*;

struct OpenToolCall {
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
    let mut output_tokens = 0_u32;
    let mut stop_reason: Option<StopReason> = None;
    let mut open_calls: HashMap<usize, OpenToolCall> = HashMap::new();
    let mut finished = false;
    let mut bytes = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = bytes.next().await {
      match chunk {
        Ok(chunk) => {
          buffer.extend_from_slice(&chunk);
          while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<_> = buffer.drain(..=newline).collect();
            if emit_sse_line(
              &sender,
              &line[..line.len() - 1],
              &mut input_tokens,
              &mut output_tokens,
              &mut stop_reason,
              &mut open_calls,
              &mut finished,
            )
            .await
            .is_err()
            {
              return;
            }
          }
        }
        Err(error) => {
          let _ = sender
            .send(Err(crate::client::map_reqwest_error(error)))
            .await;
          return;
        }
      }
    }
    if !buffer.is_empty() {
      let _ = emit_sse_line(
        &sender,
        &buffer,
        &mut input_tokens,
        &mut output_tokens,
        &mut stop_reason,
        &mut open_calls,
        &mut finished,
      )
      .await;
    }
    if !finished {
      let _ = send_error(
        &sender,
        ModelError::Protocol("OpenAI SSE stream ended without [DONE]".into()),
      )
      .await;
    }
  });
  Box::pin(ReceiverStream::new(receiver))
}

async fn emit_sse_line(
  sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
  line: &[u8],
  input_tokens: &mut u32,
  output_tokens: &mut u32,
  stop_reason: &mut Option<StopReason>,
  open_calls: &mut HashMap<usize, OpenToolCall>,
  finished: &mut bool,
) -> Result<(), ()> {
  let line = trim_ascii_whitespace(line);
  let Some(payload) = line.strip_prefix(b"data:") else {
    return Ok(());
  };
  let payload = trim_ascii_whitespace(payload);
  if payload.is_empty() {
    return Ok(());
  }
  if payload == b"[DONE]" {
    return finish_stream(
      sender,
      *input_tokens,
      *output_tokens,
      stop_reason.clone(),
      open_calls,
    )
    .await
    .map(|()| *finished = true);
  }
  let chunk: CompletionChunk = match serde_json::from_slice(payload) {
    Ok(chunk) => chunk,
    Err(error) => {
      return send_error(
        sender,
        ModelError::Protocol(format!("invalid OpenAI SSE payload: {error}")),
      )
      .await;
    }
  };
  if let Some(usage) = &chunk.usage {
    *input_tokens = usage.prompt_tokens;
    *output_tokens = usage.completion_tokens;
  }
  for choice in chunk.choices {
    if let Some(text) = choice.delta.content.filter(|text| !text.is_empty()) {
      send_event(sender, ModelEvent::Delta(text)).await?;
    }
    for call in choice.delta.tool_calls {
      let index = call.index as usize;
      let entry = open_calls.entry(index).or_insert_with(|| OpenToolCall {
        id: ToolCallId::new(),
        name: String::new(),
        arguments: String::new(),
      });
      if let Some(function) = call.function {
        if let Some(name) = function.name {
          entry.name = name;
        }
        if let Some(arguments) = function.arguments {
          entry.arguments.push_str(&arguments);
        }
      }
    }
    if let Some(reason) = choice.finish_reason {
      *stop_reason = Some(map_finish_reason(&reason));
    }
  }
  Ok(())
}

async fn finish_stream(
  sender: &mpsc::Sender<Result<ModelEvent, ModelError>>,
  input_tokens: u32,
  output_tokens: u32,
  stop_reason: Option<StopReason>,
  open_calls: &mut HashMap<usize, OpenToolCall>,
) -> Result<(), ()> {
  let mut indices: Vec<usize> = open_calls.keys().copied().collect();
  indices.sort_unstable();
  let calls: Vec<OpenToolCall> = indices
    .into_iter()
    .filter_map(|index| open_calls.remove(&index))
    .collect();
  for call in calls {
    let arguments = if call.arguments.trim().is_empty() {
      json!({})
    } else {
      match serde_json::from_str(&call.arguments) {
        Ok(value) => value,
        Err(error) => {
          return send_error(
            sender,
            ModelError::Protocol(format!("invalid OpenAI tool arguments: {error}")),
          )
          .await;
        }
      }
    };
    send_event(
      sender,
      ModelEvent::ToolCall(ToolInvocation {
        id: call.id,
        name: call.name,
        arguments,
      }),
    )
    .await?;
  }
  send_event(
    sender,
    ModelEvent::Usage(TokenUsage {
      input: input_tokens,
      output: output_tokens,
      cache_read: 0,
    }),
  )
  .await?;
  send_event(
    sender,
    ModelEvent::Done {
      stop_reason: stop_reason.unwrap_or(StopReason::EndTurn),
    },
  )
  .await
}

fn map_finish_reason(reason: &str) -> StopReason {
  match reason {
    "tool_calls" => StopReason::ToolUse,
    "length" => StopReason::MaxTokens,
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

fn trim_ascii_whitespace(mut value: &[u8]) -> &[u8] {
  while value.first().is_some_and(u8::is_ascii_whitespace) {
    value = &value[1..];
  }
  while value.last().is_some_and(u8::is_ascii_whitespace) {
    value = &value[..value.len() - 1];
  }
  value
}
