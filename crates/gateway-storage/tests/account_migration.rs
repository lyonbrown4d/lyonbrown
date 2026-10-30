use diesel::{connection::SimpleConnection, prelude::*};
use gateway_core::{AccessControl, AuditStatus};
use gateway_storage::{GatewayStorage, StorageError};
use sha2::{Digest, Sha256};

mod legacy {
  diesel::table! { gateway_migrations (version) { version -> Integer, } }
  diesel::table! { gateway_admin_credentials (id) { id -> Integer, token_hash -> Text, } }
  diesel::table! { gateway_admin_sessions (token_hash) { token_hash -> Text, csrf_hash -> Text, expires_at -> BigInt, } }
  diesel::table! { gateway_audits (sequence) { sequence -> BigInt, request_id -> Text, model -> Text, status -> SmallInt, elapsed_ms -> BigInt, policy_revision -> Text, } }
  diesel::table! { gateway_policy_revisions (revision) { revision -> Text, document -> Text, } }
  diesel::table! { gateway_policy_head (id) { id -> Integer, revision -> Text, } }
  diesel::table! { gateway_access_tokens (id) { id -> Text, label -> Text, token_hash -> Text, created_at -> BigInt, expires_at -> BigInt, revoked -> Bool, } }
}
const LEGACY_ADMIN: &str = "legacy-admin-token-must-never-authenticate";
const LEGACY_SESSION: &str = "legacy-session-token-must-be-revoked";
const API: &str = "fixture-existing-api-key-must-survive";

