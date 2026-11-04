use super::archive::SummaryIntent;
use crate::{ContextStatus, Result, Runtime, RuntimeError, RuntimeEvent};
use rig_memory::{CompactReason, SummaryPlan};
use serde_json::json;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;

impl Runtime {
  /// Manual compaction is admitted only while no AgentRun owns the session.
  pub async fn compact(&self, focus: &str) -> Result<ContextStatus> {
    if focus.len() > 4096 {
      return Err(RuntimeError::Configuration(
        "compaction focus exceeds 4096 bytes".into(),
      ));
    }
    let _execution = self
      .execution
      .try_lock()
      .map_err(|_| RuntimeError::Configuration("session already has an active execution".into()))?;
    self.session.ensure_certain().await?;
    if self.session.state.lock().await.active.is_some() {
      return Err(RuntimeError::Configuration(
        "resume the existing run before compacting".into(),
      ));
    }
    let token = self.begin_cancellation().await;
    let result = self
      .compact_context(
        CompactReason::Asked {
          focus: focus.into(),
        },
        &token,
      )
      .await;
    *self.cancellation.lock().await = None;
    result?;
    self.context_status().await
  }

  pub async fn undo_compaction(&self) -> Result<ContextStatus> {
    let _execution = self
      .execution
      .try_lock()
      .map_err(|_| RuntimeError::Configuration("session already has an active execution".into()))?;
    self.session.ensure_certain().await?;
    self
      .session
      .update("context_compaction_undone", |state| {
        if state.active.is_some() || state.context.pending.is_some() {
          return Err(RuntimeError::Configuration("context is active".into()));
        }
        state.context.undo()?;
        Ok(((), json!({"revision":state.context.revision}), vec![]))
      })
      .await?;
    self.context_status().await
  }

  pub(crate) async fn compact_context(
    &self,
    reason: CompactReason,
    token: &CancellationToken,
  ) -> Result<bool> {
    let policy = self.context_policy();
    let spec = self.context_spec()?;
    let planned = self
      .session
      .update("context_summary_intent", |state| {
        if state.context.pending.is_some()
          || !state.pending_tools.is_empty()
          || state
            .active
            .as_ref()
            .is_some_and(|active| active.in_flight.is_some())
        {
          return Err(RuntimeError::Uncertain("context has unresolved IO".into()));
        }
        let conversation = state.context.summarizable()?;
        conversation
          .validate()
          .map_err(crate::persistence::rig_error)?;
        if !conversation.pending_calls().is_empty() {
          return Err(RuntimeError::Configuration(
            "context has unfinished tool calls".into(),
          ));
        }
        let plan = policy
          .plan_summary(
            &conversation,
            &rig_memory::summary_state(&conversation),
            &reason,
            &spec,
          )
          .map_err(crate::persistence::rig_error)?;
        let Some(plan) = plan else {
          return Ok((None, json!({"outcome":"nothing_to_compact"}), vec![]));
        };
        let id = agent_core::new_id();
        let revision = state.context.revision;
        state.context.pending = Some(SummaryIntent {
          id: id.clone(),
          revision,
          sending: false,
        });
        Ok((
          Some((id.clone(), revision, plan)),
          json!({"attempt_id":id,"revision":revision}),
          vec![agent_core::events::AgentEvent::Compaction { active: true }],
        ))
      })
      .await?;
    let Some((id, revision, plan)) = planned else {
      return Ok(false);
    };
    (self.listener)(RuntimeEvent::Compaction { active: true });
    let response = self.summary_request(&id, revision, &plan, token).await;
    let result = match response {
      Ok(summary) => self.apply_compaction(&id, revision, plan, summary).await.map_err(|_| {
        RuntimeError::Uncertain("summary response could not be durably applied; retained intent requires reconciliation".into())
      }),
      Err(error) => {
        let sending = self
          .session
          .state
          .lock()
          .await
          .context
          .pending
          .as_ref()
          .is_some_and(|intent| intent.id == id && intent.sending);
        if self.session.failed.load(Ordering::Acquire) {
          Err(error)
        } else if sending && matches!(error, RuntimeError::Uncertain(_)) {
          // Keep both intent and the old conversation. Reopening cannot replay it.
          self
            .session
            .uncertain("summary request outcome requires reconciliation".into())
            .await
            .map(|()| false)
        } else {
          self
            .session
            .update("context_summary_failed", |state| {
              check_intent(state, &id, revision)?;
              state.context.pending = None;
              Ok((
                (),
                json!({"attempt_id":id,"outcome":"failed"}),
                vec![agent_core::events::AgentEvent::Compaction { active: false }],
              ))
            })
            .await?;
          Err(error)
        }
      }
    };
    (self.listener)(RuntimeEvent::Compaction { active: false });
    result
  }

