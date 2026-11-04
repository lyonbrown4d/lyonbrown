use agent_core::{RunStatus, interaction::InteractionMode};
use agent_storage::Store;
#[tokio::test]
async fn mode_is_fenced_and_bound_to_each_accepted_run() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("interaction.sqlite");
  let store = Store::open(&path).await?;
  let session = store.create_session("mode", "/tmp", "local").await?;
  let mode = store
    .set_interaction_mode(&session.id, session.owner_epoch, 0, InteractionMode::Plan)
    .await?;
  assert_eq!(mode.revision, 1);
  assert!(
    store
      .set_interaction_mode(
        &session.id,
        session.owner_epoch + 1,
        1,
        InteractionMode::Execute
      )
      .await
      .is_err()
  );
  assert!(
    store
      .set_interaction_mode(
        &session.id,
        session.owner_epoch,
        0,
        InteractionMode::Execute
      )
      .await
      .is_err()
  );
  let (run, _) = store
    .accept_run(&session.id, session.owner_epoch, "plan", "first")
    .await?;
  assert_eq!(store.interaction_policy(&run.id).await?, mode);
  assert!(
    store
      .set_interaction_mode(
        &session.id,
        session.owner_epoch,
        1,
        InteractionMode::Execute
      )
      .await
      .is_err()
  );
  store
    .update_run_owned(
      &run.id,
      session.owner_epoch,
      RunStatus::Completed,
      "done",
      "",
    )
    .await?;
  store
    .set_interaction_mode(
      &session.id,
      session.owner_epoch,
      1,
      InteractionMode::ReadOnly,
    )
    .await?;
  assert_eq!(
    store.interaction_policy(&run.id).await?.mode,
    InteractionMode::Plan
  );
  drop(store);
  let reopened = Store::open(&path).await?;
  assert_eq!(
    reopened.interaction_policy(&session.id).await?.mode,
    InteractionMode::ReadOnly
  );
  assert_eq!(
    reopened.interaction_policy(&run.id).await?.mode,
    InteractionMode::Plan
  );
  let (next, _) = reopened
    .accept_run(&session.id, session.owner_epoch, "read", "second")
    .await?;
  assert_eq!(
    reopened.interaction_policy(&next.id).await?.mode,
    InteractionMode::ReadOnly
  );
  let events = reopened.events(&session.id, 0, 100).await?;
  assert_eq!(
    events
      .iter()
      .filter(|event| event.kind == "interaction_mode_changed")
      .count(),
    2
  );
  let outbox = reopened.pending_outbox(100).await?;
  assert!(events.iter().all(|event| outbox.contains(event)));
  Ok(())
}
#[test]
fn modes_never_enable_effectful_tools_or_unknown_tools() {
  for mode in [InteractionMode::Plan, InteractionMode::ReadOnly] {
    for tool in [
      "write_file",
      "patch_apply",
      "checkpoint_undo",
      "execute_command",
      "browser_act",
      "computer_act",
      "mcp_call",
      "delegate_task",
      "memory_propose",
      "unknown",
    ] {
      assert!(!mode.permits(tool), "{mode:?} enabled {tool}");
    }
    assert!(mode.permits("read_file"));
  }
  assert!(InteractionMode::Plan.permits("plan_update"));
  assert!(!InteractionMode::ReadOnly.permits("plan_update"));
}
