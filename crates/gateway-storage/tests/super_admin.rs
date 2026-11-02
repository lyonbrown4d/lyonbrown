use argon2::{Argon2, PasswordHash, PasswordVerifier};
use gateway_storage::{AdminRole, GatewayStorage, StorageError};
use sea_orm::{ActiveModelTrait, Database, EntityTrait, Set};

const PASSWORD: &str = "fixture-super-admin-password-v2";
mod gateway_admin_accounts {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
  #[sea_orm(table_name = "gateway_admin_accounts")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub username: String,
    pub password_hash: String,
    pub role: i16,
    pub credential_revision: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}

#[tokio::test]
async fn bootstrap_is_immutable_and_salted_password_hashes_survive_reopen() {
  let first = tempfile::tempdir().expect("directory");
  let path = first.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).await.expect("open");
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
  let hash = read_hash(&path).await;
  assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
  assert!(
    Argon2::default()
      .verify_password(PASSWORD.as_bytes(), &PasswordHash::new(&hash).expect("PHC"))
      .is_ok()
  );
  let reopened = GatewayStorage::open(&path).await.expect("reopen");
  let session = reopened
    .login("operator", PASSWORD, 100, 10)
    .await
    .expect("login");
  assert_eq!(session.identity.role, AdminRole::SuperAdmin);
  drop(reopened);
  let second = tempfile::tempdir().expect("directory");
  let second_path = second.path().join("gateway.sqlite3");
  let separate = GatewayStorage::open(&second_path).await.expect("open");
  separate
    .bootstrap_super_admin("operator", PASSWORD)
    .await
    .expect("independent bootstrap");
  drop(separate);
  assert_ne!(hash, read_hash(&second_path).await);
}

#[tokio::test]
async fn credential_revision_revokes_sessions_and_utf8_password_length_is_measured_in_bytes() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).await.expect("open");
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
  let connection = Database::connect(format!("sqlite://{}", path.display()))
    .await
    .expect("connection");
  let row = gateway_admin_accounts::Entity::find_by_id(1)
    .one(&connection)
    .await
    .expect("account")
    .expect("row");
  let mut row: gateway_admin_accounts::ActiveModel = row.into();
  row.credential_revision = Set("replacement-revision".into());
  row.update(&connection).await.expect("credential revision");
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

async fn read_hash(path: &std::path::Path) -> String {
  let connection = Database::connect(format!("sqlite://{}", path.display()))
    .await
    .expect("connection");
  let rows = gateway_admin_accounts::Entity::find()
    .all(&connection)
    .await
    .expect("accounts");
  assert_eq!(rows.len(), 1);
  rows[0].password_hash.clone()
}
