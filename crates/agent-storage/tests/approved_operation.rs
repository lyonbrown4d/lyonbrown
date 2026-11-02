mod common;
use agent_core::{Approval, Run, RunStatus};
use agent_storage::{ApprovedOperation, OperationStatus, Store};
use common::approvals;
use sea_orm::prelude::*;
use serde_json::{Value, json};

async fn running(store: &Store, key: &str) -> anyhow::Result<Run> {
  let session = store.create_session(key, "/fixture", "local").await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "native operation", key)
    .await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await
}

async fn approved(store: &Store, run: &str, arguments: &Value) -> anyhow::Result<Approval> {
  let approval = store
    .request_approval_owned(run, 1, "computer_click", arguments)
    .await?;
  store.decide_approval(&approval.id, true).await
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

#[tokio::test]
async fn proof_binds_committed_approval_intent_and_outbox() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "success").await?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let approval = approved(&store, &run.id, &arguments).await?;
  let before = store.events(&run.id, 0, 100).await?;
  let proof = store
    .begin_approved_operation(input(&run.id, &approval.id, &arguments))
    .await?;
  assert_eq!(proof.action_id(), "native_action");
  assert_eq!(proof.approval_id(), approval.id);
  assert_eq!(proof.run_id(), run.id);
  assert_eq!(proof.owner_epoch(), 1);
  // Reopen independently to prove durable state, not an in-memory receipt.
  let reopened = Store::open(&path).await?;
  assert_eq!(
    reopened
      .approval(&approval.id)
      .await?
      .expect("approval")
      .status,
    "consumed"
  );
  let intent = reopened
    .operation(proof.action_id())
    .await?
    .expect("intent");
  assert_eq!(intent.status, OperationStatus::Intent);
  assert_eq!(intent.run_id, proof.run_id());
  assert_eq!(intent.owner_epoch, proof.owner_epoch());
  assert_eq!(intent.kind, "computer_click");
  assert_eq!(intent.intent_summary, json!({"argument_count":2}));
  assert_eq!(intent.result_summary, None);
  let events = reopened.events(&run.id, 0, 100).await?;
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
  assert_eq!(reopened.pending_outbox(100).await?, events);
  assert!(
    reopened
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .await
      .is_err()
  );
  assert_eq!(reopened.events(&run.id, 0, 100).await?, events);
  Ok(())
}

#[tokio::test]
async fn binding_lifecycle_and_metadata_rejections_preserve_approval_and_intent()
-> anyhow::Result<()> {
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
    let store = Store::open(":memory:").await?;
    let run = running(&store, case).await?;
    let other = running(&store, "other").await?;
    let arguments = json!({"action_id":"native_action","x":12,"y":34});
    let changed_arguments = json!({"action_id":"native_action","x":13,"y":34});
    let approval = store
      .request_approval_owned(&run.id, 1, "computer_click", &arguments)
      .await?;
    if case != "pending" {
      store
        .decide_approval(&approval.id, case != "denied")
        .await?;
    }
    if case == "cancelled" {
      store
        .update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")
        .await?;
    }
    if case == "epoch" {
      store
        .change_owner(&run.session_id, 1, "replacement")
        .await?;
    }
    let before = store.events(&run.id, 0, 100).await?;
    let outbox_before = store.pending_outbox(100).await?;
    let approval_before = store.approval(&approval.id).await?;
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
    assert!(
      store.begin_approved_operation(request).await.is_err(),
      "{case}"
    );
    assert_eq!(
      store.approval(&approval.id).await?,
      approval_before,
      "{case}"
    );
    assert!(store.operation("native_action").await?.is_none(), "{case}");
    assert!(
      store.operation("substituted_action").await?.is_none(),
      "{case}"
    );
    assert_eq!(store.events(&run.id, 0, 100).await?, before, "{case}");
    assert_eq!(store.pending_outbox(100).await?, outbox_before, "{case}");
  }
  Ok(())
}

