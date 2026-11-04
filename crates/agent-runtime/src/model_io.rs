use crate::{Result, Runtime, RuntimeError, RuntimeEvent, TokenUsage, persistence::active};
use agent_core::events::{AgentEvent, ExecutionPhase};
use futures::StreamExt;
use rig_agent::run::{StreamedTurnAssembler, StreamedTurnEvent, prepare_request};
use rig_core::{
  completion::{CompletionRequest, Usage},
  message::Message,
  streaming::{Item, StreamEvent},
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
impl Runtime {
  pub(crate) async fn call_model(
    &self,
    _prompt: Arc<Message>,
    _history: Vec<Arc<Message>>,
    turn: usize,
    token: &CancellationToken,
  ) -> Result<bool> {
    let mut messages = self.prepare_context(token).await?;
    let prompt = Arc::new(
      messages
        .pop()
        .ok_or_else(|| RuntimeError::Execution("model request has no prompt".into()))?,
    );
    let history = messages.into_iter().map(Arc::new).collect::<Vec<_>>();
    if token.is_cancelled() {
      return Ok(false);
    }
    let prepared = self
      .session
      .update("model_request_started", |state| {
        let active = active(state)?;
        let prepared = prepare_request(
          &active.spec,
          &self.model.capabilities().completion,
          &history,
          self.tool_definitions()?,
          active.run.output_tool_name(),
          None,
        )
        .map_err(model_error)?;
        active.run.advertise_tools(turn, prepared.tools.clone());
        active.prepared = Some(prepared.clone());
        active.in_flight = Some("model".into());
        Ok((
          prepared,
          json!({"run_id":active.id,"turn":turn}),
          vec![AgentEvent::PhaseChanged {
            phase: ExecutionPhase::ConnectingModel,
            model: None,
          }],
        ))
      })
      .await?;
    (self.listener)(RuntimeEvent::ModelRequestStarted);
    let request = prepared.clone().apply(CompletionRequest::new(prompt));
    let mut stream = self.model.stream(request).map_err(|_| unknown_model())?;
    let mut assembler = StreamedTurnAssembler::new(prepared.policy);
    let deadline = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(deadline);
    let mut started = false;
    let mut text_delta = String::new();
    let mut reasoning_delta = String::new();
    let mut received_bytes = 0_usize;
    loop {
      let item = tokio::select! { biased;
        _ = self.session.stopped.cancelled() => return Err(RuntimeError::Uncertain("session writer fenced".into())),
        _ = token.cancelled() => return Err(unknown_model()),
        _ = &mut deadline => return Err(unknown_model()),
        item = stream.next() => item,
      };
      let Some(item) = item else {
        break;
      };
      let item = item.map_err(|_| unknown_model())?;
      let remaining = crate::budget::MODEL_BYTES.saturating_sub(received_bytes);
      received_bytes += crate::budget::size(&item, remaining).map_err(|_| unknown_model())?;
      if !started {
        (self.listener)(RuntimeEvent::ModelResponseStarted);
        started = true;
      }
      match assembler.ingest(&item).map_err(|_| unknown_model())? {
        StreamedTurnEvent::InvalidToolCall(_) => {
          return Err(unknown_model());
        }
        StreamedTurnEvent::EmitIngested => match &item {
          Item::Event(StreamEvent::Text { text, .. }) => text_delta.push_str(text),
          Item::Event(StreamEvent::Reasoning { text, .. }) => reasoning_delta.push_str(text),
          _ => {}
        },
        _ => {}
      }
      if text_delta.len().saturating_add(reasoning_delta.len()) > agent_privacy::MAX_TEXT_BYTES {
        return Err(unknown_model());
      }
    }
    let response = tokio::select! { biased;
      _ = self.session.stopped.cancelled() => return Err(unknown_model()),
      _ = token.cancelled() => return Err(unknown_model()),
      _ = &mut deadline => return Err(unknown_model()),
      result = stream.finish() => result.map_err(|_| unknown_model())?,
    };
    crate::budget::check(&response.choice, crate::budget::MODEL_BYTES)
      .map_err(|_| unknown_model())?;
    let usage = response.usage;
    let turn = assembler.finish(&response);
    let validation = async {
      self.validate_model_value(&response.raw).await?;
      let turn_value = serde_json::to_value(&turn).map_err(model_error)?;
      self.validate_model_value(&turn_value).await?;
      self
        .validate_model_value(&serde_json::json!([text_delta, reasoning_delta]))
        .await
    }
    .await;
    if let Err(error) = validation {
      // A complete provider response was observed, but no unsafe content may
      // enter Rig state or presentation. Record only the known rejection.
      self
        .session
        .update("model_response_rejected", |state| {
          let active = active(state)?;
          active.in_flight = None;
          active.prepared = None;
          Ok(((), json!({"outcome":"rejected"}), vec![]))
        })
        .await
        .map_err(|_| unknown_model())?;
      return Err(error);
    }
    if !text_delta.is_empty() {
      (self.listener)(RuntimeEvent::TextDelta { text: text_delta });
    }
    if !reasoning_delta.is_empty() {
      (self.listener)(RuntimeEvent::ThinkingDelta {
        text: reasoning_delta,
      });
    }
    self
      .session
      .update("model_response_committed", |state| {
        let active = active(state)?;
        active
          .run
          .record_streamed_completion_call(
            usage,
            response.identity(),
            response.finish_reason(),
            response.raw,
          )
          .map_err(model_error)?;
        active.run.streamed_turn(turn).map_err(model_error)?;
        active.in_flight = None;
        active.prepared = None;
        active.usage = runtime_usage(usage);
        let value = &active.usage;
        Ok((
          (),
          json!({"run_id":active.id}),
          vec![AgentEvent::UsageUpdated {
            entry_id: format!("{}:{}:{}", active.id, active.segment, active.run.turn()),
            usage: agent_core::events::TokenUsage {
              input: value.input_tokens,
              output: value.output_tokens,
              cache_read: value.cache_read_tokens,
              cache_write: value.cache_write_tokens,
              total: value.total_tokens,
            },
          }],
        ))
      })
      .await
      .map_err(|_| unknown_model())?;
    Ok(true)
  }
}
fn runtime_usage(usage: Usage) -> TokenUsage {
  TokenUsage {
    input_tokens: usage.input_tokens.unwrap_or_default(),
    output_tokens: usage.output_tokens.unwrap_or_default(),
    cache_read_tokens: usage.cached_input_tokens.unwrap_or_default(),
    cache_write_tokens: usage.cache_creation_input_tokens.unwrap_or_default(),
    total_tokens: usage.total_tokens.unwrap_or_default(),
  }
}

fn model_error(_: impl std::fmt::Display) -> RuntimeError {
  RuntimeError::Execution("model request did not complete successfully".into())
}

fn unknown_model() -> RuntimeError {
  RuntimeError::Uncertain(
    "model IO did not durably finish; retained request intent requires reconciliation".into(),
  )
}
