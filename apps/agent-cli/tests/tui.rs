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

#[test]
fn wide_conversation_follows_the_newest_streaming_text() {
    let mut state = TuiState::default();
    for index in 0..20 {
        state.reduce(AgentEvent::ModelDelta {
            step_id: StepId::new(),
            delta: format!("older message {index}"),
        });
    }
    state.reduce(AgentEvent::ModelDelta {
        step_id: StepId::new(),
        delta: "NEWEST-WIDE-MARKER".into(),
    });

    assert!(screen(&state, 120, 12).contains("NEWEST-WIDE-MARKER"));
}

#[test]
fn narrow_conversation_follows_wrapped_newest_streaming_text() {
    let mut state = TuiState::default();
    for index in 0..20 {
        state.reduce(AgentEvent::ModelDelta {
            step_id: StepId::new(),
            delta: format!("older narrow message {index}"),
        });
    }
    state.reduce(AgentEvent::ModelDelta {
        step_id: StepId::new(),
        delta: "NEWEST-NARROW-MARKER".into(),
    });

    assert!(screen(&state, 72, 12).contains("NEWEST-NARROW-MARKER"));
}

#[test]
fn narrow_conversation_follows_newest_text_after_word_boundary_wraps() {
    let mut state = TuiState::default();
    let multi_word = "abcdefghijklmnopqrst uvwxyzabcdefghijklmn opqrstuvwxyzabcdefgh";
    for _ in 0..8 {
        state.reduce(AgentEvent::ModelDelta {
            step_id: StepId::new(),
            delta: multi_word.into(),
        });
    }
    state.reduce(AgentEvent::ModelDelta {
        step_id: StepId::new(),
        delta: "NEWEST-WORD-WRAP-MARKER".into(),
    });

    assert!(screen(&state, 40, 12).contains("NEWEST-WORD-WRAP-MARKER"));
}

#[test]
fn approval_bridge_sends_sanitized_request_and_returns_ui_decision() {
    let (prompt, requests) = TuiApprovalPrompt::channel();
    let prompt = Arc::new(prompt);
    let definition = ToolDefinition {
        name: "shell".into(),
        description: String::new(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Execute,
        requires_secrets: vec![SecretRef::env("TOKEN")],
    };
    let invocation = ToolInvocation {
        id: ToolCallId::new(),
        name: "shell".into(),
        arguments: json!({"command": "echo ready", "token": "secret:env:TOKEN"}),
    };
    let worker = thread::spawn(move || prompt.prompt(&definition, &invocation));

    let request = requests.recv().unwrap();
    assert_eq!(request.tool, "shell");
    assert_eq!(request.risk, RiskLevel::Execute);
    assert!(request.arguments.contains("echo ready"));
    assert_eq!(request.secret_references, ["env:TOKEN"]);
    assert!(!request.arguments.contains("plaintext-secret"));
    request.respond(ApprovalDecision::AllowSession);

    assert_eq!(worker.join().unwrap(), ApprovalDecision::AllowSession);
}

#[test]
fn approval_modal_scrolls_long_arguments_and_shows_position() {
    let (prompt, requests) = TuiApprovalPrompt::channel();
    let definition = ToolDefinition {
        name: "shell".into(),
        description: String::new(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Execute,
        requires_secrets: vec![],
    };
    let long_command = (0..30)
        .map(|index| format!("argument-{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let invocation = ToolInvocation {
        id: ToolCallId::new(),
        name: "shell".into(),
        arguments: json!({"command": format!("{long_command} TAIL-MARKER \u{1b}[31m")}),
    };
    let worker = thread::spawn(move || prompt.prompt(&definition, &invocation));
    let request = requests.recv().unwrap();

    let first = approval_screen(&TuiState::default(), &request, 0, 52, 14);
    let last = approval_screen(&TuiState::default(), &request, u16::MAX, 52, 14);
    assert!(first.contains("lines "));
    assert!(!first.contains("TAIL-MARKER"));
    assert!(last.contains("TAIL-MARKER"));
    assert!(!last.contains('\u{1b}'));

    request.respond(ApprovalDecision::Deny);
    assert_eq!(worker.join().unwrap(), ApprovalDecision::Deny);
}

#[test]
fn approval_modal_uses_full_minimum_screen_without_panicking() {
    let (prompt, requests) = TuiApprovalPrompt::channel();
    let definition = ToolDefinition {
        name: "shell".into(),
        description: String::new(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Execute,
        requires_secrets: vec![],
    };
    let invocation = ToolInvocation {
        id: ToolCallId::new(),
        name: "shell".into(),
        arguments: json!({"command": "echo inspect"}),
    };
    let worker = thread::spawn(move || prompt.prompt(&definition, &invocation));
    let request = requests.recv().unwrap();

    let output = approval_screen(&TuiState::default(), &request, 0, 30, 8);
    assert!(output.contains("y/n/a/d"));
    assert!(output.contains("lines "));

    request.respond(ApprovalDecision::Deny);
    assert_eq!(worker.join().unwrap(), ApprovalDecision::Deny);
}
