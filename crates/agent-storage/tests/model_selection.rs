use agent_storage::{ModelBinding, Store};

#[tokio::test]
async fn selection_is_fenced_persistent_and_snapshotted_with_run_acceptance() {
  let dir = tempfile::tempdir().expect("directory");
  let path = dir.path().join("state.db");
  let store = Store::open(&path).await.expect("store");
  let session = store
    .create_session("model", "/tmp", "node")
    .await
    .expect("session");
  let binding = ModelBinding {
    model_id: "fixture".into(),
    execution_id: "configured".into(),
    provider: "openai-completions".into(),
    model: "fixture".into(),
    endpoint: Some("http://127.0.0.1:8000/v1".into()),
    credential_id: "openai".into(),
  };
  assert!(store.select_model(&session.id, 2, &binding).await.is_err());
  assert!(
    store
      .model_binding(&session.id)
      .await
      .expect("binding")
      .is_none()
  );
  store
    .select_model(&session.id, 1, &binding)
    .await
    .expect("select");
  let (run, _) = store
    .accept_run(&session.id, 1, "hello", "once")
    .await
    .expect("accept");
  assert_eq!(
    store
      .model_binding(&run.id)
      .await
      .expect("run binding")
      .expect("snapshot")
      .model,
    "fixture"
  );
  assert!(store.select_model(&session.id, 1, &binding).await.is_err());
  drop(store);
  let store = Store::open(&path).await.expect("reopen");
  assert_eq!(
    store
      .model_binding(&session.id)
      .await
      .expect("binding")
      .expect("selection")
      .model_id,
    "fixture"
  );
  assert!(
    store
      .events(&session.id, 0, 100)
      .await
      .expect("events")
      .iter()
      .any(|event| event.kind == "model_selected")
  );
}
