use std::{future::Future, pin::Pin, sync::Arc};

use agent_inference::{GenerationRequest, GenerationSummary, InferenceBackend, InferenceChunk};
use futures::{Stream, stream};
use rig_core::completion::{FinishReason, Usage};
use rig_core::error::ProviderError;
use rig_core::message::{CallId, ToolName};
use rig_core::operation::{Block, Completion, Finish};
use rig_core::wire::{Flow, Out, WireEvent};
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, DropGuard};

pub(crate) enum Frame {
  Chunk(InferenceChunk),
  Finish(GenerationSummary),
}

struct State {
  generate: Pin<Box<dyn Future<Output = agent_inference::Result<GenerationSummary>> + Send>>,
  receive: mpsc::Receiver<InferenceChunk>,
  summary: Option<GenerationSummary>,
  closed: bool,
  _cancel: DropGuard,
}

pub(crate) fn frames(
  backend: Arc<dyn InferenceBackend>,
  request: GenerationRequest,
) -> impl Stream<Item = Result<Frame, ProviderError>> + Send + 'static {
  let cancellation = CancellationToken::new();
  let guard = cancellation.clone().drop_guard();
  let (send, receive) = mpsc::channel(16);
  let generate = Box::pin(async move { backend.generate(request, cancellation, send).await });
  let state = State {
    generate,
    receive,
    summary: None,
    closed: false,
    _cancel: guard,
  };
  stream::unfold(Some(state), |state| async move {
    let mut state = state?;
    loop {
      if state.closed
        && let Some(summary) = state.summary.take()
      {
        return Some((Ok(Frame::Finish(summary)), None));
      }
      tokio::select! {
        chunk = state.receive.recv(), if !state.closed => match chunk {
          Some(chunk) => return Some((Ok(Frame::Chunk(chunk)), Some(state))),
          None => state.closed = true,
        },
        summary = &mut state.generate, if state.summary.is_none() => match summary {
          Ok(summary) => state.summary = Some(summary),
          Err(_) => return Some((Err(unavailable()), None)),
        },
      }
    }
  })
}

#[derive(Default)]
pub(crate) struct Decoder {
  index: usize,
  total_bytes: usize,
}

impl<'id> rig_core::wire::Decoder<'id, Completion, Frame> for Decoder {
  type Event = Frame;

  fn classify(&self, frame: Frame) -> WireEvent<Frame> {
    WireEvent::Known(frame)
  }

  fn decode(&mut self, frame: Frame, mut out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
    match frame {
      Frame::Chunk(chunk) => {
        let (block, text) = match chunk {
          InferenceChunk::Text { text } => (Block::Text, text),
          InferenceChunk::Thinking { text } => (Block::Reasoning { redacted: false }, text),
          InferenceChunk::ToolCall { call } => (
            Block::Call {
              id: CallId::from_wire(call.id),
              name: ToolName::new(call.name).map_err(|_| unavailable())?,
            },
            serde_json::to_string(&call.arguments).map_err(|_| unavailable())?,
          ),
        };
        self.total_bytes = self.total_bytes.saturating_add(text.len());
        if self.total_bytes > 16 * 1024 * 1024 {
          return Err(unavailable());
        }
        out.open(self.index, block, serde_json::Value::Null)?;
        out.push(self.index, &text)?;
        out.finish(self.index)?;
        self.index += 1;
        Ok(Flow::More)
      }
      Frame::Finish(summary) => {
        let reason = match summary.reason {
          agent_inference::FinishReason::Stop => FinishReason::Stop,
          agent_inference::FinishReason::Length => FinishReason::Length,
          agent_inference::FinishReason::ToolCalls => FinishReason::ToolCalls,
        };
        let mut usage = Usage::new();
        usage.input_tokens = Some(summary.input_tokens);
        usage.output_tokens = Some(summary.output_tokens);
        usage.total_tokens = summary.input_tokens.checked_add(summary.output_tokens);
        Ok(out.end(Finish {
          reason: Some(reason),
          usage,
          ..Finish::default()
        }))
      }
    }
  }
}

/// Local responses have no provider-native wire document. Record only their
/// execution summary; canonical content is already owned by Rig's reply fold.
#[derive(Default)]
pub(crate) struct Document(serde_json::Value);
impl rig_core::wire::document::Serves<Completion> for Document {}
impl rig_core::wire::document::Reassemble<Frame> for Document {
  fn absorb(&mut self, frame: &Frame) {
    if let Frame::Finish(summary) = frame {
      self.0 = serde_json::json!({ "summary": summary });
    }
  }
  fn finish(self) -> serde_json::Value {
    self.0
  }
}
fn unavailable() -> ProviderError {
  ProviderError::Provider("local inference unavailable".into())
}
