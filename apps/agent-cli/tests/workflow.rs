use agent_cli::workflow::{ApprovalSelection, TaskOperation, TaskSelection, safe_text};
use agent_protocol::v1;
use serde_json::json;

fn approval(id: &str) -> v1::Approval {
  v1::Approval {
    id: id.into(),
    run_id: "run".into(),
    tool: "workspace.write".into(),
    arguments_json: "{\"path\":\"file.rs\",\"content\":\"new text\"}".into(),
    status: "pending".into(),
    expires_at_ms: 100,
  }
}

#[test]
fn approval_selection_tracks_exact_identity_and_clears_a_removed_request() {
  let mut state = ApprovalSelection::default();
  state.replace(vec![approval("first"), approval("second")]);
  state.next();
  assert_eq!(state.decision_id(99), Some("second"));
  state.replace(vec![approval("second"), approval("first")]);
  assert_eq!(state.decision_id(99), Some("second"));
  assert_eq!(state.decision_id(100), None);
  let mut resolved = approval("second");
  resolved.status = "approved".into();
  state.replace(vec![resolved, approval("first")]);
  assert_eq!(state.decision_id(99), None);
  state.replace(vec![approval("first")]);
  assert_eq!(state.selected_id(), None);
  assert_eq!(state.decision_id(99), None);
  state.previous();
  assert_eq!(state.decision_id(99), Some("first"));
  state.replace(Vec::new());
  assert_eq!(state.selected_id(), None);
}

#[test]
fn approval_preserves_exact_args_and_safely_expands_long_content() {
  let mut state = ApprovalSelection::default();
  let mut row = approval("unsafe\u{1b}[2J");
  let arguments = format!(
    "{{\"path\":\"test\",\"content\":\"{}\"}}\n{}\u{1b}[31m\u{202e}secret",
    "a".repeat(260),
    "more\n".repeat(15)
  );
  row.arguments_json = arguments.clone();
  state.replace(vec![row]);
  assert_eq!(
    state.selected().map(|row| row.arguments_json.as_str()),
    Some(arguments.as_str())
  );
  let collapsed = state.detail().to_string();
  assert!(collapsed.contains("expand"));
  assert!(!collapsed.contains('\u{1b}'));
  state.toggle_expanded();
  let expanded = state.detail().to_string();
  assert!(expanded.contains(&safe_text(&arguments)));
  assert!(!expanded.contains('\u{1b}'));
  assert!(!expanded.contains('\u{202e}'));
  assert!(expanded.contains("\\u{202e}"));
}

fn task(id: &str, status: &str, attempt: &str) -> serde_json::Value {
  json!({"envelope":{"task_id":id,"attempt_id":"attempt"},"status":status,"attempt_status":attempt,"result":"worker output","error":""})
}

#[test]
fn task_intents_keep_identity_and_observe_result_state() {
  let mut state = TaskSelection::default();
  state.replace(vec![
    task("a", "running", "running"),
    task("b", "awaiting_acceptance", "succeeded"),
  ]);
  assert!(state.intent(TaskOperation::Accept).is_err());
  assert!(state.intent(TaskOperation::Cancel).is_ok());
  let inspect = state
    .intent(TaskOperation::Inspect)
    .expect("local inspection");
  assert_eq!(inspect.task_id, "a");
  state.next();
  state.replace(vec![
    task("b", "awaiting_acceptance", "succeeded"),
    task("a", "running", "running"),
  ]);
  for operation in [
    TaskOperation::Accept,
    TaskOperation::Reject,
    TaskOperation::Reconcile,
    TaskOperation::Cancel,
  ] {
    let intent = state.intent(operation).expect("valid explicit target");
    assert_eq!(intent.task_id, "b");
    assert_eq!(intent.operation, operation);
  }
  state.replace(vec![task("b", "completed", "succeeded")]);
  assert!(state.intent(TaskOperation::Accept).is_err());
  assert!(state.intent(TaskOperation::Cancel).is_err());
  state.replace(vec![task("a", "running", "succeeded")]);
  assert_eq!(state.selected_id(), None);
  state.next();
  assert!(state.intent(TaskOperation::Accept).is_err());
  assert!(state.detail().to_string().contains("does not mean"));
}