#[tokio::test]
async fn v1_migration_preserves_business_data_and_revokes_token_admin_credentials() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  {
    let mut connection =
      SqliteConnection::establish(path.to_str().expect("path")).expect("connection");
    connection
      .batch_execute(include_str!("../migrations/0001_initial/up.sql"))
      .expect("v1 migration");
    use legacy::*;
    diesel::insert_into(gateway_migrations::table)
      .values(gateway_migrations::version.eq(1))
      .execute(&mut connection)
      .expect("version");
    diesel::insert_into(gateway_admin_credentials::table)
      .values((
        gateway_admin_credentials::id.eq(1),
        gateway_admin_credentials::token_hash.eq(digest("admin", LEGACY_ADMIN)),
      ))
      .execute(&mut connection)
      .expect("old admin");
    diesel::insert_into(gateway_admin_sessions::table)
      .values((
        gateway_admin_sessions::token_hash.eq(digest("session", LEGACY_SESSION)),
        gateway_admin_sessions::csrf_hash.eq(digest("csrf", LEGACY_SESSION)),
        gateway_admin_sessions::expires_at.eq(1000_i64),
      ))
      .execute(&mut connection)
      .expect("old session");
    diesel::insert_into(gateway_audits::table)
      .values((
        gateway_audits::request_id.eq("pending-v1"),
        gateway_audits::model.eq("local"),
        gateway_audits::status.eq(0_i16),
        gateway_audits::elapsed_ms.eq(0_i64),
        gateway_audits::policy_revision.eq("rules-v1"),
      ))
      .execute(&mut connection)
      .expect("intent");
    let policy = decision_core::Policy::default();
    diesel::insert_into(gateway_policy_revisions::table)
      .values((
        gateway_policy_revisions::revision.eq(&policy.revision),
        gateway_policy_revisions::document.eq(serde_json::to_string(&policy).expect("policy")),
      ))
      .execute(&mut connection)
      .expect("policy");
    diesel::insert_into(gateway_policy_head::table)
      .values((
        gateway_policy_head::id.eq(1),
        gateway_policy_head::revision.eq(&policy.revision),
      ))
      .execute(&mut connection)
      .expect("policy head");
    diesel::insert_into(gateway_access_tokens::table)
      .values((
        gateway_access_tokens::id.eq("existing"),
        gateway_access_tokens::label.eq("existing key"),
        gateway_access_tokens::token_hash.eq(digest("access", API)),
        gateway_access_tokens::created_at.eq(1_i64),
        gateway_access_tokens::expires_at.eq(i64::MAX),
        gateway_access_tokens::revoked.eq(false),
      ))
      .execute(&mut connection)
      .expect("API credential");
  }
  let store = GatewayStorage::open(&path).expect("v2 open");
  assert!(
    !store
      .authenticate_session(LEGACY_SESSION, 100)
      .await
      .expect("old session invalid")
  );
  assert!(
    store
      .session_identity(LEGACY_SESSION, 100)
      .await
      .expect("identity")
      .is_none()
  );
  assert!(
    AccessControl::authorize(&store, API)
      .await
      .expect("API key survives")
  );
  assert_eq!(
    store.load_policy().await.expect("policy"),
    Some(decision_core::Policy::default())
  );
  let audits = store.list_audits(20, 0).await.expect("audits");
  assert_eq!(audits.total, 1);
  assert!(matches!(audits.records[0].status, AuditStatus::Uncertain));
  store
    .bootstrap_super_admin("operator", "new-fixture-password-v2")
    .await
    .expect("new account");
  assert!(matches!(
    store.login("operator", LEGACY_ADMIN, 100, 10).await,
    Err(StorageError::Unauthorized)
  ));
  let session = store
    .login("operator", "new-fixture-password-v2", 100, 10)
    .await
    .expect("password login");
  drop(store);
  let reopened =
    GatewayStorage::open(&path).expect("v2 reopens without reapplying destructive migration");
  assert!(
    AccessControl::authorize(&reopened, API)
      .await
      .expect("API survives reopen")
  );
  reopened
    .login("operator", "new-fixture-password-v2", 100, 10)
    .await
    .expect("account survives reopen");
  drop(reopened);

  // Reconstruct the original v2 schema and ledger while preserving its business data.
  // Importing this deployed state must seed Diesel rather than replay destructive v2 SQL.
  {
    let mut connection =
      SqliteConnection::establish(path.to_str().expect("path")).expect("connection");
    connection
      .batch_execute(include_str!("../migrations/0003_scoped_budgets/down.sql"))
      .expect("remove later schema additions from v2 fixture");
    connection
      .batch_execute(
        "CREATE TABLE gateway_migrations (version INTEGER PRIMARY KEY NOT NULL);
         DROP TABLE __diesel_schema_migrations;",
      )
      .expect("legacy v2 ledger fixture");
    diesel::insert_into(legacy::gateway_migrations::table)
      .values([
        legacy::gateway_migrations::version.eq(1),
        legacy::gateway_migrations::version.eq(2),
      ])
      .execute(&mut connection)
      .expect("legacy v2 versions");
  }
  let imported = GatewayStorage::open(&path).expect("legacy v2 imported");
  assert!(
    imported
      .authenticate_session(session.token(), 100)
      .await
      .expect("v2 session survives import")
  );
  assert!(
    imported
      .verify_csrf(session.token(), session.csrf_token(), 100)
      .await
      .expect("v2 CSRF credential survives import")
  );
  assert!(
    AccessControl::authorize(&imported, API)
      .await
      .expect("API survives legacy v2 import")
  );
  assert_eq!(
    imported.load_policy().await.expect("policy preserved"),
    Some(decision_core::Policy::default())
  );
  assert_eq!(imported.list_audits(20, 0).await.expect("audits").total, 1);
  imported
    .login("operator", "new-fixture-password-v2", 100, 10)
    .await
    .expect("account and password hash survive legacy v2 import");
  drop(imported);
  let reopened = GatewayStorage::open(&path).expect("imported v2 reopens");
  assert!(
    reopened
      .authenticate_session(session.token(), 100)
      .await
      .expect("session survives imported v2 reopen")
  );
}
fn digest(purpose: &str, token: &str) -> String {
  let mut hash = Sha256::new();
  hash.update(b"lyonbrown-gateway-v1\0");
  hash.update(purpose.as_bytes());
  hash.update(b"\0");
  hash.update(token.as_bytes());
  hex::encode(hash.finalize())
}
