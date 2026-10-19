use argon2::{Argon2, PasswordHash, PasswordVerifier};
use diesel::prelude::*;
use gateway_storage::{AdminRole, GatewayStorage, StorageError};

const PASSWORD: &str = "fixture-super-admin-password-v2";
diesel::table! { gateway_admin_accounts (id) { id -> Integer, username -> Text, password_hash -> Text, role -> SmallInt, credential_revision -> Text, } }

#[tokio::test]
async fn bootstrap_is_immutable_and_salted_password_hashes_survive_reopen() {
  let first = tempfile::tempdir().expect("directory");
  let path = first.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).expect("open");
  let (one, two) = tokio::join!(
    store.bootstrap_super_admin("operator", PASSWORD),
    store.bootstrap_super_admin("operator", PASSWORD)
  );
  one.expect("idempotent concurrent bootstrap");
  two.expect("idempotent concurrent bootstrap");
  assert!(matches!(
    store.bootstrap_super_admin("different", PASSWORD).await,
    Err(StorageError::Conflict)
  ));
  assert!(matches!(
    store
      .bootstrap_super_admin("operator", "another-valid-password")
      .await,
    Err(StorageError::Conflict)
  ));
  drop(store);
  let hash = read_hash(&path);
  assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
  assert!(
    Argon2::default()
      .verify_password(PASSWORD.as_bytes(), &PasswordHash::new(&hash).expect("PHC"))
      .is_ok()
  );
  let reopened = GatewayStorage::open(&path).expect("reopen");
  let session = reopened
    .login("operator", PASSWORD, 100, 10)
    .await
    .expect("login");
  assert_eq!(session.identity.role, AdminRole::SuperAdmin);
  drop(reopened);
  let second = tempfile::tempdir().expect("directory");
  let second_path = second.path().join("gateway.sqlite3");
  let separate = GatewayStorage::open(&second_path).expect("open");
  separate
    .bootstrap_super_admin("operator", PASSWORD)
    .await
    .expect("independent bootstrap");
  drop(separate);
  assert_ne!(hash, read_hash(&second_path));
}

#[tokio::test]
async fn credential_revision_revokes_sessions_and_utf8_password_length_is_measured_in_bytes() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).expect("open");
  for username in ["", "invalid name", "用户", &"x".repeat(65)] {
    assert!(
      store
        .bootstrap_super_admin(username, PASSWORD)
        .await
        .is_err()
    );
  }
  assert!(
    store
      .bootstrap_super_admin("operator", "short")
      .await
      .is_err()
  );
  assert!(
    store
      .bootstrap_super_admin("operator", &"x".repeat(129))
      .await
      .is_err()
  );
  store
    .bootstrap_super_admin("operator", "五个汉字呀")
    .await
    .expect("15 UTF-8 bytes");
  let old = store
    .login("operator", "五个汉字呀", 100, 10)
    .await
    .expect("login");
  let mut connection =
    SqliteConnection::establish(path.to_str().expect("path")).expect("connection");
  diesel::update(gateway_admin_accounts::table.find(1))
    .set(gateway_admin_accounts::credential_revision.eq("replacement-revision"))
    .execute(&mut connection)
    .expect("credential revision");
  assert!(
    !store
      .authenticate_session(old.token(), 101)
      .await
      .expect("revoked identity")
  );
  assert!(
    !store
      .verify_csrf(old.token(), old.csrf_token(), 101)
      .await
      .expect("revoked CSRF")
  );
  let new = store
    .login("operator", "五个汉字呀", 101, 10)
    .await
    .expect("current credential");
  assert!(
    store
      .verify_csrf(new.token(), new.csrf_token(), 102)
      .await
      .expect("current CSRF")
  );
}

fn read_hash(path: &std::path::Path) -> String {
  let mut connection =
    SqliteConnection::establish(path.to_str().expect("path")).expect("connection");
  assert_eq!(
    gateway_admin_accounts::table
      .count()
      .get_result::<i64>(&mut connection)
      .expect("account count"),
    1
  );
  gateway_admin_accounts::table
    .find(1)
    .select(gateway_admin_accounts::password_hash)
    .first(&mut connection)
    .expect("hash")
}
