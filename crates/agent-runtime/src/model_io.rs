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
    prompt: Arc<Message>,
    history: Vec<Arc<Message>>,
    turn: usize,
    token: &CancellationToken,
  ) -> Result<bool> {
    let prepared = self
      .session
      .update("model_request_started", |state| {
        let active = active(state)?;
        if active
          .usage
          .input_tokens
          .saturating_add(u64::from(self.config.max_output_tokens))
          >= u64::from(self.config.context_window)
        {
          return Err(RuntimeError::Configuration(
            "context budget exhausted; automatic compaction is unavailable in the Rig runtime"
              .into(),
          ));
        }
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
    let mut stream = self.model.stream(request).map_err(model_error)?;
    let mut assembler = StreamedTurnAssembler::new(prepared.policy);
    let deadline = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(deadline);
    let mut started = false;
    loop {
      let item = tokio::select! { biased;
        _ = self.session.stopped.cancelled() => return Err(RuntimeError::Uncertain("session writer fenced".into())),
        _ = token.cancelled() => return Ok(false),
        _ = &mut deadline => return Err(RuntimeError::Execution("model request exceeded 120 second deadline".into())),
        item = stream.next() => item,
      };
      let Some(item) = item else {
        break;
      };
      let item = item.map_err(model_error)?;
      if !started {
        (self.listener)(RuntimeEvent::ModelResponseStarted);
        started = true;
      }
      match assembler.ingest(&item).map_err(model_error)? {
        StreamedTurnEvent::InvalidToolCall(_) => {
          return Err(RuntimeError::Execution(
            "model emitted a tool outside the advertised authorization set".into(),
          ));
        }
        StreamedTurnEvent::EmitIngested => match &item {
          Item::Event(StreamEvent::Text { text, .. }) => {
            (self.listener)(RuntimeEvent::TextDelta { text: text.clone() })
          }
          Item::Event(StreamEvent::Reasoning { text, .. }) => {
            (self.listener)(RuntimeEvent::ThinkingDelta { text: text.clone() })
          }
          _ => {}
        },
        _ => {}
      }
    }
    let response = stream.finish().await.map_err(model_error)?;
    let usage = response.usage;
    let turn = assembler.finish(&response);
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
      .await?;
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
