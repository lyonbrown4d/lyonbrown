use agent_core::{AgentEvent, StepId, StopReason, TurnId};
use std::collections::VecDeque;

use crate::{
  ActivityPhase, ActivityView, ContextSummary, ModelFallbackView, ModelRoute, RouteSummary,
  RuntimePhase, RuntimeSnapshot, TurnStatus, TurnView,
};

mod tools;

/// Deterministic projection of the durable event stream into UI-neutral state.
#[derive(Debug, Default)]
pub struct Projector {
  snapshot: RuntimeSnapshot,
}

impl Projector {
  pub fn new(snapshot: RuntimeSnapshot) -> Self {
    Self { snapshot }
  }

  pub fn snapshot(&self) -> &RuntimeSnapshot {
    &self.snapshot
  }

  pub fn into_snapshot(self) -> RuntimeSnapshot {
    self.snapshot
  }

  pub(crate) fn set_phase(&mut self, phase: RuntimePhase) {
    self.snapshot.phase = phase;
  }

  pub(crate) fn record_failure(&mut self, message: String) {
    self.record_error(message);
  }

  pub(crate) fn set_queued_follow_ups(&mut self, queued: &VecDeque<String>) {
    self.snapshot.queued_follow_ups = queued.iter().cloned().collect();
  }

  pub fn extend(&mut self, events: impl IntoIterator<Item = AgentEvent>) {
    for event in events {
      self.apply(event);
    }
  }

  pub fn apply(&mut self, event: AgentEvent) {
    match event {
      AgentEvent::SessionStarted { session_id, .. } => {
        self.snapshot.session_id = Some(session_id);
        self.snapshot.phase = RuntimePhase::Ready;
      }
      AgentEvent::UserMessageReceived { turn_id, text, .. } => {
        self.receive_user_message(turn_id, text)
      }
      AgentEvent::TurnStarted { turn_id, .. } => {
        if let Some(turn) = self.turn_mut(turn_id) {
          turn.status = TurnStatus::Running;
        }
        self.snapshot.phase = RuntimePhase::Preparing;
      }
      AgentEvent::TurnSteered { turn_id, text, .. } => {
        if let Some(turn) = self.turn_mut(turn_id) {
          if let Some(activity) = turn.activities.last_mut()
            && !matches!(
              activity.phase,
              ActivityPhase::Completed | ActivityPhase::Failed
            )
          {
            activity.phase = ActivityPhase::Superseded;
          }
          turn.steering.push(text);
          turn.status = TurnStatus::Running;
        }
      }
      AgentEvent::StepStarted {
        turn_id, step_id, ..
      } => {
        if let Some(turn) = self.turn_mut(turn_id) {
          turn.activities.push(ActivityView::new(step_id));
        }
        self.snapshot.phase = RuntimePhase::Preparing;
      }
      AgentEvent::ContextPrepared {
        step_id,
        system_tokens,
        context_tokens,
        dropped,
        ..
      } => {
        if let Some(activity) = self.activity_mut(step_id) {
          activity.context = Some(ContextSummary {
            system_tokens,
            context_tokens,
            dropped,
          });
        }
      }
      AgentEvent::ModelSelected {
        step_id,
        provider,
        model,
        reason,
        ..
      } => self.select_model(step_id, provider, model, reason),
      AgentEvent::ModelFallback {
        step_id,
        from_provider,
        from_model,
        to_provider,
        to_model,
        reason,
        ..
      } => self.fallback_model(
        step_id,
        ModelRoute {
          provider: from_provider,
          model: from_model,
        },
        ModelRoute {
          provider: to_provider,
          model: to_model,
        },
        reason,
      ),
      AgentEvent::ModelStarted { step_id, .. } => {
        if let Some(activity) = self.activity_mut(step_id) {
          activity.phase = ActivityPhase::Streaming;
        }
        self.snapshot.phase = RuntimePhase::Streaming;
      }
      AgentEvent::ModelDelta { step_id, delta } => self.append_delta(step_id, &delta),
      AgentEvent::ModelCompleted { step_id, usage, .. } => {
        if let Some(activity) = self.activity_mut(step_id) {
          activity.usage = Some(usage);
          activity.phase = ActivityPhase::Preparing;
        }
        self.snapshot.phase = RuntimePhase::Preparing;
      }
      AgentEvent::ToolRequested {
        step_id,
        invocation,
        ..
      } => self.request_tool(step_id, invocation),
      AgentEvent::ToolApproved {
        step_id,
        tool_call_id,
        by,
        ..
      } => self.approve_tool(step_id, tool_call_id, by),
      AgentEvent::ToolStarted {
        step_id,
        tool_call_id,
        ..
      } => self.start_tool(step_id, tool_call_id),
      AgentEvent::ToolCompleted {
        step_id, result, ..
      } => self.complete_tool(step_id, result),
      AgentEvent::StepCompleted {
        step_id, outcome, ..
      } => {
        let failed = matches!(outcome.stop_reason, StopReason::Error(_));
        if let Some(activity) = self.activity_mut(step_id) {
          activity.phase = if failed {
            ActivityPhase::Failed
          } else {
            ActivityPhase::Completed
          };
          activity.outcome = Some(outcome);
        }
      }
      AgentEvent::TurnCompleted {
        turn_id, answer, ..
      } => self.complete_turn(turn_id, answer),
      AgentEvent::TurnCancelled { turn_id, .. } => self.cancel_turn(turn_id),
      AgentEvent::Error { message, .. } => self.record_error(message),
      AgentEvent::SessionClosed { .. } => {
        self.snapshot.approval = None;
        self.snapshot.phase = RuntimePhase::Ready;
      }
      AgentEvent::ObservationCreated { .. } => {}
    }
  }

