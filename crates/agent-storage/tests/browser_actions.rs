use agent_storage::Store;
use serde_json::json;
#[test]
fn browser_intents_fence_owners_recover_unknown_and_never_replay_ids() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("browser", "/fixture", "node")?;
  store.begin_browser_action(&session.id, 1, "action", "click", json!({"ref":"button"}))?;
  assert!(
    store
      .begin_browser_action(&session.id, 1, "action", "click", json!({"ref":"other"}))
      .is_err()
  );
  assert_eq!(
    store.browser_action_status("action")?.as_deref(),
    Some("intent")
  );
  drop(store);
  let store = Store::open(&path)?;
  assert_eq!(store.recover_browser_actions()?, 1);
  assert_eq!(
    store.browser_action_status("action")?.as_deref(),
    Some("uncertain")
  );
  assert_eq!(store.recover_browser_actions()?, 0);
  assert!(
    store
      .begin_browser_action(&session.id, 1, "action", "click", json!({}))
      .is_err()
  );
  store.begin_browser_action(&session.id, 1, "new-action", "click", json!({}))?;
  store.change_owner(&session.id, 1, "replacement")?;
  assert!(
    store
      .finish_browser_action("new-action", 1, "completed", json!({}))
      .is_err()
  );
  assert_eq!(
    store.browser_action_status("new-action")?.as_deref(),
    Some("intent")
  );
  assert!(
    store
      .begin_browser_action(&session.id, 1, "stale-owner", "click", json!({}))
      .is_err()
  );
  assert_eq!(
    store
      .events(&format!("browser:{}", session.id), 0, 100)?
      .len(),
    3
  );
  Ok(())
}
