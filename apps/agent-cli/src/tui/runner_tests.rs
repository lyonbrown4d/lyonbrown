use std::{sync::Arc, thread};

use agent_core::{
    AgentEvent, RiskLevel, ToolCallId, ToolDefinition, ToolInvocation, ToolSource, TurnId,
};
use agent_kernel::Kernel;
use agent_tool::{ApprovalDecision, ApprovalPrompt};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use tokio::sync::broadcast;

use super::{Runtime, TuiState, can_accept_input, final_drain, quit_key, shutdown};
use crate::tui::TuiApprovalPrompt;

#[test]
fn final_drain_consumes_events_queued_at_task_completion() {
    let (sender, receiver) = broadcast::channel(4);
    let mut receiver = Some(receiver);
    let mut state = TuiState::default();
    sender
        .send(AgentEvent::TurnCompleted {
            turn_id: TurnId::new(),
            answer: "last answer".into(),
            ts: Utc::now(),
        })
        .unwrap();

    final_drain(&mut receiver, &mut state);

    assert_eq!(state.transcript().last().unwrap().text, "last answer");
    assert_eq!(state.status(), "idle");
}

#[test]
fn input_remains_locked_until_task_join_returns_kernel() {
    let mut state = TuiState::default();
    state.reduce(AgentEvent::TurnCompleted {
        turn_id: TurnId::new(),
        answer: "rendered before join".into(),
        ts: Utc::now(),
    });
    assert_eq!(state.status(), "idle");
    assert!(!can_accept_input(true, false));
    assert!(!can_accept_input(true, true));
    assert!(!can_accept_input(false, false));
    assert!(can_accept_input(false, true));
}

#[test]
fn escape_and_control_c_are_unconditional_quit_keys() {
    assert!(quit_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(quit_key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    )));
}

#[tokio::test]
async fn shutdown_denies_approval_cancels_and_joins_turn_task() {
    let (prompt, approvals) = TuiApprovalPrompt::channel();
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
        arguments: json!({"command": "echo pending"}),
    };
    let prompt = Arc::new(prompt);
    let approval_worker = thread::spawn(move || prompt.prompt(&definition, &invocation));
    let pending = approvals.recv().unwrap();

    let kernel = Kernel::builder().build();
    let cancellation = kernel.cancellation_token();
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        task_cancellation.cancelled().await;
        (kernel, Err(agent_core::AgentError::Cancelled))
    });
    let mut runtime = Runtime {
        state: TuiState::default(),
        kernel: None,
        events: None,
        task: Some(task),
        approval: Some(pending),
        approvals: Some(approvals),
        approval_scroll: 0,
        approval_max_scroll: 0,
    };

    shutdown(&mut runtime, &cancellation).await;

    assert!(cancellation.is_cancelled());
    assert!(runtime.task.is_none());
    assert_eq!(approval_worker.join().unwrap(), ApprovalDecision::Deny);
}
