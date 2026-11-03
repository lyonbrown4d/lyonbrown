use gateway_storage::GatewayStorage;
use sea_orm::{Database, EntityTrait};
use sea_orm_migration::seaql_migrations;

#[tokio::test]
async fn fresh_schema_ledger_is_idempotent() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).await.expect("fresh install");
  drop(store);
  let _store = GatewayStorage::open(&path).await.expect("reopen");
  let connection = Database::connect(format!("sqlite://{}", path.display()))
    .await
    .expect("connection");
  let rows = seaql_migrations::Entity::find()
    .all(&connection)
    .await
    .expect("ledger");
  assert_eq!(rows.len(), 1);
}
