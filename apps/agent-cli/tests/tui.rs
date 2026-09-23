use std::{sync::Arc, thread};

use agent_cli::tui::{TuiApprovalPrompt, TuiState, render, render_with_approval};
use agent_core::{
  AgentEvent, RiskLevel, SecretRef, SessionId, StepId, TokenUsage, ToolCallId, ToolDefinition,
  ToolInvocation, ToolResult, ToolSource,
};
use agent_tool::{ApprovalDecision, ApprovalPrompt};
use chrono::Utc;
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn screen(state: &TuiState, width: u16, height: u16) -> String {
  let backend = TestBackend::new(width, height);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal.draw(|frame| render(frame, state)).unwrap();
  let buffer = terminal.backend().buffer();
  (0..height)
    .map(|y| {
      (0..width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn approval_screen(
  state: &TuiState,
  request: &agent_cli::tui::ApprovalRequest,
  scroll: u16,
  width: u16,
  height: u16,
) -> String {
  let backend = TestBackend::new(width, height);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| {
      render_with_approval(frame, state, Some(request), scroll);
    })
    .unwrap();
  let buffer = terminal.backend().buffer();
  (0..height)
    .map(|y| {
      (0..width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn streaming_deltas_are_coalesced_into_one_assistant_message() {
  let mut state = TuiState::default();
  let step_id = StepId::new();
  state.reduce(AgentEvent::ModelDelta {
    step_id,
    delta: "hello ".into(),
  });
  state.reduce(AgentEvent::ModelDelta {
    step_id,
    delta: "world".into(),
  });

  assert_eq!(state.transcript().len(), 1);
  assert_eq!(state.transcript()[0].text, "hello world");
}

#[test]
fn terminal_control_sequences_are_never_kept_as_renderable_text() {
  let mut state = TuiState::default();
  state.reduce(AgentEvent::ModelDelta {
    step_id: StepId::new(),
    delta: "\u{1b}[31mred\u{7}".into(),
  });

  assert!(!state.transcript()[0].text.contains('\u{1b}'));
  assert!(!state.transcript()[0].text.contains('\u{7}'));
  assert!(state.transcript()[0].text.contains("red"));
}

#[test]
fn wide_layout_shows_instrument_panel_and_event_rail() {
  let mut state = TuiState::default();
  state.reduce(AgentEvent::ModelSelected {
    step_id: StepId::new(),
    provider: "local".into(),
    model: "test-model".into(),
    reason: "workspace route".into(),
    ts: Utc::now(),
  });
  let output = screen(&state, 120, 28);

  assert!(output.contains("CONVERSATION"));
  assert!(output.contains("RUNTIME"));
  assert!(output.contains("STEP"));
  assert!(output.contains("TOOL"));
  assert!(output.contains("test-model"));
  assert!(output.contains("EVENT PULSE"));
  assert!(output.contains("Enter send"));
}

#[test]
fn narrow_layout_collapses_status_panel_but_keeps_composer() {
  let output = screen(&TuiState::default(), 72, 20);

  assert!(output.contains("CONVERSATION"));
  assert!(!output.contains("RUNTIME"));
  assert!(output.contains("Message"));
}

#[test]
fn tool_and_error_events_update_runtime_status() {
  let mut state = TuiState::default();
  let step_id = StepId::new();
  let invocation_id = ToolCallId::new();
  state.reduce(AgentEvent::ToolRequested {
    step_id,
    invocation: ToolInvocation {
      id: invocation_id,
      name: "shell".into(),
      arguments: json!({"command": "echo safe"}),
    },
    ts: Utc::now(),
  });
  assert!(state.status().contains("shell"));

  state.reduce(AgentEvent::ToolCompleted {
    step_id,
    result: ToolResult {
      invocation_id,
      ok: false,
      output: String::new(),
      truncated: false,
      duration_ms: 2,
      error: Some("permission denied".into()),
    },
    ts: Utc::now(),
  });
  state.reduce(AgentEvent::Error {
    session_id: SessionId::new(),
    message: "provider unavailable".into(),
    ts: Utc::now(),
  });

  assert!(state.status().contains("error"));
  assert!(screen(&state, 110, 24).contains("provider unavailable"));
}

#[test]
fn reducer_tracks_token_usage() {
  let mut state = TuiState::default();
  state.reduce(AgentEvent::ModelCompleted {
    step_id: StepId::new(),
    usage: TokenUsage {
      input: 12,
      output: 7,
      cache_read: 3,
    },
    ts: Utc::now(),
  });
  assert!(screen(&state, 100, 24).contains("12 in / 7 out"));
}

#[test]
fn minimum_supported_terminal_does_not_panic() {
  let output = screen(&TuiState::default(), 30, 8);
  assert!(!output.is_empty());
}

#[path = "tui/extended.rs"]
mod extended;
