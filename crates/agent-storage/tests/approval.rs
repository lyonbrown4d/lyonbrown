mod common;
use agent_core::RunStatus;
use agent_storage::Store;
use common::approvals;
use sea_orm::prelude::*;
use serde_json::json;

#[tokio::test]
async fn authorization_is_exact_expiring_and_consumed_once() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("state.sqlite");
  let store = Store::open(&path).await?;
  let session = store.create_session("approval", "/tmp", "local").await?;
  let (run, _) = store.accept_run(&session.id, 1, "write", "key").await?;
  store
    .update_run_owned(&run.id, 1, RunStatus::Running, "", "")
    .await?;
  let arguments = json!({"path":"approved.txt","contents":"safe"});
  let approved = store.request_approval(&run.id, "write", &arguments).await?;
  assert_eq!(
    store.run(&run.id).await?.expect("run").status,
    "awaiting_approval"
  );
  assert!(
    !store
      .consume_approval(&approved.id, &run.id, "write", &arguments)
      .await?
  );
  store.decide_approval(&approved.id, true).await?;
  assert!(
    store
      .consume_approval(
        &approved.id,
        &run.id,
        "write",
        &json!({"path":"other.txt","contents":"safe"})
      )
      .await
      .is_err()
  );
  assert!(
    store
      .consume_approval(&approved.id, &run.id, "shell", &arguments)
      .await
      .is_err()
  );
  assert!(
    store
      .consume_approval(&approved.id, &run.id, "write", &arguments)
      .await?
  );
  assert!(
    !store
      .consume_approval(&approved.id, &run.id, "write", &arguments)
      .await?
  );
  let mut expired = store.request_approval(&run.id, "write", &arguments).await?;
  expired.expires_at_ms = 0;
  let connection = store.connection();
  approvals::Entity::update_many()
    .filter(approvals::Column::Id.eq(&expired.id))
    .col_expr(
      approvals::Column::Record,
      Expr::value((serde_json::to_string(&expired)?).to_owned()),
    )
    .exec(connection)
    .await
    .map(|result| result.rows_affected)?;
  assert!(store.decide_approval(&expired.id, true).await.is_err());
  assert!(
    store
      .consume_approval(&expired.id, &run.id, "write", &arguments)
      .await
      .is_err()
  );
  assert_eq!(
    store.approval(&expired.id).await?.expect("approval").status,
    "pending"
  );
  Ok(())
}
