use agent_core::{Approval, Run, RunStatus};
use agent_storage::{ApprovedOperation, OperationStatus, Store};
use diesel::{connection::SimpleConnection, prelude::*};
use serde_json::{Value, json};

fn running(store: &Store, key: &str) -> anyhow::Result<Run> {
  let session = store.create_session(key, "/fixture", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "native operation", key)?;
  store.update_run_owned(&run.id, 1, RunStatus::Running, "", "")
}

fn approved(store: &Store, run: &str, arguments: &Value) -> anyhow::Result<Approval> {
  let approval = store.request_approval_owned(run, 1, "computer_click", arguments)?;
  store.decide_approval(&approval.id, true)
}

fn input<'a>(run: &'a str, approval: &'a str, arguments: &'a Value) -> ApprovedOperation<'a> {
  ApprovedOperation {
    run_id: run,
    owner_epoch: 1,
    action_id: "native_action",
    tool: "computer_click",
    arguments,
    summary: json!({"argument_count":2}),
    approval_id: approval,
  }
}

#[test]
fn proof_binds_committed_approval_intent_and_outbox() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let run = running(&store, "success")?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let approval = approved(&store, &run.id, &arguments)?;
  let before = store.events(&run.id, 0, 100)?;
  let proof = store.begin_approved_operation(input(&run.id, &approval.id, &arguments))?;
  assert_eq!(proof.action_id(), "native_action");
  assert_eq!(proof.approval_id(), approval.id);
  assert_eq!(proof.run_id(), run.id);
  assert_eq!(proof.owner_epoch(), 1);
  // Reopen independently to prove durable state, not an in-memory receipt.
  let reopened = Store::open(&path)?;
  assert_eq!(
    reopened.approval(&approval.id)?.expect("approval").status,
    "consumed"
  );
  let intent = reopened.operation(proof.action_id())?.expect("intent");
  assert_eq!(intent.status, OperationStatus::Intent);
  assert_eq!(intent.run_id, proof.run_id());
  assert_eq!(intent.owner_epoch, proof.owner_epoch());
  assert_eq!(intent.kind, "computer_click");
  assert_eq!(intent.intent_summary, json!({"argument_count":2}));
  assert_eq!(intent.result_summary, None);
  let events = reopened.events(&run.id, 0, 100)?;
  assert_eq!(events.len(), before.len() + 2);
  let authorized = &events[before.len()];
  assert_eq!(authorized.kind, "tool_authorized");
  assert_eq!(
    serde_json::from_str::<Value>(&authorized.payload_json)?,
    json!({"approval_id":approval.id,"tool":"computer_click"})
  );
  let intent = &events[before.len() + 1];
  assert_eq!(intent.kind, "operation_intent");
  assert_eq!(
    serde_json::from_str::<Value>(&intent.payload_json)?,
    json!({"action_id":"native_action","kind":"computer_click"})
  );
  assert_eq!(reopened.pending_outbox(100)?, events);
  assert!(
    reopened
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .is_err()
  );
  assert_eq!(reopened.events(&run.id, 0, 100)?, events);
  Ok(())
}

#[test]
fn binding_lifecycle_and_metadata_rejections_preserve_approval_and_intent() -> anyhow::Result<()> {
  for case in [
    "arguments",
    "run",
    "tool",
    "epoch",
    "denied",
    "pending",
    "cancelled",
    "summary",
    "action",
    "bound_action",
  ] {
    let store = Store::open(":memory:")?;
    let run = running(&store, case)?;
    let other = running(&store, "other")?;
    let arguments = json!({"action_id":"native_action","x":12,"y":34});
    let changed_arguments = json!({"action_id":"native_action","x":13,"y":34});
    let approval = store.request_approval_owned(&run.id, 1, "computer_click", &arguments)?;
    if case != "pending" {
      store.decide_approval(&approval.id, case != "denied")?;
    }
    if case == "cancelled" {
      store.update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")?;
    }
    if case == "epoch" {
      store.change_owner(&run.session_id, 1, "replacement")?;
    }
    let before = store.events(&run.id, 0, 100)?;
    let outbox_before = store.pending_outbox(100)?;
    let approval_before = store.approval(&approval.id)?;
    let mut request = input(&run.id, &approval.id, &arguments);
    match case {
      "arguments" => request.arguments = &changed_arguments,
      "run" => request.run_id = &other.id,
      "tool" => request.tool = "computer_type",
      "summary" => request.summary = json!({"screenshot":"private pixels"}),
      "action" => request.action_id = "",
      "bound_action" => request.action_id = "substituted_action",
      _ => {}
    }
    assert!(store.begin_approved_operation(request).is_err(), "{case}");
    assert_eq!(store.approval(&approval.id)?, approval_before, "{case}");
    assert!(store.operation("native_action")?.is_none(), "{case}");
    assert!(store.operation("substituted_action")?.is_none(), "{case}");
    assert_eq!(store.events(&run.id, 0, 100)?, before, "{case}");
    assert_eq!(store.pending_outbox(100)?, outbox_before, "{case}");
  }
  Ok(())
}

