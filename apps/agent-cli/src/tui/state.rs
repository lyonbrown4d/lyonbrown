use agent_app::{ModelSetupView, RuntimePhase, RuntimeSnapshot};
use agent_core::{SessionId, StepId};
use crossterm::event::KeyCode;

use super::{
  ActivityItem, OverlayState, activity, connection::ConnectionProjection, input::edit_input,
  status_line_safe, terminal_safe,
};

mod activity_cache;
mod events;
mod overlay;

use activity_cache::ActivityDetailCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
  User,
  Assistant,
  System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
  pub speaker: Speaker,
  pub text: String,
}

#[derive(Debug, Default)]
pub struct TuiState {
  pub(super) connection: ConnectionProjection,
  runtime_phase: RuntimePhase,
  command_error: Option<String>,
  client_local_notice: Option<String>,
  transcript: Vec<TranscriptEntry>,
  status: String,
  model: String,
  reason: String,
  step: String,
  tool: String,
  usage: (u32, u32, u32),
  pulse: Vec<String>,
  streaming_step: Option<StepId>,
  pub(crate) input: String,
  pub(crate) input_cursor: usize,
  pub(crate) busy: bool,
  model_setup: Option<ModelSetupView>,
  queued_follow_up: Option<String>,
  overlay: OverlayState,
  activities: Vec<ActivityItem>,
  session_id: Option<SessionId>,
  session_observed: bool,
  activity_generation: u64,
  activity_cache: ActivityDetailCache,
}

impl TuiState {
  pub fn apply_input_key(&mut self, key: KeyCode) {
    self.client_local_notice = None;
    let (input, cursor) = edit_input(&self.input, self.input_cursor, key);
    self.input = input;
    self.input_cursor = cursor;
  }

  pub fn transcript(&self) -> &[TranscriptEntry] {
    &self.transcript
  }

  pub fn status(&self) -> &str {
    if self.status.is_empty() {
      "idle"
    } else {
      &self.status
    }
  }

  pub(crate) fn mark_submitted(&mut self) {
    self.status = "starting turn".into();
    self.busy = true;
  }

  pub fn is_busy(&self) -> bool {
    self.busy
  }

  pub fn is_model_setup_required(&self) -> bool {
    self.runtime_phase == RuntimePhase::ModelSetupRequired
  }

  pub fn model_setup(&self) -> Option<&ModelSetupView> {
    (self.runtime_phase == RuntimePhase::ModelSetupRequired)
      .then_some(self.model_setup.as_ref())
      .flatten()
  }

  pub fn command_error(&self) -> Option<&str> {
    self.command_error.as_deref()
  }

  pub fn client_local_notice(&self) -> Option<&str> {
    self.client_local_notice.as_deref()
  }

  pub fn sync_snapshot(&mut self, snapshot: &RuntimeSnapshot) {
    let session_changed = self.session_observed && self.session_id != snapshot.session_id;
    if session_changed {
      self.reset_activity_inspector();
    }
    self.session_id = snapshot.session_id;
    self.session_observed = true;
    if snapshot.phase == RuntimePhase::ModelSetupRequired && snapshot.model_setup.is_some() {
      self.close_overlay();
    }
    if !same_model_setup_state(self.model_setup.as_ref(), snapshot.model_setup.as_ref()) {
      self.command_error = None;
    }
    self.transcript.clear();
    for turn in &snapshot.turns {
      self.transcript.push(TranscriptEntry {
        speaker: Speaker::User,
        text: terminal_safe(&turn.user_text),
      });
      self
        .transcript
        .extend(turn.steering.iter().map(|text| TranscriptEntry {
          speaker: Speaker::User,
          text: terminal_safe(text),
        }));
      if !turn.assistant_text.is_empty() {
        self.transcript.push(TranscriptEntry {
          speaker: Speaker::Assistant,
          text: terminal_safe(&turn.assistant_text),
        });
      }
    }
    if let Some(message) = &snapshot.last_error {
      self.transcript.push(TranscriptEntry {
        speaker: Speaker::System,
        text: terminal_safe(message),
      });
    }
    self.status = phase_status(snapshot.phase).into();
    self.runtime_phase = snapshot.phase;
    self.model_setup = snapshot.model_setup.clone();
    self.activity_generation = self.activity_generation.wrapping_add(1);
    self.activity_cache.clear();
    self.activities = activity::project(snapshot);
    self.reconcile_activity_selection();
    self.busy = matches!(
      snapshot.phase,
      RuntimePhase::Preparing
        | RuntimePhase::Streaming
        | RuntimePhase::AwaitingApproval
        | RuntimePhase::RunningTool
        | RuntimePhase::Cancelling
    );
    match &snapshot.route {
      Some(route) => {
        self.model = format!(
          "{}/{}",
          status_line_safe(&route.selected.provider),
          status_line_safe(&route.selected.model)
        );
        self.reason = status_line_safe(&route.reason);
      }
      None => {
        self.model.clear();
        self.reason.clear();
      }
    }
  }

