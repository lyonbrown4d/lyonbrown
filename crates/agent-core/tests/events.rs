use agent_core::{
  RunStatus,
  events::{
    AGENT_EVENT_KIND, AgentEvent, DecodedEvent, ExecutionOutcome, ExecutionPhase, ModelIdentity,
    QueueKind, QueueStatus, TokenUsage, decode,
  },
};
use serde_json::json;

#[test]
fn daemon_phase_and_model_have_a_stable_tagged_contract() -> Result<(), serde_json::Error> {
  let event = AgentEvent::PhaseChanged {
    phase: ExecutionPhase::ConnectingModel,
    model: Some(ModelIdentity {
      provider: "local-mock".into(),
      model: "fixture".into(),
      context_window: Some(8192),
    }),
  };
  let payload = serde_json::to_string(&event)?;
  assert_eq!(event.kind(), AGENT_EVENT_KIND);
  assert_eq!(
    serde_json::from_str::<serde_json::Value>(&payload)?,
    json!({
      "type":"phase_changed", "phase":"connecting_model",
      "model":{"provider":"local-mock","model":"fixture","context_window":8192}
    })
  );
  assert_eq!(decode(event.kind(), &payload)?, DecodedEvent::Known(event));
  Ok(())
}

#[test]
fn typed_usage_queue_and_outcome_round_trip() -> Result<(), serde_json::Error> {
  for event in [
    AgentEvent::UsageUpdated {
      entry_id: "entry".into(),
      usage: TokenUsage {
        input: 10,
        output: 5,
        cache_read: 3,
        cache_write: 2,
        total: 20,
      },
    },
    AgentEvent::QueueUpdated {
      entry_id: "entry".into(),
      kind: QueueKind::FollowUp,
      status: QueueStatus::Pending,
      prompt: Some("continue".into()),
      request_id: Some("request".into()),
    },
    AgentEvent::RunFinished {
      run_id: "run".into(),
      outcome: ExecutionOutcome::Suspended,
    },
    AgentEvent::RunStateChanged {
      status: RunStatus::Failed,
      error: "mock unavailable".into(),
    },
  ] {
    let payload = serde_json::to_string(&event)?;
    assert_eq!(
      decode(AGENT_EVENT_KIND, &payload)?,
      DecodedEvent::Known(event)
    );
  }
  assert_eq!(
    decode(
      AGENT_EVENT_KIND,
      r#"{"type":"queue_updated","entry_id":"entry","kind":"steer","status":"consumed"}"#
    )?,
    DecodedEvent::Known(AgentEvent::QueueUpdated {
      entry_id: "entry".into(),
      kind: QueueKind::Steer,
      status: QueueStatus::Consumed,
      prompt: None,
      request_id: None,
    })
  );
  Ok(())
}

#[test]
fn legacy_runtime_events_decode_without_client_phase_inference() -> Result<(), serde_json::Error> {
  for (kind, payload, expected) in [
    (
      "text_delta",
      json!({"text":"hello"}),
      AgentEvent::TextDelta {
        text: "hello".into(),
      },
    ),
    (
      "thinking_delta",
      json!({"text":"reason"}),
      AgentEvent::ThinkingDelta {
        text: "reason".into(),
      },
    ),
    (
      "tool_started",
      json!({"call_id":"call","name":"read","arguments":{"path":"file"}}),
      AgentEvent::ToolStarted {
        call_id: "call".into(),
        name: "read".into(),
        arguments: json!({"path":"file"}),
      },
    ),
    (
      "tool_finished",
      json!({"call_id":"call","name":"read","text":"content","is_error":false}),
      AgentEvent::ToolFinished {
        call_id: "call".into(),
        name: "read".into(),
        text: "content".into(),
        is_error: false,
      },
    ),
    (
      "run_finished",
      json!({"run_id":"run","outcome":"completed"}),
      AgentEvent::RunFinished {
        run_id: "run".into(),
        outcome: ExecutionOutcome::Completed,
      },
    ),
    (
      "compaction",
      json!({"active":true}),
      AgentEvent::Compaction { active: true },
    ),
  ] {
    assert_eq!(
      decode(kind, &payload.to_string())?,
      DecodedEvent::Known(expected)
    );
  }
  Ok(())
}

#[test]
fn legacy_run_status_aliases_preserve_status_and_error() -> Result<(), serde_json::Error> {
  for (name, status) in [
    ("accepted", RunStatus::Accepted),
    ("running", RunStatus::Running),
    ("awaiting_approval", RunStatus::AwaitingApproval),
    ("cancel_requested", RunStatus::CancelRequested),
    ("completed", RunStatus::Completed),
    ("failed", RunStatus::Failed),
    ("cancelled", RunStatus::Cancelled),
    ("uncertain", RunStatus::Uncertain),
  ] {
    for kind in [name.to_owned(), format!("run_{name}")] {
      assert_eq!(
        decode(&kind, &json!({"status":name,"error":"detail"}).to_string())?,
        DecodedEvent::Known(AgentEvent::RunStateChanged {
          status,
          error: "detail".into()
        })
      );
      assert_eq!(
        decode(&kind, "{}")?,
        DecodedEvent::Known(AgentEvent::RunStateChanged {
          status,
          error: String::new()
        })
      );
    }
  }
  Ok(())
}

#[test]
fn malformed_known_payloads_are_errors_instead_of_unknown_events() {
  for (kind, payload) in [
    (AGENT_EVENT_KIND, "not json"),
    (AGENT_EVENT_KIND, r#"{"type":"text_delta"}"#),
    (
      AGENT_EVENT_KIND,
      r#"{"type":"phase_changed","phase":"future_phase"}"#,
    ),
    (
      AGENT_EVENT_KIND,
      r#"{"type":"run_finished","run_id":"run","outcome":"future_outcome"}"#,
    ),
    ("text_delta", r#"{"text":2}"#),
    ("tool_finished", r#"{"name":"read"}"#),
    ("compaction", "[]"),
    ("text_delta", r#"{"type":"thinking_delta","text":"hello"}"#),
    ("run_failed", r#"{"status":"completed"}"#),
    ("run_completed", r#"{"status":"future_status"}"#),
    ("run_uncertain", r#"{"error":7}"#),
  ] {
    assert!(
      decode(kind, payload).is_err(),
      "accepted malformed {kind}: {payload}"
    );
  }
}

#[test]
fn unknown_and_future_events_discard_payloads_without_inventing_phases()
-> Result<(), serde_json::Error> {
  for kind in [
    "future_outer",
    "run_started",
    "entry_appended",
    "record_appended",
  ] {
    assert_eq!(
      decode(kind, "untrusted non-json payload")?,
      DecodedEvent::Unknown { kind: kind.into() }
    );
  }
  assert_eq!(
    decode(
      AGENT_EVENT_KIND,
      r#"{"type":"future_inner","secret":"discard this"}"#
    )?,
    DecodedEvent::Unknown {
      kind: "future_inner".into()
    }
  );
  Ok(())
}