#[test]
fn duplicate_action_rolls_back_consumption_and_authorized_event() -> anyhow::Result<()> {
  let store = Store::open(":memory:")?;
  let run = running(&store, "duplicate")?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  store.begin_operation(&run.id, 1, "native_action", "computer_click", json!({}))?;
  let existing = store.operation("native_action")?;
  let approval = approved(&store, &run.id, &arguments)?;
  let before = store.events(&run.id, 0, 100)?;
  assert!(
    store
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .is_err()
  );
  assert_eq!(
    store.approval(&approval.id)?.expect("approval").status,
    "approved"
  );
  assert_eq!(store.operation("native_action")?, existing);
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  assert_eq!(store.pending_outbox(100)?, before);
  Ok(())
}

#[test]
fn intent_outbox_failure_rolls_back_consumption_intent_and_both_events() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let run = running(&store, "outbox")?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let approval = approved(&store, &run.id, &arguments)?;
  let before = store.events(&run.id, 0, 100)?;
  let mut injected = SqliteConnection::establish(path.to_str().expect("path"))?;
  // Fail the second event's outbox write, after the authorization event succeeded.
  injected.batch_execute("CREATE TRIGGER reject_intent_outbox BEFORE INSERT ON outbox WHEN EXISTS (SELECT 1 FROM operations WHERE action_id = 'native_action') BEGIN SELECT RAISE(ABORT, 'injected storage fault'); END;")?;
  assert!(
    store
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .is_err()
  );
  assert_eq!(
    store.approval(&approval.id)?.expect("approval").status,
    "approved"
  );
  assert!(store.operation("native_action")?.is_none());
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  assert_eq!(store.pending_outbox(100)?, before);
  injected.batch_execute("DROP TRIGGER reject_intent_outbox;")?;
  store.begin_approved_operation(input(&run.id, &approval.id, &arguments))?;
  assert_eq!(
    store.approval(&approval.id)?.expect("approval").status,
    "consumed"
  );
  assert_eq!(store.pending_outbox(100)?, store.events(&run.id, 0, 100)?);
  Ok(())
}

#[test]
fn concurrent_writers_can_consume_the_bound_action_approval_only_once() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let run = running(&store, "race")?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let approval = approved(&store, &run.id, &arguments)?;
  let left = Store::open(&path)?;
  let right = Store::open(&path)?;
  let before = store.events(&run.id, 0, 100)?;
  let barrier = std::sync::Barrier::new(2);
  let results = std::thread::scope(|scope| {
    let first = scope.spawn(|| {
      barrier.wait();
      left.begin_approved_operation(input(&run.id, &approval.id, &arguments))
    });
    let second = scope.spawn(|| {
      barrier.wait();
      right.begin_approved_operation(input(&run.id, &approval.id, &arguments))
    });
    [
      first.join().expect("first writer"),
      second.join().expect("second writer"),
    ]
  });
  assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
  let winner = results.into_iter().find_map(Result::ok).expect("winner");
  assert_eq!(winner.approval_id(), approval.id);
  assert_eq!(
    store.operation(winner.action_id())?.expect("intent").status,
    OperationStatus::Intent
  );
  assert_eq!(winner.action_id(), "native_action");
  assert_eq!(
    store.approval(&approval.id)?.expect("approval").status,
    "consumed"
  );
  let events = store.events(&run.id, 0, 100)?;
  assert_eq!(events.len(), before.len() + 2);
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "tool_authorized")
      .count(),
    1
  );
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "operation_intent")
      .count(),
    1
  );
  assert_eq!(store.pending_outbox(100)?, events);
  Ok(())
}

#[test]
fn expired_approved_record_cannot_authorize_an_intent() -> anyhow::Result<()> {
  diesel::table! {
    approvals (id) {
      id -> Text,
      run_id -> Text,
      status -> Text,
      record -> Text,
    }
  }
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let run = running(&store, "expired")?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let mut approval = approved(&store, &run.id, &arguments)?;
  approval.expires_at_ms = 0;
  let mut connection = SqliteConnection::establish(path.to_str().expect("path"))?;
  diesel::update(approvals::table.find(&approval.id))
    .set(approvals::record.eq(serde_json::to_string(&approval)?))
    .execute(&mut connection)?;
  let before = store.events(&run.id, 0, 100)?;
  assert!(
    store
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .is_err()
  );
  assert_eq!(store.approval(&approval.id)?, Some(approval));
  assert!(store.operation("native_action")?.is_none());
  assert_eq!(store.events(&run.id, 0, 100)?, before);
  assert_eq!(store.pending_outbox(100)?, before);
  Ok(())
}