  pub(crate) fn mark_steering_submitted(&mut self) {
    self.status = "steering queued for current turn".into();
  }

  pub(crate) fn set_queued_follow_up(&mut self, text: Option<String>) {
    self.queued_follow_up = text.map(|value| status_line_safe(&value));
  }

  pub(crate) fn queued_follow_up(&self) -> Option<&str> {
    self.queued_follow_up.as_deref()
  }

  pub(crate) fn set_error(&mut self, message: impl Into<String>) {
    self.status = "error".into();
    self.transcript.push(TranscriptEntry {
      speaker: Speaker::System,
      text: terminal_safe(&message.into()),
    });
    self.busy = false;
  }

  pub(crate) fn set_client_local_notice(&mut self, message: impl Into<String>) {
    self.client_local_notice = Some(status_line_safe(&message.into()));
  }

  pub(crate) fn clear_client_local_notice(&mut self) {
    self.client_local_notice = None;
  }

  pub(crate) fn set_command_error(&mut self, message: impl Into<String>) {
    self.command_error = Some(status_line_safe(&message.into()));
  }

  pub(crate) fn clear_command_error(&mut self) {
    self.command_error = None;
  }

  pub(crate) fn reset_epoch_transients(&mut self) {
    self.model.clear();
    self.reason.clear();
    self.step.clear();
    self.tool.clear();
    self.usage = (0, 0, 0);
    self.pulse.clear();
    self.streaming_step = None;
    self.session_id = None;
    self.session_observed = false;
    self.activity_generation = self.activity_generation.wrapping_add(1);
    self.activity_cache.clear();
    self.reset_activity_inspector();
  }

  pub(crate) fn model(&self) -> &str {
    if self.model.is_empty() {
      "—"
    } else {
      &self.model
    }
  }

  pub fn activities(&self) -> &[ActivityItem] {
    &self.activities
  }
}

fn same_model_setup_state(current: Option<&ModelSetupView>, next: Option<&ModelSetupView>) -> bool {
  match (current, next) {
    (Some(current), Some(next)) => {
      current.source == next.source
        && current.phase == next.phase
        && current.managed_model == next.managed_model
        && current.operation_id == next.operation_id
        && current.failure == next.failure
        && current.allowed_actions == next.allowed_actions
    }
    (None, None) => true,
    _ => false,
  }
}

fn phase_status(phase: RuntimePhase) -> &'static str {
  match phase {
    RuntimePhase::Initializing => "initializing",
    RuntimePhase::ModelSetupRequired => "model setup required",
    RuntimePhase::Ready | RuntimePhase::Completed => "idle",
    RuntimePhase::Preparing => "preparing step",
    RuntimePhase::Streaming => "streaming",
    RuntimePhase::AwaitingApproval => "awaiting approval",
    RuntimePhase::RunningTool => "tool running",
    RuntimePhase::Cancelling => "cancelling",
    RuntimePhase::FailedRecoverable | RuntimePhase::FailedTerminal => "error",
  }
}
