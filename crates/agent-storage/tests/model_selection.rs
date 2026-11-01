use agent_storage::{ModelBinding, Store};

#[test]
fn selection_is_fenced_persistent_and_snapshotted_with_run_acceptance() {
  let dir = tempfile::tempdir().expect("directory");
  let path = dir.path().join("state.db");
  let store = Store::open(&path).expect("store");
  let session = store
    .create_session("model", "/tmp", "node")
    .expect("session");
  let binding = ModelBinding {
    model_id: "fixture".into(),
    execution_id: "configured".into(),
    provider: "openai-completions".into(),
    model: "fixture".into(),
    endpoint: Some("http://127.0.0.1:8000/v1".into()),
    credential_id: "openai".into(),
  };
  assert!(store.select_model(&session.id, 2, &binding).is_err());
  assert!(store.model_binding(&session.id).expect("binding").is_none());
  store
    .select_model(&session.id, 1, &binding)
    .expect("select");
  let (run, _) = store
    .accept_run(&session.id, 1, "hello", "once")
    .expect("accept");
  assert_eq!(
    store
      .model_binding(&run.id)
      .expect("run binding")
      .expect("snapshot")
      .model,
    "fixture"
  );
  assert!(store.select_model(&session.id, 1, &binding).is_err());
  drop(store);
  let store = Store::open(&path).expect("reopen");
  assert_eq!(
    store
      .model_binding(&session.id)
      .expect("binding")
      .expect("selection")
      .model_id,
    "fixture"
  );
  assert!(
    store
      .events(&session.id, 0, 100)
      .expect("events")
      .iter()
      .any(|event| event.kind == "model_selected")
  );
}
