use ai_gateway::config::AppConfig;
use gateway_storage::{AdminRole, GatewayStorage, StorageError};
use std::collections::BTreeMap;

#[tokio::test]
async fn environment_bootstrap_preserves_the_unique_super_admin_across_restarts() {
  let directory = tempfile::tempdir().expect("tempdir");
  let config_path = directory.path().join("gateway.toml");
  std::fs::write(
    &config_path,
    r#"
[gateway]
token = "bootstrap-independent-api-token-at-least-32-bytes"
[[gateway.models]]
id = "local"
upstream_model = "actual"
api_base = "http://127.0.0.1:1234/v1"
tier = "balanced"
local_inference = true
capabilities = ["text"]
context_tokens = 65536
"#,
  )
  .expect("configuration");
  let environment = BTreeMap::from([
    (
      "LYONBROWN_GATEWAY_ADMIN_USERNAME".into(),
      "bootstrap.admin".into(),
    ),
    (
      "LYONBROWN_GATEWAY_ADMIN_PASSWORD".into(),
      "private-bootstrap-password".into(),
    ),
  ]);
  let settings =
    AppConfig::load_with_environment(directory.path(), Some(&config_path), &environment)
      .expect("environment credentials");
  let path = settings.data_dir.join("gateway.sqlite");
  let store = GatewayStorage::open(&path).expect("store");
  store
    .bootstrap_super_admin(
      &settings.admin_username,
      settings.admin_password.expose_secret(),
    )
    .await
    .expect("initial super admin");
  store
    .bootstrap_super_admin(
      &settings.admin_username,
      settings.admin_password.expose_secret(),
    )
    .await
    .expect("same credentials are idempotent");
  let session = store
    .login(
      &settings.admin_username,
      settings.admin_password.expose_secret(),
      100,
      60,
    )
    .await
    .expect("username/password login");
  assert_eq!(session.identity.username, settings.admin_username);
  assert_eq!(session.identity.role, AdminRole::SuperAdmin);
  drop(session);
  drop(store);
  let reopened = GatewayStorage::open(&path).expect("reopen");
  for (username, password) in [
    ("different-admin", settings.admin_password.expose_secret()),
    (
      settings.admin_username.as_str(),
      "different-private-password",
    ),
  ] {
    assert!(matches!(
      reopened.bootstrap_super_admin(username, password).await,
      Err(StorageError::Conflict)
    ));
    assert!(reopened.login(username, password, 101, 60).await.is_err());
  }
  let session = reopened
    .login(
      &settings.admin_username,
      settings.admin_password.expose_secret(),
      101,
      60,
    )
    .await
    .expect("original credentials remain active");
  let identity = reopened
    .session_identity(session.token(), 102)
    .await
    .expect("session lookup")
    .expect("active identity");
  assert_eq!(identity.username, settings.admin_username);
  assert_eq!(identity.role, AdminRole::SuperAdmin);
}
