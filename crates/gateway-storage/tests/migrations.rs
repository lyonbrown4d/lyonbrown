use diesel::{connection::SimpleConnection, prelude::*};
use gateway_core::AuditStatus;
use gateway_storage::{GatewayStorage, StorageError};

mod migration_support;
use migration_support::*;

#[test]
fn fresh_database_uses_diesel_ledger_and_reopens_without_legacy_ledger() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  drop(GatewayStorage::open(&path).expect("fresh install"));
  let mut connection = connect(&path);
  let versions = __diesel_schema_migrations::table
    .select(__diesel_schema_migrations::version)
    .order(__diesel_schema_migrations::version.asc())
    .load::<String>(&mut connection)
    .expect("Diesel ledger");
  assert_eq!(versions, known_versions());
  assert_eq!(
    sqlite_schema::table
      .filter(sqlite_schema::name.eq("gateway_migrations"))
      .count()
      .get_result::<i64>(&mut connection)
      .expect("legacy ledger absent"),
    0
  );
  drop(connection);
  drop(GatewayStorage::open(&path).expect("reopen"));
}

#[test]
fn unknown_gapped_or_empty_legacy_ledgers_fail_without_mutating_schema_or_data() {
  for versions in [vec![], vec![2], vec![1, 3], vec![1, 2, 3]] {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("gateway.sqlite3");
    let mut connection = connect(&path);
    v1_fixture(&mut connection, &versions);
    let before = snapshot(&mut connection);
    drop(connection);
    assert!(matches!(
      GatewayStorage::open(&path),
      Err(StorageError::InvalidData)
    ));
    assert_eq!(snapshot(&mut connect(&path)), before, "{versions:?}");
  }
}

#[test]
fn unknown_gapped_or_noncanonical_diesel_ledgers_fail_without_mutation() {
  let known = known_versions();
  for versions in [
    vec![known[1].clone()],
    vec![known[0].clone(), "9999".into()],
    vec![known[0].clone(), known[1].clone(), "9999".into()],
    vec!["1".into()],
  ] {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("gateway.sqlite3");
    let mut connection = connect(&path);
    v1_fixture(&mut connection, &[1]);
    diesel_versions(&mut connection, &versions);
    connection
      .batch_execute("DROP TABLE gateway_migrations;")
      .expect("native Diesel ledger fixture");
    let before = snapshot(&mut connection);
    drop(connection);
    assert!(matches!(
      GatewayStorage::open(&path),
      Err(StorageError::InvalidData)
    ));
    assert_eq!(snapshot(&mut connect(&path)), before, "{versions:?}");
  }
}

#[test]
fn an_application_schema_without_a_ledger_fails_without_replaying_migrations() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let mut connection = connect(&path);
  v1_fixture(&mut connection, &[1]);
  connection
    .batch_execute("DROP TABLE gateway_migrations;")
    .expect("orphaned schema fixture");
  let before = snapshot(&mut connection);
  drop(connection);
  assert!(matches!(
    GatewayStorage::open(&path),
    Err(StorageError::InvalidData)
  ));
  assert_eq!(snapshot(&mut connect(&path)), before);
}

#[test]
fn conflicting_recognized_ledgers_fail_without_replaying_or_seeding_migrations() {
  let known = known_versions();
  for (legacy, diesel) in [(vec![1], known.clone()), (vec![1, 2], known[..1].to_vec())] {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("gateway.sqlite3");
    let mut connection = connect(&path);
    v1_fixture(&mut connection, &legacy);
    diesel_versions(&mut connection, &diesel);
    let before = snapshot(&mut connection);
    drop(connection);
    assert!(matches!(
      GatewayStorage::open(&path),
      Err(StorageError::InvalidData)
    ));
    assert_eq!(snapshot(&mut connect(&path)), before);
  }
}

#[tokio::test]
async fn matching_legacy_and_diesel_ledgers_finish_the_pending_migration() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let mut connection = connect(&path);
  v1_fixture(&mut connection, &[1]);
  diesel_versions(&mut connection, &known_versions()[..1]);
  drop(connection);
  let store = GatewayStorage::open(&path).expect("matching ledgers accepted");
  let audits = store.list_audits(10, 0).await.expect("audit");
  assert_eq!(audits.total, 1);
  assert!(matches!(audits.records[0].status, AuditStatus::Uncertain));
  drop(store);
  drop(GatewayStorage::open(&path).expect("native ledger reopens"));
}

#[tokio::test]
async fn reconciliation_failure_rolls_back_bridge_migrations_and_legacy_retirement() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let mut connection = connect(&path);
  v1_fixture(&mut connection, &[1]);
  connection
    .batch_execute(
      "CREATE TRIGGER reject_reconciliation BEFORE UPDATE ON gateway_audits
       BEGIN SELECT RAISE(ABORT, 'fixture reconciliation rejection'); END;",
    )
    .expect("failure fixture");
  let before = snapshot(&mut connection);
  drop(connection);
  assert!(matches!(
    GatewayStorage::open(&path),
    Err(StorageError::Database(_))
  ));
  let mut connection = connect(&path);
  assert_eq!(snapshot(&mut connection), before);
  connection
    .batch_execute("DROP TRIGGER reject_reconciliation;")
    .expect("remove failure fixture");
  drop(connection);
  let store = GatewayStorage::open(&path).expect("retry after rolled-back open");
  let audits = store.list_audits(10, 0).await.expect("audit");
  assert_eq!(audits.total, 1);
  assert!(matches!(audits.records[0].status, AuditStatus::Uncertain));
}
