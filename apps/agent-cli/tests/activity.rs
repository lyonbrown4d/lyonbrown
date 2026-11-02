use agent_cli::{
  activity::{ActivityKind, ActivityState, ActivityStatus, ReadingState},
  config::ThemeChoice,
  theme::Theme,
};
use agent_core::{
  RunStatus,
  events::{AgentEvent, ExecutionOutcome, ExecutionPhase, ModelIdentity, TokenUsage},
};
use agent_protocol::v1::Event;
use serde_json::json;

fn event(run: &str, cursor: u64, time: i64, event: AgentEvent) -> Event {
  Event {
    run_id: run.into(),
    cursor,
    created_at_ms: time,
    kind: event.kind().into(),
    payload_json: serde_json::to_string(&event).expect("typed event fixture"),
  }
}

fn phase(phase: ExecutionPhase) -> AgentEvent {
  AgentEvent::PhaseChanged {
    phase,
    model: Some(ModelIdentity {
      provider: "fixture-provider".into(),
      model: "fixture-model".into(),
      context_window: Some(8000),
    }),
  }
}

#[test]
fn typed_tools_merge_across_live_replay_with_safe_foldable_details() {
  let events = [
    event(
      "run",
      1,
      100,
      AgentEvent::ToolStarted {
        call_id: "read".into(),
        name: "read_file".into(),
        arguments: json!({"path":"src/main.rs", "api_key":"secret-fixture", "headers":{"Authorization":"Bearer secret-fixture"}}),
      },
    ),
    event(
      "run",
      2,
      400,
      AgentEvent::ToolFinished {
        call_id: "read".into(),
        name: "read_file".into(),
        text: "cannot read\u{1b}[31m\u{202e}".into(),
        is_error: true,
      },
    ),
  ];
  let mut state = ActivityState::default();
  state.apply(&events[1]);
  state.replay(&events);
  state.replay(&events);
  assert_eq!(state.items().len(), 1);
  let tool = &state.items()[0];
  assert_eq!(tool.kind, ActivityKind::Tool);
  assert_eq!(tool.target, "src/main.rs");
  assert_eq!(tool.status, ActivityStatus::Failed);
  assert_eq!(tool.elapsed_ms(), Some(300));
  assert!(tool.result.contains("cannot read"));
  assert!(!tool.result.contains(['\u{1b}', '\u{202e}']));
  assert!(!format!("{state:?}").contains("secret-fixture"));
  let theme = Theme::resolve(ThemeChoice::Dark, None);
  let collapsed = state.text(theme).to_string();
  assert!(collapsed.contains("失败"));
  assert!(collapsed.contains("0.3 秒"));
  assert!(!collapsed.contains("参数"));
  state.toggle_selected();
  assert!(state.text(theme).to_string().contains("参数"));
  let mut other = events[0].clone();
  other.run_id = "other".into();
  assert!(state.apply(&other));
  assert_eq!(state.items().len(), 2);
}

#[test]
fn daemon_phase_and_model_are_authoritative_while_usage_is_deduplicated() {
  let usage = TokenUsage {
    input: 30,
    output: 10,
    cache_read: 5,
    cache_write: 0,
    total: 45,
  };
  let events = [
    event("run", 1, 10, phase(ExecutionPhase::ConnectingModel)),
    event("run", 2, 20, phase(ExecutionPhase::Thinking)),
    event(
      "run",
      3,
      30,
      AgentEvent::TextDelta {
        text: "not a phase transition".into(),
      },
    ),
    event(
      "run",
      4,
      40,
      AgentEvent::UsageUpdated {
        entry_id: "answer".into(),
        usage,
      },
    ),
    event(
      "run",
      5,
      50,
      AgentEvent::UsageUpdated {
        entry_id: "answer".into(),
        usage: TokenUsage {
          input: 35,
          total: 50,
          ..usage
        },
      },
    ),
    event(
      "run",
      6,
      60,
      AgentEvent::UsageUpdated {
        entry_id: "other-turn".into(),
        usage,
      },
    ),
  ];
  let mut state = ActivityState::default();
  state.apply(&events[4]);
  state.replay(&events);
  assert!(state.summary().contains("模型正在思考"));
  assert!(state.summary().contains("fixture-provider / fixture-model"));
  assert!(state.summary().contains("Token 95"));
  assert_eq!(state.token_usage().input, 65);
  assert_eq!(state.token_usage().cache_read, 10);
  assert_eq!(
    state.items()[0]
      .model
      .as_ref()
      .expect("daemon model")
      .context_window,
    Some(8000)
  );
  // A late history page cannot overwrite the newer explicit phase.
  let older = event("run", 0, 1, phase(ExecutionPhase::Preparing));
  state.apply(&older);
  assert!(state.summary().contains("模型正在思考"));
  let mut opaque = events[0].clone();
  opaque.cursor = 7;
  opaque.kind = "record_appended".into();
  opaque.payload_json =
    json!({"record":{"type":"assistant_frame","frame":{"type":"text_delta"}}}).to_string();
  assert!(!state.apply(&opaque));
  assert!(state.summary().contains("模型正在思考"));
}

