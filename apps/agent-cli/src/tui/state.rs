use agent_app::{RuntimePhase, RuntimeSnapshot};
use agent_core::{AgentEvent, StepId};
use crossterm::event::KeyCode;

use super::{connection::ConnectionProjection, input::edit_input, terminal_safe};

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
  pub(super) model_setup_required: bool,
  queued_follow_up: Option<String>,
}

impl TuiState {
  pub fn apply_input_key(&mut self, key: KeyCode) {
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

  pub fn reduce(&mut self, event: AgentEvent) {
    self.push_pulse(event.kind());
    match event {
      AgentEvent::UserMessageReceived { text, .. } => {
        self.transcript.push(TranscriptEntry {
          speaker: Speaker::User,
          text: terminal_safe(&text),
        });
        self.status = "thinking".into();
        self.busy = true;
      }
      AgentEvent::TurnStarted { .. } => {
        self.status = "thinking".into();
        self.busy = true;
      }
      AgentEvent::TurnSteered { text, .. } => {
        self.transcript.push(TranscriptEntry {
          speaker: Speaker::User,
          text: terminal_safe(&text),
        });
        self.status = "steering current turn".into();
        self.busy = true;
      }
      AgentEvent::StepStarted { step_id, .. } => {
        self.step = step_id.to_string();
        self.status = "preparing step".into();
      }
      AgentEvent::ContextPrepared { context_tokens, .. } => {
        self.status = format!("context ready · {context_tokens} tokens");
      }
      AgentEvent::ModelSelected {
        provider,
        model,
        reason,
        ..
      } => {
        self.model = format!("{}/{}", terminal_safe(&provider), terminal_safe(&model));
        self.reason = terminal_safe(&reason);
        self.status = "model selected".into();
      }
      AgentEvent::ModelFallback {
        to_provider,
        to_model,
        reason,
        ..
      } => {
        self.model = format!(
          "{}/{}",
          terminal_safe(&to_provider),
          terminal_safe(&to_model)
        );
        self.reason = terminal_safe(&reason);
        self.status = format!("model fallback · {}", self.model);
      }
      AgentEvent::ModelStarted { .. } => self.status = "streaming".into(),
      AgentEvent::ModelDelta { step_id, delta } => {
        let delta = terminal_safe(&delta);
        if self.streaming_step == Some(step_id) {
          if let Some(entry) = self.transcript.last_mut() {
            entry.text.push_str(&delta);
          }
        } else {
          self.transcript.push(TranscriptEntry {
            speaker: Speaker::Assistant,
            text: delta,
          });
          self.streaming_step = Some(step_id);
        }
        self.status = "streaming".into();
        self.busy = true;
      }
      AgentEvent::ModelCompleted { usage, .. } => {
        self.usage.0 = self.usage.0.saturating_add(usage.input);
        self.usage.1 = self.usage.1.saturating_add(usage.output);
        self.usage.2 = self.usage.2.saturating_add(usage.cache_read);
        self.status = "model complete".into();
        self.streaming_step = None;
      }
      AgentEvent::ToolRequested { invocation, .. } => {
        self.tool = terminal_safe(&invocation.name);
        self.status = format!("tool requested · {}", self.tool);
      }
      AgentEvent::ToolApproved { .. } => self.status = "tool approved".into(),
      AgentEvent::ToolStarted { .. } => self.status = "tool running".into(),
      AgentEvent::ToolCompleted { result, .. } => {
        self.status = if result.ok {
          "tool complete".into()
        } else {
          format!(
            "tool error · {}",
            terminal_safe(&result.error.unwrap_or_else(|| "unknown".into()))
          )
        };
      }
      AgentEvent::StepCompleted { .. } => self.status = "step complete".into(),
      AgentEvent::TurnCompleted { answer, .. } => {
        let answer = terminal_safe(&answer);
        if !answer.is_empty()
          && !self
            .transcript
            .iter()
            .rev()
            .any(|entry| entry.speaker == Speaker::Assistant && entry.text == answer)
        {
          self.transcript.push(TranscriptEntry {
            speaker: Speaker::Assistant,
            text: answer,
          });
        }
        self.status = "idle".into();
        self.busy = false;
        self.streaming_step = None;
      }
      AgentEvent::TurnCancelled { .. } => {
        self.status = "cancelled".into();
        self.busy = false;
      }
      AgentEvent::Error { message, .. } => {
        self.status = "error".into();
        self.transcript.push(TranscriptEntry {
          speaker: Speaker::System,
          text: terminal_safe(&message),
        });
        self.busy = false;
        self.streaming_step = None;
      }
      AgentEvent::SessionStarted { .. }
      | AgentEvent::ObservationCreated { .. }
      | AgentEvent::SessionClosed { .. } => {}
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
    self.model_setup_required
  }

  pub fn sync_snapshot(&mut self, snapshot: &RuntimeSnapshot) {
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
    self.model_setup_required = snapshot.phase == RuntimePhase::ModelSetupRequired;
    self.busy = matches!(
      snapshot.phase,
      RuntimePhase::Preparing
        | RuntimePhase::Streaming
        | RuntimePhase::AwaitingApproval
        | RuntimePhase::RunningTool
        | RuntimePhase::Cancelling
    );
    if let Some(route) = &snapshot.route {
      self.model = format!(
        "{}/{}",
        terminal_safe(&route.selected.provider),
        terminal_safe(&route.selected.model)
      );
      self.reason = terminal_safe(&route.reason);
    }
  }

  pub(crate) fn mark_steering_submitted(&mut self) {
    self.status = "steering queued for current turn".into();
  }

  pub(crate) fn set_queued_follow_up(&mut self, text: Option<String>) {
    self.queued_follow_up = text.map(|value| terminal_safe(&value));
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

  fn push_pulse(&mut self, kind: &str) {
    self.pulse.push(kind.to_owned());
    if self.pulse.len() > 8 {
      self.pulse.remove(0);
    }
  }

  pub(crate) fn model(&self) -> &str {
    if self.model.is_empty() {
      "—"
    } else {
      &self.model
    }
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
