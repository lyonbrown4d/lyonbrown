use std::collections::BTreeMap;

use futures::{Stream, StreamExt};
use mistralrs::{Response, ToolCallResponse};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{FinishReason, GenerationSummary, InferenceChunk, InferenceError, Result, ToolCall};

type Calls = BTreeMap<usize, (String, String, String)>;

pub(crate) async fn collect(
  mut stream: impl Stream<Item = Response> + Unpin,
  cancellation: CancellationToken,
  output: mpsc::Sender<InferenceChunk>,
) -> Result<GenerationSummary> {
  let mut summary = GenerationSummary {
    reason: FinishReason::Stop,
    input_tokens: 0,
    output_tokens: 0,
  };
  let mut calls = Calls::new();
  let mut terminal = false;
  let mut chunk_seen = false;
  loop {
    let response = tokio::select! {biased;_=cancellation.cancelled()=>return Err(InferenceError::Cancelled),response=stream.next()=>response};
    match response {
      Some(Response::Chunk(chunk)) => {
        chunk_seen = true;
        if chunk.choices.len() > 1 || chunk.choices.iter().any(|choice| choice.index != 0) {
          return Err(error("unexpected native inference choice"));
        }
        if let Some(usage) = chunk.usage {
          summary.input_tokens = usage.prompt_tokens as u64;
          summary.output_tokens = usage.completion_tokens as u64;
        }
        for choice in chunk.choices {
          emit_text(
            &output,
            &cancellation,
            choice.delta.content,
            choice.delta.reasoning_content,
          )
          .await?;
          append_calls(&mut calls, choice.delta.tool_calls.unwrap_or_default());
          if let Some(reason) = choice.finish_reason {
            terminal = true;
            summary.reason = finish(&reason)?;
          }
        }
        // A terminal choice completes generation even if the SDK retains its sender.
        if terminal {
          break;
        }
      }
      Some(Response::Done(response)) if !chunk_seen => {
        if response.choices.len() != 1 || response.choices[0].index != 0 {
          return Err(error("unexpected native inference choice"));
        }
        summary.input_tokens = response.usage.prompt_tokens as u64;
        summary.output_tokens = response.usage.completion_tokens as u64;
        for choice in response.choices {
          summary.reason = finish(&choice.finish_reason)?;
          emit_text(
            &output,
            &cancellation,
            choice.message.content,
            choice.message.reasoning_content,
          )
          .await?;
          append_calls(&mut calls, choice.message.tool_calls.unwrap_or_default());
        }
        terminal = true;
        break;
      }
      Some(Response::InternalError(message) | Response::ValidationError(message)) => {
        return Err(error(message));
      }
      Some(Response::ModelError(message, _)) => return Err(error(message)),
      None => break,
      _ => return Err(error("unexpected response from chat inference")),
    }
  }
  if !terminal {
    return Err(error("model stream ended without a terminal response"));
  }
  if summary.reason == FinishReason::Length && !calls.is_empty() {
    return Err(error("truncated native tool response"));
  }
  if summary.reason == FinishReason::ToolCalls && calls.is_empty() {
    return Err(error("terminal native tool response has no calls"));
  }
  if !calls.is_empty() {
    summary.reason = FinishReason::ToolCalls;
  }
  for (_, (id, name, arguments)) in calls {
    if id.is_empty() || name.is_empty() {
      return Err(error("incomplete native tool identity"));
    }
    let arguments = serde_json::from_str(&arguments).map_err(error)?;
    crate::contracts::emit(
      &output,
      &cancellation,
      InferenceChunk::ToolCall {
        call: ToolCall {
          id,
          name,
          arguments,
        },
      },
    )
    .await?;
  }
  Ok(summary)
}

fn append_calls(calls: &mut Calls, values: Vec<ToolCallResponse>) {
  for call in values {
    let entry = calls.entry(call.index).or_default();
    if !call.id.is_empty() {
      entry.0 = call.id;
    }
    if entry.1 != call.function.name {
      entry.1.push_str(&call.function.name);
    }
    entry.2.push_str(&call.function.arguments);
  }
}

fn finish(reason: &str) -> Result<FinishReason> {
  match reason {
    "tool_calls" => Ok(FinishReason::ToolCalls),
    "length" => Ok(FinishReason::Length),
    "stop" => Ok(FinishReason::Stop),
    "canceled" | "cancelled" => Err(InferenceError::Cancelled),
    _ => Err(error("unknown native inference finish reason")),
  }
}

async fn emit_text(
  output: &mpsc::Sender<InferenceChunk>,
  cancellation: &CancellationToken,
  text: Option<String>,
  thinking: Option<String>,
) -> Result<()> {
  if let Some(text) = text {
    crate::contracts::emit(output, cancellation, InferenceChunk::Text { text }).await?;
  }
  if let Some(text) = thinking {
    crate::contracts::emit(output, cancellation, InferenceChunk::Thinking { text }).await?;
  }
  Ok(())
}

fn error(error: impl std::fmt::Display) -> InferenceError {
  InferenceError::Backend(error.to_string())
}
