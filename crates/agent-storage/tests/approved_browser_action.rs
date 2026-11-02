mod common;
use common::{approvals, browser_actions};
use sea_orm::{QuerySelect, prelude::*};

use agent_core::{Approval, Event, Run, RunStatus};
use agent_storage::{ApprovedBrowserAction, Store};
use anyhow::Result;
use serde_json::{Value, json};

const ACTION: &str = "browser_action";

async fn running(store: &Store, key: &str) -> Result<Run> {
  let session = store.create_session(key, "/fixture", "local").await?;
  let (run, _) = store
    .accept_run(&session.id, 1, "browser operation", key)
    .await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await
}

fn exact(run: &Run) -> Value {
  json!({"session_id":run.session_id,"owner_epoch":1,"host_id":"fixture-host",
    "action_id":ACTION,"call":{"op":"click","tab_id":"fixture-tab"}})
}

async fn approved(store: &Store, run: &Run, arguments: &Value) -> Result<Approval> {
  let approval = store
    .request_approval_owned(&run.id, 1, "browser_act", arguments)
    .await?;
  store.decide_approval(&approval.id, true).await
}

async fn assert_consumed(store: &Store, id: &str) -> Result<()> {
  let approval = store.approval(id).await?.expect("approval");
  assert_eq!(approval.status, "consumed");
  Ok(())
}

fn input<'a>(run: &'a Run, approval: &'a str, arguments: &'a Value) -> ApprovedBrowserAction<'a> {
  ApprovedBrowserAction {
    run_id: &run.id,
    owner_epoch: 1,
    action_id: ACTION,
    tool: "browser_act",
    arguments,
    summary: json!({"kind":"act","tab_id":"fixture-tab"}),
    approval_id: approval,
  }
}

async fn streams(store: &Store, run: &Run) -> Result<Vec<Event>> {
  let mut events = store.events(&run.id, 0, 100).await?;
  events.extend(
    store
      .events(&format!("browser:{}", run.session_id), 0, 100)
      .await?,
  );
  events.sort_by(|left, right| (&left.run_id, left.cursor).cmp(&(&right.run_id, right.cursor)));
  Ok(events)
}

fn unique_event<'a>(events: &'a [Event], kind: &str) -> &'a Event {
  let matches: Vec<_> = events.iter().filter(|event| event.kind == kind).collect();
  assert_eq!(matches.len(), 1, "{kind}");
  matches[0]
}

#[derive(Debug, PartialEq)]
struct Checkpoint {
  approval: Option<Approval>,
  action: Option<String>,
  events: Vec<Event>,
  outbox: Vec<Event>,
}

async fn checkpoint(store: &Store, run: &Run, approval: &str) -> Result<Checkpoint> {
  Ok(Checkpoint {
    approval: store.approval(approval).await?,
    action: store.browser_action_status(ACTION).await?,
    events: streams(store, run).await?,
    outbox: store.pending_outbox(100).await?,
  })
}

#[tokio::test]
async fn proof_binds_committed_approval_browser_intent_and_both_outboxes() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "success").await?;
  let arguments = exact(&run);
  let approval = approved(&store, &run, &arguments).await?;
  let proof = store
    .begin_approved_browser_action(input(&run, &approval.id, &arguments))
    .await?;
  assert_eq!(proof.session_id(), run.session_id);
  assert_eq!(proof.run_id(), run.id);
  assert_eq!(proof.owner_epoch(), 1);
  assert_eq!(proof.host_id(), "fixture-host");
  let reopened = Store::open(&path).await?;
  assert_consumed(&reopened, proof.approval_id()).await?;
  let connection = store.connection();
  let (session, summary): (String, String) = browser_actions::Entity::find()
    .filter(browser_actions::Column::ActionId.eq(proof.action_id()))
    .select_only()
    .column(browser_actions::Column::SessionId)
    .column(browser_actions::Column::Arguments)
    .into_tuple::<(String, String)>()
    .one(connection)
    .await?
    .ok_or_else(|| anyhow::anyhow!("browser_actions record missing"))?;
  assert_eq!(session, proof.session_id());
  assert_eq!(
    reopened.browser_action_status(ACTION).await?.as_deref(),
    Some("intent")
  );
  assert_eq!(
    serde_json::from_str::<Value>(&summary)?,
    json!({"kind":"act","tab_id":"fixture-tab"})
  );
  let events = streams(&reopened, &run).await?;
  let authorized = unique_event(&events, "tool_authorized");
  assert_eq!(authorized.run_id, run.id);
  assert_eq!(
    serde_json::from_str::<Value>(&authorized.payload_json)?,
    json!({"approval_id":approval.id,"tool":"browser_act"})
  );
  let intent = unique_event(&events, "browser_action_intent");
  assert_eq!(intent.run_id, format!("browser:{}", run.session_id));
  assert_eq!(
    serde_json::from_str::<Value>(&intent.payload_json)?,
    json!({"action_id":ACTION,"kind":"browser_command"})
  );
  assert_eq!(reopened.pending_outbox(100).await?, events);
  Ok(())
}

