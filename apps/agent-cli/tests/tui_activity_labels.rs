use agent_app::{
  ActivityPhase, ActivityView, RuntimePhase, RuntimeSnapshot, ToolActivityState, ToolActivityView,
  TurnStatus, TurnView,
};
use agent_cli::tui::{Locale, TuiState};
use agent_core::{StepId, StepOutcome, StopReason, ToolCallId, TurnId};

#[test]
fn every_activity_phase_has_explicit_english_and_chinese_labels() {
  let cases = [
    (ActivityPhase::Preparing, "preparing", "准备中"),
    (ActivityPhase::Streaming, "streaming", "生成中"),
    (
      ActivityPhase::AwaitingApproval,
      "awaiting approval",
      "等待审批",
    ),
    (ActivityPhase::RunningTool, "running tool", "工具运行中"),
    (ActivityPhase::Superseded, "superseded", "已被替代"),
    (ActivityPhase::Completed, "completed", "已完成"),
    (ActivityPhase::Failed, "failed", "失败"),
  ];
  for (phase, english, chinese) in cases {
    let activity = base_activity(phase);
    assert_labels(activity.clone(), english, chinese);
    let zh = rendered(activity, Locale::ZhCn);
    assert!(!zh.contains(&format!("{phase:?}")), "{zh}");
  }
}

#[test]
fn every_tool_state_has_explicit_english_and_chinese_labels() {
  let cases = [
    (ToolActivityState::Requested, "requested", "已请求"),
    (ToolActivityState::Approved, "approved", "已批准"),
    (ToolActivityState::Running, "running tool", "工具运行中"),
    (ToolActivityState::Completed, "completed", "已完成"),
    (ToolActivityState::Failed, "failed", "失败"),
  ];
  for (tool_state, english, chinese) in cases {
    let mut activity = base_activity(ActivityPhase::Preparing);
    activity.tools.push(ToolActivityView {
      call_id: ToolCallId::new(),
      name: "tool-name".into(),
      arguments: serde_json::json!({}),
      state: tool_state,
      approved_by: None,
      result: None,
    });
    let en = rendered(activity.clone(), Locale::EnUs);
    let zh = rendered(activity, Locale::ZhCn);
    assert_tool_line(&en, "tool 1:", english);
    assert_tool_line(&zh, "工具 1:", chinese);
    assert!(!zh.contains(&format!("{tool_state:?}")), "{zh}");
  }
}

#[test]
fn every_outcome_stop_reason_has_explicit_english_and_chinese_labels() {
  let cases = [
    (StopReason::EndTurn, "end turn", "结束回合"),
    (StopReason::ToolUse, "tool use", "调用工具"),
    (StopReason::MaxTokens, "max tokens", "达到最大令牌数"),
    (StopReason::Stop, "stopped", "已停止"),
    (
      StopReason::Error("model failed".into()),
      "error: model failed",
      "错误: model failed",
    ),
  ];
  for (stop_reason, english, chinese) in cases {
    let outcome_call_id = ToolCallId::new();
    let mut activity = base_activity(ActivityPhase::Completed);
    activity.outcome = Some(StepOutcome {
      stop_reason,
      text: "final text".into(),
      tool_calls: vec![outcome_call_id],
      escalated: false,
    });
    let en = rendered(activity.clone(), Locale::EnUs);
    let zh = rendered(activity, Locale::ZhCn);
    assert_outcome_line(&en, "outcome:", english);
    assert_outcome_line(&zh, "结果:", chinese);
    for output in [&en, &zh] {
      let id = outcome_call_id.to_string();
      assert_eq!(output.matches(&id).count(), 1, "{output}");
      assert!(output.lines().any(|line| line == format!("  call: {id}")));
    }
  }
}

fn assert_tool_line(output: &str, prefix: &str, label: &str) {
  let line = output
    .lines()
    .find(|line| line.starts_with(prefix))
    .unwrap_or_else(|| panic!("missing {prefix}:\n{output}"));
  assert!(line.contains(&format!(" · {label} · ")), "{line}");
}

fn assert_outcome_line(output: &str, prefix: &str, label: &str) {
  let line = output
    .lines()
    .find(|line| line.starts_with(prefix))
    .unwrap_or_else(|| panic!("missing {prefix}:\n{output}"));
  assert_eq!(line, format!("{prefix} {label}"));
}

fn assert_labels(activity: ActivityView, english: &str, chinese: &str) {
  let en = rendered(activity.clone(), Locale::EnUs);
  let zh = rendered(activity, Locale::ZhCn);
  assert!(en.contains(english), "missing {english}:\n{en}");
  assert!(zh.contains(chinese), "missing {chinese}:\n{zh}");
}

fn rendered(activity: ActivityView, locale: Locale) -> String {
  let mut state = TuiState::default();
  state.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    turns: vec![TurnView {
      turn_id: TurnId::new(),
      user_text: "labels".into(),
      steering: vec![],
      assistant_text: String::new(),
      status: TurnStatus::Running,
      activities: vec![activity],
    }],
    ..RuntimeSnapshot::default()
  });
  state.open_activity();
  state.activity_detail(locale).join("\n")
}

fn base_activity(phase: ActivityPhase) -> ActivityView {
  ActivityView {
    step_id: StepId::new(),
    phase,
    route: None,
    context: None,
    assistant_text: String::new(),
    usage: None,
    tools: vec![],
    outcome: None,
  }
}
