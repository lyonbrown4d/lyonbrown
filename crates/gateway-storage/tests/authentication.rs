use gateway_core::AccessControl;
use gateway_storage::{AdminRole, GatewayStorage, StorageError};

const PASSWORD: &str = "fixture-admin-7fD4k2dY8mR3xC9vA1qZ0";
const API: &str = "fixture-api-5mN9rB8cT4kY7pD1aS3fE6";

#[tokio::test]
async fn admin_authentication_session_expiry_and_csrf_are_bound() {
  let directory = tempfile::tempdir().expect("directory");
  let store = GatewayStorage::open(directory.path().join("gateway.sqlite3")).expect("open");
  store
    .bootstrap_super_admin("operator", PASSWORD)
    .await
    .expect("bootstrap");
  store
    .bootstrap_super_admin("operator", PASSWORD)
    .await
    .expect("idempotent bootstrap");
  assert!(
    store
      .bootstrap_super_admin("different", PASSWORD)
      .await
      .is_err()
  );
  assert!(store.login("operator", API, 100, 10).await.is_err());
  assert!(
    !store
      .authorize_access_token(PASSWORD, 100)
      .await
      .expect("no API scope")
  );
  let one = store
    .login("operator", PASSWORD, 100, 10)
    .await
    .expect("session");
  let two = store
    .login("operator", PASSWORD, 100, 10)
    .await
    .expect("second session");
  assert_ne!(one.token(), two.token());
  assert_eq!(one.identity.username, "operator");
  assert_eq!(one.identity.role, AdminRole::SuperAdmin);
  assert_eq!(
    store
      .session_identity(one.token(), 109)
      .await
      .expect("identity"),
    Some(one.identity.clone())
  );
  assert!(matches!(
    store.login("unknown", PASSWORD, 100, 10).await,
    Err(StorageError::Unauthorized)
  ));
  assert!(matches!(
    store
      .login("unknown", "gateway-dummy-not-an-account-v2", 100, 10)
      .await,
    Err(StorageError::Unauthorized)
  ));
  assert!(matches!(
    store.login("operator", "wrong-password-123", 100, 10).await,
    Err(StorageError::Unauthorized)
  ));
  assert!(
    store
      .authenticate_session(one.token(), 109)
      .await
      .expect("valid session")
  );
  assert!(
    !store
      .authenticate_session(one.token(), 110)
      .await
      .expect("expired session")
  );
  assert!(
    store
      .verify_csrf(one.token(), one.csrf_token(), 109)
      .await
      .expect("CSRF")
  );
  assert!(
    !store
      .verify_csrf(one.token(), two.csrf_token(), 109)
      .await
      .expect("wrong CSRF")
  );
  assert!(
    !store
      .verify_csrf(one.token(), one.csrf_token(), 110)
      .await
      .expect("expired CSRF")
  );
  assert!(!format!("{one:?}").contains(one.token()));
  assert!(!format!("{one:?}").contains(one.csrf_token()));
  store.revoke_session(two.token()).await.expect("revoke");
  assert!(
    !store
      .authenticate_session(two.token(), 101)
      .await
      .expect("revoked session")
  );
  assert!(
    !store
      .authenticate_session("malformed", 101)
      .await
      .expect("malformed")
  );
}

#[tokio::test]
async fn api_tokens_are_hash_only_revocable_expiring_and_separate_from_admin() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).expect("open");
  store
    .bootstrap_super_admin("operator", PASSWORD)
    .await
    .expect("admin bootstrap");
  store
    .bootstrap_access_token(API)
    .await
    .expect("API bootstrap");
  assert!(
    AccessControl::authorize(&store, API)
      .await
      .expect("API authorized")
  );
  assert!(
    !AccessControl::authorize(&store, PASSWORD)
      .await
      .expect("admin has no API scope")
  );
  assert!(store.login("operator", API, 100, 10).await.is_err());
  let issued = store
    .create_access_token("test key", 100, 10)
    .await
    .expect("issue key");
  assert!(
    store
      .authorize_access_token(issued.token(), 109)
      .await
      .expect("valid")
  );
  assert!(
    !store
      .authorize_access_token(issued.token(), 110)
      .await
      .expect("expired")
  );
  assert!(!format!("{issued:?}").contains(issued.token()));
  let metadata = store.list_access_tokens().await.expect("metadata");
  assert_eq!(metadata.len(), 2);
  assert!(
    !serde_json::to_string(&metadata)
      .expect("json")
      .contains(issued.token())
  );
  store
    .revoke_access_token(&issued.metadata.id)
    .await
    .expect("revoke");
  assert!(
    !store
      .authorize_access_token(issued.token(), 101)
      .await
      .expect("revoked")
  );
  let bootstrap = metadata
    .iter()
    .find(|key| key.label == "bootstrap")
    .expect("bootstrap metadata");
  store
    .revoke_access_token(&bootstrap.id)
    .await
    .expect("revoke bootstrap");
  store
    .bootstrap_access_token(API)
    .await
    .expect("restart bootstrap stays revoked");
  assert!(
    !AccessControl::authorize(&store, API)
      .await
      .expect("revoked bootstrap")
  );
  let issued_token = issued.token().to_owned();
  drop(store);
  let reopened = GatewayStorage::open(&path).expect("reopen");
  assert!(
    !AccessControl::authorize(&reopened, API)
      .await
      .expect("durable revocation")
  );
  drop(reopened);
  for entry in std::fs::read_dir(directory.path()).expect("read directory") {
    let entry = entry.expect("entry");
    if entry.file_type().expect("type").is_file() {
      let bytes = std::fs::read(entry.path()).expect("read file");
      for secret in [PASSWORD, API, issued_token.as_str()] {
        assert!(
          !bytes
            .windows(secret.len())
            .any(|window| window == secret.as_bytes())
        );
      }
    }
  }
}
