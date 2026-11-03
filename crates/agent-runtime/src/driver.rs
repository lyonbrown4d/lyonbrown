//! A host IO driver for Rig. Only AgentRun chooses model/tool/done transitions.
use crate::{
  Result, RunOutcome, Runtime, RuntimeError, RuntimeEvent, RuntimeResult,
  persistence::{active, rig_error},
};
use agent_core::events::{AgentEvent, ExecutionOutcome, QueueKind, QueueStatus};
use rig_agent::run::{AgentRun, AgentRunStep};
use rig_core::message::Message;
use serde_json::json;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;
impl Runtime {
  pub(crate) async fn drive(&self, token: &CancellationToken) -> Result<RuntimeResult> {
    let id = self
      .session
      .state
      .lock()
      .await
      .active
      .as_ref()
      .ok_or_else(|| RuntimeError::Execution("no active run".into()))?
      .id
      .clone();
    (self.listener)(RuntimeEvent::RunStarted { run_id: id.clone() });
    match self.drive_steps(token).await {
      Ok(result) => Ok(result),
      Err(error @ RuntimeError::Uncertain(_)) => Err(error),
      Err(error) if self.session.failed.load(Ordering::Acquire) => Err(error),
      Err(error) => {
        self
          .finish(RunOutcome::Failed {
            message: error.to_string(),
          })
          .await
      }
    }
  }
  async fn drive_steps(&self, token: &CancellationToken) -> Result<RuntimeResult> {
    loop {
      if self.session.stopped.is_cancelled() {
        return Err(RuntimeError::Uncertain(
          "session persistence requires reconciliation".into(),
        ));
      }
      if token.is_cancelled() {
        return self.finish(RunOutcome::Cancelled).await;
      }
      let step = self
        .session
        .update("run_advanced", |state| {
          let active = active(state)?;
          let step = active.run.next_step().map_err(rig_error)?;
          Ok((step, json!({"run_id":active.id}), vec![]))
        })
        .await?;
      match step {
        AgentRunStep::CallModel {
          prompt,
          history,
          turn,
        } => {
          if self.consume_steering().await? {
            continue;
          }
          if !self.call_model(prompt, history, turn, token).await? {
            return self.finish(RunOutcome::Cancelled).await;
          }
        }
        AgentRunStep::CallTools { calls } => {
          for call in calls {
            if token.is_cancelled() {
              return self.finish(RunOutcome::Cancelled).await;
            }
            self.call_tool(call, token).await?;
          }
        }
        AgentRunStep::Done(_) => {
          if !self.start_follow_up().await? {
            return self.finish(RunOutcome::Completed).await;
          }
        }
      }
    }
  }
  /// Rig has no arbitrary append-user operation. At a safe model boundary,
  /// transfer its canonical transcript into a new bounded AgentRun. This is
  /// input scheduling; Rig still owns all reasoning/tool transitions.
  async fn consume_steering(&self) -> Result<bool> {
    self
      .session
      .update("steering_consumed", |state| {
        let Some(index) = state
          .queue
          .iter()
          .position(|input| input.kind == QueueKind::Steer && input.status == QueueStatus::Pending)
        else {
          return Ok((false, json!({}), vec![]));
        };
        let input = &mut state.queue[index];
        input.status = QueueStatus::Consumed;
        let prompt = Message::user(input.prompt.clone());
        let event = input.event(QueueStatus::Consumed);
        let active = active(state)?;
        let history = active.run.full_history();
        // next_step already issued this turn but no provider IO has started.
        let remaining = active
          .spec
          .effective_max_turns()
          .saturating_sub(active.run.turn())
          .saturating_add(1);
        active.spec.max_turns = Some(remaining);
        active.run = AgentRun::from_spec(&active.spec, prompt, None)
          .with_validated_history(history)
          .map_err(rig_error)?;
        active.prepared = None;
        active.segment = active.segment.saturating_add(1);
        Ok((true, json!({"run_id":active.id}), vec![event]))
      })
      .await
  }
  async fn start_follow_up(&self) -> Result<bool> {
    self
      .session
      .update("follow_up_boundary", |state| {
        let Some(index) = state
          .queue
          .iter()
          .position(|input| input.status == QueueStatus::Pending)
        else {
          self.admission.store(false, Ordering::Release);
          return Ok((false, json!({}), vec![]));
        };
        let input = &mut state.queue[index];
        input.status = QueueStatus::Consumed;
        let event = input.event(QueueStatus::Consumed);
        let prompt = Message::user(input.prompt.clone());
        let active = active(state)?;
        let history = active.run.full_history();
        active.spec = self.spec();
        active.run = AgentRun::from_spec(&active.spec, prompt, None)
          .with_validated_history(history)
          .map_err(rig_error)?;
        active.prepared = None;
        active.segment = active.segment.saturating_add(1);
        Ok((true, json!({"run_id":active.id}), vec![event]))
      })
      .await
  }
  pub(crate) async fn finish(&self, outcome: RunOutcome) -> Result<RuntimeResult> {
    let public_outcome = match &outcome {
      RunOutcome::Completed => ExecutionOutcome::Completed,
      RunOutcome::Cancelled => ExecutionOutcome::Cancelled,
      RunOutcome::Failed { .. } => ExecutionOutcome::Failed,
      RunOutcome::Uncertain { .. } => ExecutionOutcome::Uncertain,
    };
    let result = self
      .session
      .update("run_finished", |state| {
        if !state.pending_tools.is_empty() {
          return Err(RuntimeError::Uncertain(
            "tool effects require reconciliation".into(),
          ));
        }
        let active = state
          .active
          .take()
          .ok_or_else(|| RuntimeError::Execution("no active run".into()))?;
        let text = active
          .run
          .response()
          .map(|response| response.output())
          .unwrap_or_default();
        state.history = active.run.canonical_history();
        let result = RuntimeResult {
          run_id: active.id.clone(),
          outcome,
          text,
          usage: active.usage,
        };
        let event = AgentEvent::RunFinished {
          run_id: active.id.clone(),
          outcome: public_outcome,
        };
        Ok((
          result,
          json!({"run_id":active.id,"outcome":public_outcome}),
          vec![event],
        ))
      })
      .await?;
    (self.listener)(RuntimeEvent::RunFinished {
      run_id: result.run_id.clone(),
      outcome: format!("{public_outcome:?}").to_lowercase(),
    });
    Ok(result)
  }
}
