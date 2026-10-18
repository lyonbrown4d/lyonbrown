use agent_core::RunStatus;
use agent_storage::Store;
use diesel::prelude::*;
use serde_json::json;

diesel::table! {
  approvals (id) {
    id -> Text,
    run_id -> Text,
    status -> Text,
    record -> Text,
  }
}

#[test]
fn authorization_is_exact_expiring_and_consumed_once() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("approval", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "write", "key")?;
  store.update_run_owned(&run.id, 1, RunStatus::Running, "", "")?;
  let arguments = json!({"path":"approved.txt","contents":"safe"});
  let approved = store.request_approval(&run.id, "write", &arguments)?;
  assert_eq!(
    store.run(&run.id)?.expect("run").status,
    "awaiting_approval"
  );
  assert!(!store.consume_approval(&approved.id, &run.id, "write", &arguments)?);
  store.decide_approval(&approved.id, true)?;
  assert!(
    store
      .consume_approval(
        &approved.id,
        &run.id,
        "write",
        &json!({"path":"other.txt","contents":"safe"})
      )
      .is_err()
  );
  assert!(
    store
      .consume_approval(&approved.id, &run.id, "shell", &arguments)
      .is_err()
  );
  assert!(store.consume_approval(&approved.id, &run.id, "write", &arguments)?);
  assert!(!store.consume_approval(&approved.id, &run.id, "write", &arguments)?);
  let mut expired = store.request_approval(&run.id, "write", &arguments)?;
  expired.expires_at_ms = 0;
  let mut connection = SqliteConnection::establish(path.to_str().expect("path"))?;
  diesel::update(approvals::table.find(&expired.id))
    .set(approvals::record.eq(serde_json::to_string(&expired)?))
    .execute(&mut connection)?;
  assert!(store.decide_approval(&expired.id, true).is_err());
  assert!(
    store
      .consume_approval(&expired.id, &run.id, "write", &arguments)
      .is_err()
  );
  assert_eq!(
    store.approval(&expired.id)?.expect("approval").status,
    "pending"
  );
  Ok(())
}