#[tokio::test]
async fn duplicate_action_rolls_back_consumption_and_authorized_event() -> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let run = running(&store, "duplicate").await?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  store
    .begin_operation(&run.id, 1, "native_action", "computer_click", json!({}))
    .await?;
  let existing = store.operation("native_action").await?;
  let approval = approved(&store, &run.id, &arguments).await?;
  let before = store.events(&run.id, 0, 100).await?;
  assert!(
    store
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .await
      .is_err()
  );
  assert_eq!(
    store
      .approval(&approval.id)
      .await?
      .expect("approval")
      .status,
    "approved"
  );
  assert_eq!(store.operation("native_action").await?, existing);
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  assert_eq!(store.pending_outbox(100).await?, before);
  Ok(())
}

#[tokio::test]
async fn intent_outbox_failure_rolls_back_consumption_intent_and_both_events() -> anyhow::Result<()>
{
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "outbox").await?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let approval = approved(&store, &run.id, &arguments).await?;
  let before = store.events(&run.id, 0, 100).await?;
  let injected = store.connection();
  // Fail the second event's outbox write, after the authorization event succeeded.
  injected.execute_unprepared("CREATE TRIGGER reject_intent_outbox BEFORE INSERT ON outbox WHEN EXISTS (SELECT 1 FROM operations WHERE action_id = 'native_action') BEGIN SELECT RAISE(ABORT, 'injected storage fault'); END;").await?;
  assert!(
    store
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .await
      .is_err()
  );
  assert_eq!(
    store
      .approval(&approval.id)
      .await?
      .expect("approval")
      .status,
    "approved"
  );
  assert!(store.operation("native_action").await?.is_none());
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  assert_eq!(store.pending_outbox(100).await?, before);
  injected
    .execute_unprepared("DROP TRIGGER reject_intent_outbox;")
    .await?;
  store
    .begin_approved_operation(input(&run.id, &approval.id, &arguments))
    .await?;
  assert_eq!(
    store
      .approval(&approval.id)
      .await?
      .expect("approval")
      .status,
    "consumed"
  );
  assert_eq!(
    store.pending_outbox(100).await?,
    store.events(&run.id, 0, 100).await?
  );
  Ok(())
}

#[tokio::test]
async fn concurrent_writers_can_consume_the_bound_action_approval_only_once() -> anyhow::Result<()>
{
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "race").await?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let approval = approved(&store, &run.id, &arguments).await?;
  let left = Store::open(&path).await?;
  let right = Store::open(&path).await?;
  let before = store.events(&run.id, 0, 100).await?;
  let (first, second) = tokio::join!(
    async {
      left
        .begin_approved_operation(input(&run.id, &approval.id, &arguments))
        .await
    },
    async {
      right
        .begin_approved_operation(input(&run.id, &approval.id, &arguments))
        .await
    }
  );
  let results = [first, second];
  assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
  let winner = results.into_iter().find_map(Result::ok).expect("winner");
  assert_eq!(winner.approval_id(), approval.id);
  assert_eq!(
    store
      .operation(winner.action_id())
      .await?
      .expect("intent")
      .status,
    OperationStatus::Intent
  );
  assert_eq!(winner.action_id(), "native_action");
  assert_eq!(
    store
      .approval(&approval.id)
      .await?
      .expect("approval")
      .status,
    "consumed"
  );
  let events = store.events(&run.id, 0, 100).await?;
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
  assert_eq!(store.pending_outbox(100).await?, events);
  Ok(())
}

#[tokio::test]
async fn expired_approved_record_cannot_authorize_an_intent() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "expired").await?;
  let arguments = json!({"action_id":"native_action","x":12,"y":34});
  let mut approval = approved(&store, &run.id, &arguments).await?;
  approval.expires_at_ms = 0;
  let connection = store.connection();
  approvals::Entity::update_many()
    .filter(approvals::Column::Id.eq(&approval.id))
    .col_expr(
      approvals::Column::Record,
      Expr::value((serde_json::to_string(&approval)?).to_owned()),
    )
    .exec(connection)
    .await
    .map(|result| result.rows_affected)?;
  let before = store.events(&run.id, 0, 100).await?;
  assert!(
    store
      .begin_approved_operation(input(&run.id, &approval.id, &arguments))
      .await
      .is_err()
  );
  assert_eq!(store.approval(&approval.id).await?, Some(approval));
  assert!(store.operation("native_action").await?.is_none());
  assert_eq!(store.events(&run.id, 0, 100).await?, before);
  assert_eq!(store.pending_outbox(100).await?, before);
  Ok(())
}
