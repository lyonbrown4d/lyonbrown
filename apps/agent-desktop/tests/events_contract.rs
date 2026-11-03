#[path = "../src/events.rs"]
mod events;

use agent_core::events::{ExecutionPhase, ModelIdentity, TokenUsage};
use agent_protocol::v1::Event;
use events::{RunEvent, decode};

fn event(kind: &str, payload: &str) -> Event {
  Event {
    cursor: 7,
    run_id: "run-1".into(),
    kind: kind.into(),
    payload_json: payload.into(),
    created_at_ms: 0,
  }
}

#[test]
fn only_tagged_text_contributes_a_transcript_increment() {
  let decoded = decode(&event(
    "agent_event",
    r#"{"type":"text_delta","text":"hello"}"#,
  ));
  assert_eq!(
    decoded.event,
    RunEvent::TextDelta {
      text: "hello".into()
    }
  );
  assert_eq!(decoded.status(), None);
  assert_eq!(
    decode(&event("text_delta", r#"{"text":"hello"}"#)).event,
    RunEvent::Other
  );
}

#[test]
fn phase_model_and_usage_are_shared_typed_facts() {
  let phase = decode(&event(
    "agent_event",
    r#"{"type":"phase_changed","phase":"connecting_model","model":{"provider":"local","model":"mock","context_window":4096}}"#,
  ));
  let model = ModelIdentity {
    provider: "local".into(),
    model: "mock".into(),
    context_window: Some(4096),
  };
  assert_eq!(
    phase.event,
    RunEvent::PhaseChanged {
      phase: ExecutionPhase::ConnectingModel,
      model: Some(model.clone()),
    }
  );
  assert_eq!(
    events::phase_label(ExecutionPhase::ConnectingModel, Some(&model)),
    "Connecting model · local / mock"
  );
  let usage = decode(&event(
    "agent_event",
    r#"{"type":"usage_updated","entry_id":"e1","usage":{"input":2,"output":3,"cache_read":0,"cache_write":0,"total":5}}"#,
  ));
  assert_eq!(
    usage.event,
    RunEvent::UsageUpdated {
      usage: TokenUsage {
        input: 2,
        output: 3,
        cache_read: 0,
        cache_write: 0,
        total: 5
      },
    }
  );
}

#[test]
fn tools_retain_only_reviewed_labels() {
  let started = decode(&event(
    "agent_event",
    r#"{"type":"tool_started","call_id":"c1","name":"read_file","arguments":{"secret":"redacted"}}"#,
  ));
  assert_eq!(
    started.event,
    RunEvent::ToolStarted {
      name: "read_file".into()
    }
  );
  let finished = decode(&event(
    "agent_event",
    r#"{"type":"tool_finished","call_id":"c1","name":"read_file","text":"sensitive result","is_error":false}"#,
  ));
  assert_eq!(
    finished.event,
    RunEvent::ToolFinished {
      name: "read_file".into()
    }
  );
  assert!(!format!("{started:?} {finished:?}").contains("secret"));
  assert!(!format!("{finished:?}").contains("sensitive result"));
}

#[test]
fn unknown_and_malformed_payloads_cannot_supply_status_or_secrets() {
  for (kind, payload) in [
    ("future_audit", r#"{"status":"completed","token":"secret"}"#),
    ("agent_event", r#"{"type":"future_event","token":"secret"}"#),
    ("text_delta", "not json"),
    ("tool_started", "{}"),
    ("agent_event", r#"{"type":"approval_required"}"#),
  ] {
    let decoded = decode(&event(kind, payload));
    assert_eq!(decoded.event, RunEvent::Other);
    assert_eq!(decoded.status(), None);
    assert!(!format!("{decoded:?}").contains("secret"));
  }
  assert_eq!(
    decode(&event("agent_event", r#"{"type":"future_event"}"#)).kind,
    "future_event"
  );
}

#[test]
fn only_durable_lifecycle_kinds_end_the_watcher() {
  for status in ["completed", "failed", "cancelled", "uncertain"] {
    let kind = format!("run_{status}");
    let decoded = decode(&event(&kind, &format!(r#"{{"status":"{status}"}}"#)));
    assert_eq!(decoded.status(), Some(status));
    assert!(events::is_terminal(&kind));
  }
  let finished = decode(&event(
    "agent_event",
    r#"{"type":"run_finished","run_id":"run-1","outcome":"completed"}"#,
  ));
  assert_eq!(finished.status(), None);
  assert!(!events::is_terminal("agent_event"));
  assert!(!events::is_terminal("run_finished"));
  let state = decode(&event(
    "agent_event",
    r#"{"type":"run_state_changed","status":"completed","error":""}"#,
  ));
  assert_eq!(state.status(), Some("completed"));
  assert!(events::is_approval_required("approval_required"));
  assert!(!events::is_approval_required("agent_event"));
  assert_eq!(
    decode(&event("approval_required", "{}")).status(),
    Some("awaiting_approval")
  );
}
