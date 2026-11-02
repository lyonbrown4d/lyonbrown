use agent_storage::Store;

use serde_json::json;
#[tokio::test]
async fn browser_intents_fence_owners_recover_unknown_and_never_replay_ids() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let session = store.create_session("browser", "/fixture", "node").await?;
  store
    .begin_browser_action(&session.id, 1, "action", "click", json!({"ref":"button"}))
    .await?;
  assert!(
    store
      .begin_browser_action(&session.id, 1, "action", "click", json!({"ref":"other"}))
      .await
      .is_err()
  );
  assert_eq!(
    store.browser_action_status("action").await?.as_deref(),
    Some("intent")
  );
  drop(store);
  let store = Store::open(&path).await?;
  assert_eq!(store.recover_browser_actions().await?, 1);
  assert_eq!(
    store.browser_action_status("action").await?.as_deref(),
    Some("uncertain")
  );
  assert_eq!(store.recover_browser_actions().await?, 0);
  assert!(
    store
      .begin_browser_action(&session.id, 1, "action", "click", json!({}))
      .await
      .is_err()
  );
  store
    .begin_browser_action(&session.id, 1, "new-action", "click", json!({}))
    .await?;
  store.change_owner(&session.id, 1, "replacement").await?;
  assert!(
    store
      .finish_browser_action("new-action", 1, "completed", json!({}))
      .await
      .is_err()
  );
  assert_eq!(
    store.browser_action_status("new-action").await?.as_deref(),
    Some("intent")
  );
  assert!(
    store
      .begin_browser_action(&session.id, 1, "stale-owner", "click", json!({}))
      .await
      .is_err()
  );
  assert_eq!(
    store
      .events(&format!("browser:{}", session.id), 0, 100)
      .await?
      .len(),
    3
  );
  Ok(())
}