#[tokio::test]
async fn binding_and_lifecycle_rejections_preserve_approval_intent_and_both_streams() -> Result<()>
{
  for case in [
    "arguments",
    "run",
    "tool",
    "session",
    "epoch_binding",
    "stale_epoch",
    "pending",
    "denied",
    "cancel_requested",
    "completed",
    "expired",
    "action",
    "summary",
  ] {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state.sqlite");
    let store = Store::open(&path).await?;
    let run = running(&store, case).await?;
    let other = running(&store, "other").await?;
    let mut arguments = exact(&run);
    if matches!(case, "session" | "run") {
      arguments["session_id"] = json!(other.session_id);
    }
    if case == "epoch_binding" {
      arguments["owner_epoch"] = json!(2);
    }
    let tool = if case == "tool" {
      "computer_click"
    } else {
      "browser_act"
    };
    let mut approval = store
      .request_approval_owned(&run.id, 1, tool, &arguments)
      .await?;
    if case != "pending" {
      approval = store
        .decide_approval(&approval.id, case != "denied")
        .await?;
    }
    match case {
      "stale_epoch" => {
        store
          .change_owner(&run.session_id, 1, "replacement")
          .await?;
      }
      "cancel_requested" => {
        store
          .update_run_owned(&run.id, 1, RunStatus::CancelRequested, "", "")
          .await?;
      }
      "completed" => {
        store
          .update_run_owned(&run.id, 1, RunStatus::Completed, "", "")
          .await?;
      }
      "expired" => {
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
      }
      _ => {}
    }
    let before = checkpoint(&store, &run, &approval.id).await?;
    let mut changed = arguments.clone();
    changed["call"]["op"] = json!("navigate");
    let mut request = input(&run, &approval.id, &arguments);
    match case {
      "arguments" => request.arguments = &changed,
      "run" => request.run_id = &other.id,
      "tool" => request.tool = tool,
      "action" => request.action_id = "substituted_action",
      "summary" => request.summary = json!({"oversized":"x".repeat(1024 * 1024)}),
      _ => {}
    }
    assert!(
      store.begin_approved_browser_action(request).await.is_err(),
      "{case}"
    );
    assert_eq!(
      checkpoint(&store, &run, &approval.id).await?,
      before,
      "{case}"
    );
    assert_eq!(
      store.browser_action_status("substituted_action").await?,
      None,
      "{case}"
    );
  }
  Ok(())
}

#[tokio::test]
async fn duplicate_action_rolls_back_approval_consumption_and_authorization_event() -> Result<()> {
  let store = Store::open(":memory:").await?;
  let run = running(&store, "duplicate").await?;
  let arguments = exact(&run);
  store
    .begin_browser_action(&run.session_id, 1, ACTION, "browser_command", json!({}))
    .await?;
  store
    .finish_browser_action(ACTION, 1, "failed", json!({"status":"failed"}))
    .await?;
  let approval = approved(&store, &run, &arguments).await?;
  let before = checkpoint(&store, &run, &approval.id).await?;
  let request = input(&run, &approval.id, &arguments);
  assert!(store.begin_approved_browser_action(request).await.is_err());
  assert_eq!(checkpoint(&store, &run, &approval.id).await?, before);
  Ok(())
}

#[tokio::test]
async fn browser_intent_outbox_failure_rolls_back_approval_intent_and_both_events() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "outbox").await?;
  let arguments = exact(&run);
  let approval = approved(&store, &run, &arguments).await?;
  let before = checkpoint(&store, &run, &approval.id).await?;
  let injected = store.connection();
  // Fail the browser intent Outbox write after the run authorization event succeeded.
  injected.execute_unprepared("CREATE TRIGGER reject_browser_outbox BEFORE INSERT ON outbox WHEN EXISTS (SELECT 1 FROM browser_actions WHERE action_id = 'browser_action') BEGIN SELECT RAISE(ABORT, 'injected storage fault'); END;").await?;
  let request = input(&run, &approval.id, &arguments);
  assert!(store.begin_approved_browser_action(request).await.is_err());
  assert_eq!(checkpoint(&store, &run, &approval.id).await?, before);
  injected
    .execute_unprepared("DROP TRIGGER reject_browser_outbox;")
    .await?;
  store
    .begin_approved_browser_action(input(&run, &approval.id, &arguments))
    .await?;
  assert_consumed(&store, &approval.id).await?;
  assert_eq!(
    store.browser_action_status(ACTION).await?.as_deref(),
    Some("intent")
  );
  assert_eq!(
    store.pending_outbox(100).await?,
    streams(&store, &run).await?
  );
  Ok(())
}

#[tokio::test]
async fn concurrent_writers_have_one_winner_and_restart_recovers_without_replay() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let run = running(&store, "race").await?;
  let arguments = exact(&run);
  let approval = approved(&store, &run, &arguments).await?;
  let left = Store::open(&path).await?;
  let right = Store::open(&path).await?;
  let (first, second) = tokio::join!(
    async {
      left
        .begin_approved_browser_action(input(&run, &approval.id, &arguments))
        .await
    },
    async {
      right
        .begin_approved_browser_action(input(&run, &approval.id, &arguments))
        .await
    }
  );
  let results = [first, second];
  assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
  let reopened = Store::open(&path).await?;
  assert_consumed(&reopened, &approval.id).await?;
  let events = streams(&reopened, &run).await?;
  unique_event(&events, "tool_authorized");
  unique_event(&events, "browser_action_intent");
  assert_eq!(reopened.pending_outbox(100).await?, events);
  assert_eq!(reopened.recover_browser_actions().await?, 1);
  assert_eq!(
    reopened.browser_action_status(ACTION).await?.as_deref(),
    Some("uncertain")
  );
  assert_eq!(reopened.recover_browser_actions().await?, 0);
  // A fresh approval still cannot replay a recovered browser action identifier.
  let retry = approved(&reopened, &run, &arguments).await?;
  let before = checkpoint(&reopened, &run, &retry.id).await?;
  let request = input(&run, &retry.id, &arguments);
  assert!(
    reopened
      .begin_approved_browser_action(request)
      .await
      .is_err()
  );
  assert_eq!(checkpoint(&reopened, &run, &retry.id).await?, before);
  Ok(())
}