  fn receive_user_message(&mut self, turn_id: TurnId, text: String) {
    if let Some(turn) = self.turn_mut(turn_id) {
      turn.user_text = text;
    } else {
      self.snapshot.turns.push(TurnView::new(turn_id, text));
    }
  }

  fn select_model(&mut self, step_id: StepId, provider: String, model: String, reason: String) {
    let route = ModelRoute { provider, model };
    if let Some(activity) = self.activity_mut(step_id) {
      let fallbacks = activity
        .route
        .take()
        .map(|summary| summary.fallbacks)
        .unwrap_or_default();
      activity.route = Some(RouteSummary {
        selected: route,
        reason,
        fallbacks,
      });
      self.snapshot.route = activity.route.clone();
    }
  }

  fn fallback_model(&mut self, step_id: StepId, from: ModelRoute, to: ModelRoute, reason: String) {
    if let Some(activity) = self.activity_mut(step_id) {
      let summary = activity.route.get_or_insert_with(|| RouteSummary {
        selected: to.clone(),
        reason: reason.clone(),
        fallbacks: Vec::new(),
      });
      summary.selected = to.clone();
      summary.reason = reason.clone();
      summary
        .fallbacks
        .push(ModelFallbackView { from, to, reason });
      self.snapshot.route = Some(summary.clone());
    }
  }

  fn append_delta(&mut self, step_id: StepId, delta: &str) {
    if let Some(activity) = self.activity_mut(step_id) {
      activity.phase = ActivityPhase::Streaming;
      activity.assistant_text.push_str(delta);
    }
    if let Some(turn) = self.turn_for_step_mut(step_id) {
      turn.assistant_text.push_str(delta);
    }
    self.snapshot.phase = RuntimePhase::Streaming;
  }

  fn turn_mut(&mut self, turn_id: TurnId) -> Option<&mut TurnView> {
    self
      .snapshot
      .turns
      .iter_mut()
      .find(|turn| turn.turn_id == turn_id)
  }

  fn turn_for_step_mut(&mut self, step_id: StepId) -> Option<&mut TurnView> {
    self.snapshot.turns.iter_mut().find(|turn| {
      turn
        .activities
        .iter()
        .any(|activity| activity.step_id == step_id)
    })
  }

  fn activity_mut(&mut self, step_id: StepId) -> Option<&mut ActivityView> {
    self
      .snapshot
      .turns
      .iter_mut()
      .flat_map(|turn| &mut turn.activities)
      .find(|activity| activity.step_id == step_id)
  }
}
