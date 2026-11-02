use gateway_storage::GatewayStorage;
use sea_orm::{Database, EntityTrait};
use sea_orm_migration::seaql_migrations;

#[tokio::test]
async fn fresh_schema_ledger_is_idempotent_and_reopen_preserves_credentials() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).await.expect("fresh install");
  store
    .bootstrap_super_admin("operator", "schema-fixture-password")
    .await
    .expect("account");
  let session = store
    .login("operator", "schema-fixture-password", 100, 100)
    .await
    .expect("session");
  drop(store);
  let store = GatewayStorage::open(&path).await.expect("reopen");
  assert!(
    store
      .verify_csrf(session.token(), session.csrf_token(), 101)
      .await
      .expect("session persists")
  );
  let connection = Database::connect(format!("sqlite://{}", path.display()))
    .await
    .expect("connection");
  let rows = seaql_migrations::Entity::find()
    .all(&connection)
    .await
    .expect("ledger");
  assert_eq!(rows.len(), 1);
}