  async fn apply_compaction(
    &self,
    id: &str,
    revision: u64,
    mut plan: SummaryPlan,
    summary: String,
  ) -> Result<bool> {
    self
      .session
      .update("context_compaction_committed", |state| {
        check_intent(state, id, revision)?;
        let mut conversation = state.context.conversation()?;
        plan.state.summary = summary;
        rig_memory::apply_summary(&mut conversation, plan.keep_from, &plan.state)
          .ok_or_else(|| RuntimeError::Execution("summary cut no longer exists".into()))?;
        conversation
          .validate()
          .map_err(crate::persistence::rig_error)?;
        state.context.save(&mut conversation)?;
        state.context.pending = None;
        Ok((
          true,
          json!({"attempt_id":id,"revision":state.context.revision}),
          vec![agent_core::events::AgentEvent::Compaction { active: false }],
        ))
      })
      .await
  }

  /// The only automatic compaction point: all preceding tools have final results,
  /// and no provider attempt for the next turn has been started.
  pub(crate) async fn prepare_context(
    &self,
    token: &CancellationToken,
  ) -> Result<Vec<rig_core::message::Message>> {
    let spec = self.context_spec()?;
    let output_limit = u64::from(
      spec
        .max_output_tokens
        .unwrap_or(self.config.max_output_tokens),
    );
    self
      .session
      .update("context_boundary_recorded", |state| {
        let active = state
          .active
          .as_mut()
          .ok_or_else(|| RuntimeError::Execution("no active run".into()))?;
        active.spec.max_tokens = Some(
          active
            .spec
            .max_tokens
            .unwrap_or(output_limit)
            .min(output_limit),
        );
        state
          .context
          .append(&active.run.full_history(), &mut active.context_cursor)?;
        Ok(((), json!({"revision":state.context.revision}), vec![]))
      })
      .await?;
    let (messages, preamble, output) = {
      let state = self.session.state.lock().await;
      let active = state
        .active
        .as_ref()
        .ok_or_else(|| RuntimeError::Execution("no active run".into()))?;
      (
        state.context.messages()?,
        active.spec.preamble.clone(),
        active.spec.max_tokens.unwrap_or(output_limit),
      )
    };
    let estimate = self.context_estimate_for(messages, preamble.as_deref())?;
    if self.context_policy().over_threshold(estimate, &spec) {
      self
        .compact_context(CompactReason::Threshold, token)
        .await?;
    }
    let messages = self.session.state.lock().await.context.messages()?;
    if self
      .context_estimate_for(messages.clone(), preamble.as_deref())?
      .saturating_add(output)
      >= u64::from(spec.input_limit().unwrap_or(self.config.context_window))
    {
      return Err(RuntimeError::Configuration(
        "context remains above the model input budget".into(),
      ));
    }
    Ok(messages)
  }
}

pub(super) fn check_intent(
  state: &crate::persistence::Snapshot,
  id: &str,
  revision: u64,
) -> Result<()> {
  let matches = state.context.pending.as_ref().is_some_and(|intent| {
    intent.id == id && intent.revision == revision && state.context.revision == revision
  });
  if !matches {
    return Err(RuntimeError::Uncertain(
      "summary compare-and-swap refused a changed context".into(),
    ));
  }
  Ok(())
}
