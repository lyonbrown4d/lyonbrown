use agent_core::AgentEvent;

use super::{Speaker, TranscriptEntry, TuiState};
use crate::tui::{status_line_safe, terminal_safe};

impl TuiState {
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
        self.model = format!(
          "{}/{}",
          status_line_safe(&provider),
          status_line_safe(&model)
        );
        self.reason = status_line_safe(&reason);
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
          status_line_safe(&to_provider),
          status_line_safe(&to_model)
        );
        self.reason = status_line_safe(&reason);
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
        self.tool = status_line_safe(&invocation.name);
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
            status_line_safe(&result.error.unwrap_or_else(|| "unknown".into()))
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

  fn push_pulse(&mut self, kind: &str) {
    self.pulse.push(kind.to_owned());
    if self.pulse.len() > 8 {
      self.pulse.remove(0);
    }
  }
}