#[test]
fn terminal_status_and_compaction_replay_do_not_fabricate_success_or_restart() {
  let mut state = ActivityState::default();
  let start = event("run", 1, 100, AgentEvent::Compaction { active: true });
  let end = event("run", 2, 200, AgentEvent::Compaction { active: false });
  state.apply(&end);
  state.apply(&start);
  assert_eq!(state.items().len(), 1);
  assert_eq!(state.items()[0].elapsed_ms(), Some(100));
  assert_eq!(state.items()[0].status, ActivityStatus::Uncertain);
  state.apply(&event("run", 3, 210, phase(ExecutionPhase::Responding)));
  state.apply(&event(
    "run",
    4,
    220,
    AgentEvent::ToolStarted {
      call_id: "write".into(),
      name: "write".into(),
      arguments: json!({}),
    },
  ));
  state.apply(&event(
    "run",
    5,
    230,
    AgentEvent::RunFinished {
      run_id: "harness-run".into(),
      outcome: ExecutionOutcome::Completed,
    },
  ));
  state.apply(&event(
    "run",
    6,
    240,
    AgentEvent::RunStateChanged {
      status: RunStatus::Cancelled,
      error: String::new(),
    },
  ));
  state.apply(&event("run", 7, 250, phase(ExecutionPhase::Thinking)));
  assert!(state.summary().contains("执行已取消"));
  assert!(
    state
      .items()
      .iter()
      .all(|item| item.status != ActivityStatus::Running)
  );
  // History that arrives after termination must not display an active tool.
  state.apply(&event(
    "run",
    0,
    1,
    AgentEvent::ToolStarted {
      call_id: "older".into(),
      name: "read".into(),
      arguments: json!({}),
    },
  ));
  assert!(
    state
      .items()
      .iter()
      .all(|item| item.status != ActivityStatus::Running)
  );
}

#[test]
fn reading_search_preserves_message_anchor_and_unicode_offsets_when_history_grows() {
  let mut reading = ReadingState::default();
  reading.sync([("one", "开始\n查找工具"), ("two", "查找模型")]);
  reading.set_query("查找");
  assert_eq!(reading.position(), Some((1, 2)));
  assert_eq!(reading.current().expect("hit").anchor.line, 1);
  assert_eq!(reading.current().expect("hit").end, 2);
  assert_eq!(reading.next_hit().expect("next").anchor.message_id, "two");
  reading.sync([
    ("zero", "查找历史"),
    ("one", "开始\n查找工具"),
    ("two", "查找模型\n新增回复"),
  ]);
  assert_eq!(reading.position(), Some((3, 3)));
  assert_eq!(reading.next_hit().expect("wrap").anchor.message_id, "zero");
  assert_eq!(
    reading.previous_hit().expect("back").anchor.message_id,
    "two"
  );
  assert_eq!(
    reading.message_index(&reading.current().expect("hit").anchor),
    Some(2)
  );
  reading.set_query("");
  assert!(reading.current().is_none());
  assert_eq!(reading.anchors().count(), 3);
}
