use agent_app::{
  ActivityPhase, ActivityView, ContextSummary, ModelFallbackView, ModelRoute, RouteSummary,
  RuntimePhase, RuntimeSnapshot, ToolActivityState, ToolActivityView, TurnStatus, TurnView,
};
use agent_cli::tui::{ActivityTarget, Locale, TuiState};
use agent_core::{StepId, StepOutcome, StopReason, TokenUsage, ToolCallId, ToolResult, TurnId};

#[test]
fn projection_preserves_creation_order_and_uses_stable_targets() {
  let first_turn = TurnId::new();
  let second_turn = TurnId::new();
  let first_step = StepId::new();
  let second_step = StepId::new();
  let mut state = TuiState::default();

  state.sync_snapshot(&snapshot(vec![
    turn(first_turn, vec![activity(first_step, "first")]),
    turn(second_turn, vec![activity(second_step, "second")]),
  ]));

  assert_eq!(
    state
      .activities()
      .iter()
      .map(|item| item.target)
      .collect::<Vec<_>>(),
    [
      ActivityTarget {
        turn_id: first_turn,
        step_id: first_step,
      },
      ActivityTarget {
        turn_id: second_turn,
        step_id: second_step,
      },
    ]
  );
}

#[test]
fn selected_detail_contains_snapshot_evidence_without_debug_labels() {
  let turn_id = TurnId::new();
  let step_id = StepId::new();
  let item = rich_activity(step_id);
  let success_call = item.tools[0].call_id;
  let failed_call = item.tools[1].call_id;
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot(vec![turn(turn_id, vec![item])]));
  state.open_activity();

  let english = state.activity_detail(Locale::EnUs).join("\n");
  let chinese = state.activity_detail(Locale::ZhCn).join("\n");

  for evidence in [
    "provider-safe/model-safe",
    "route reason",
    "provider-old/model-old",
    "provider-safe/model-safe",
    "fallback reason",
    "system 3",
    "context 21",
    "dropped 2",
    "input 34",
    "output 55",
    "cache read 8",
    "assistant output",
    "shell",
    "\"command\": \"echo ok\"",
    "reviewer",
    "tool output",
    "duration 17 ms",
    "permission denied",
    "source marked output truncated",
    "outcome text",
    "tool calls: 2",
    "escalated: yes",
  ] {
    assert!(english.contains(evidence), "missing {evidence}:\n{english}");
  }
  for id in [
    turn_id.to_string(),
    step_id.to_string(),
    success_call.to_string(),
    failed_call.to_string(),
  ] {
    assert!(english.contains(&id), "missing id {id}:\n{english}");
  }
  assert!(english.contains("completed"), "{english}");
  assert!(chinese.contains("已完成"), "{chinese}");
  assert!(!chinese.contains("Completed"), "{chinese}");
  assert!(!chinese.contains("ToolUse"), "{chinese}");
}

#[test]
fn dynamic_fields_are_terminal_safe_and_large_payloads_are_bounded() {
  let mut item = rich_activity(StepId::new());
  item.assistant_text = format!("\u{1b}[31mred\u{1b}[0m\u{202e}{}", "x".repeat(20_000));
  item.route.as_mut().unwrap().selected.provider = format!("\u{202e}{}", "p".repeat(20_000));
  item.tools[0].name = format!("bad\nname\u{202e}{}", "n".repeat(20_000));
  item.tools[0].arguments = serde_json::json!({"payload": "y".repeat(20_000)});
  item.tools[0].result.as_mut().unwrap().output = "z".repeat(20_000);
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot(vec![turn(TurnId::new(), vec![item])]));
  state.open_activity();

  let detail = state.activity_detail(Locale::EnUs).join("\n");

  assert!(!detail.contains('\u{1b}'), "{detail}");
  assert!(!detail.contains('\u{202e}'), "{detail}");
  assert!(detail.contains('�'), "{detail}");
  assert!(detail.contains("truncated"), "{detail}");
  assert!(detail.len() < 10_000, "detail was {} bytes", detail.len());
}

fn snapshot(turns: Vec<TurnView>) -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    turns,
    ..RuntimeSnapshot::default()
  }
}

fn turn(turn_id: TurnId, activities: Vec<ActivityView>) -> TurnView {
  TurnView {
    turn_id,
    user_text: "task".into(),
    steering: vec![],
    assistant_text: String::new(),
    status: TurnStatus::Running,
    activities,
  }
}

fn activity(step_id: StepId, text: &str) -> ActivityView {
  ActivityView {
    step_id,
    phase: ActivityPhase::Streaming,
    route: None,
    context: None,
    assistant_text: text.into(),
    usage: None,
    tools: vec![],
    outcome: None,
  }
}

fn rich_activity(step_id: StepId) -> ActivityView {
  let call_id = ToolCallId::new();
  let failed_call_id = ToolCallId::new();
  ActivityView {
    step_id,
    phase: ActivityPhase::Completed,
    route: Some(RouteSummary {
      selected: ModelRoute {
        provider: "provider-safe".into(),
        model: "model-safe".into(),
      },
      reason: "route reason".into(),
      fallbacks: vec![ModelFallbackView {
        from: ModelRoute {
          provider: "provider-old".into(),
          model: "model-old".into(),
        },
        to: ModelRoute {
          provider: "provider-safe".into(),
          model: "model-safe".into(),
        },
        reason: "fallback reason".into(),
      }],
    }),
    context: Some(ContextSummary {
      system_tokens: 3,
      context_tokens: 21,
      dropped: 2,
    }),
    assistant_text: "assistant output".into(),
    usage: Some(TokenUsage {
      input: 34,
      output: 55,
      cache_read: 8,
    }),
    tools: vec![
      ToolActivityView {
        call_id,
        name: "shell".into(),
        arguments: serde_json::json!({"command": "echo ok"}),
        state: ToolActivityState::Completed,
        approved_by: Some("reviewer".into()),
        result: Some(ToolResult {
          invocation_id: call_id,
          ok: true,
          output: "tool output".into(),
          truncated: false,
          duration_ms: 17,
          error: None,
        }),
      },
      ToolActivityView {
        call_id: failed_call_id,
        name: "filesystem".into(),
        arguments: serde_json::json!({"path": "blocked"}),
        state: ToolActivityState::Failed,
        approved_by: None,
        result: Some(ToolResult {
          invocation_id: failed_call_id,
          ok: false,
          output: String::new(),
          truncated: true,
          duration_ms: 4,
          error: Some("permission denied".into()),
        }),
      },
    ],
    outcome: Some(StepOutcome {
      stop_reason: StopReason::ToolUse,
      text: "outcome text".into(),
      tool_calls: vec![call_id, failed_call_id],
      escalated: true,
    }),
  }
}
