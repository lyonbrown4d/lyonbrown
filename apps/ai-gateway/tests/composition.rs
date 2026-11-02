use ai_gateway::config;
#[path = "../src/composition.rs"]
mod composition;

use composition::GatewayModule;
use decision_core::{ModelTier, Policy};
use fluxdi::Shared;
use gateway_core::{GatewayConfig, ModelConfig, Secret};
use std::{collections::BTreeSet, path::Path};

const PASSWORD: &str = "composition-private-admin-password";
const TOKEN: &str = "composition-independent-api-token-at-least-32-bytes";

fn settings(directory: &Path, password: &str) -> config::AppConfig {
  config::AppConfig {
    listen: "127.0.0.1:8099".parse().expect("address"),
    data_dir: directory.into(),
    web_root: directory.join("web"),
    admin_username: "composition.admin".into(),
    admin_password: Secret::new(password),
    gateway: GatewayConfig {
      token: Secret::new(TOKEN),
      models: vec![ModelConfig {
        id: "fixture".into(),
        upstream_model: "fixture-model".into(),
        api_base: "http://127.0.0.1:1/v1".into(),
        api_key: None,
        api_key_env: None,
        tier: ModelTier::Balanced,
        local_inference: true,
        capabilities: BTreeSet::from(["text".into()]),
        context_tokens: Some(65_536),
      }],
      endpoints: vec![],
      limits: Default::default(),
      constraints: Default::default(),
    },
  }
}

#[tokio::test]
async fn concurrent_resolution_bootstraps_once_and_restart_loads_persisted_policy_and_credentials()
{
  let directory = tempfile::tempdir().expect("directory");
  let module = GatewayModule::new(Shared::new(settings(directory.path(), PASSWORD)));
  let (first, second) = tokio::join!(module.resolve(), module.resolve());
  let first = first.expect("first resolution");
  let second = second.expect("second resolution");
  assert!(Shared::ptr_eq(&first, &second));
  assert!(Shared::ptr_eq(
    &first.gateway,
    &module.gateway().await.expect("gateway")
  ));
  assert_eq!(
    first
      .storage
      .list_access_tokens()
      .await
      .expect("keys")
      .len(),
    1
  );
  assert!(
    first
      .storage
      .authorize_access_token(
        TOKEN,
        std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .expect("clock")
          .as_secs()
      )
      .await
      .expect("API bootstrap")
  );
  assert_eq!(
    first
      .storage
      .load_models()
      .await
      .expect("models")
      .expect("snapshot")
      .models
      .len(),
    1
  );
  let session = first
    .storage
    .login("composition.admin", PASSWORD, 100, 100)
    .await
    .expect("admin ready");
  let policy = Policy {
    revision: "operator-policy-v2".into(),
    ..Policy::default()
  };
  first
    .storage
    .save_policy(policy.clone(), Some(Policy::default().revision))
    .await
    .expect("operator policy");
  drop(first);
  drop(second);
  drop(module);

  let module = GatewayModule::new(Shared::new(settings(directory.path(), PASSWORD)));
  let app = module.resolve().await.expect("restart composition");
  assert_eq!(app.decision.snapshot().policy(), &policy);
  assert!(
    app
      .storage
      .verify_csrf(session.token(), session.csrf_token(), 101)
      .await
      .expect("persisted session")
  );
  assert_eq!(
    app
      .storage
      .list_access_tokens()
      .await
      .expect("keys after restart")
      .len(),
    1
  );
}

#[tokio::test]
async fn conflicting_bootstrap_credentials_prevent_application_resolution_without_overwriting_account()
 {
  let directory = tempfile::tempdir().expect("directory");
  let module = GatewayModule::new(Shared::new(settings(directory.path(), PASSWORD)));
  module.resolve().await.expect("initial startup");
  drop(module);

  let wrong_password = "conflicting-private-password";
  let module = GatewayModule::new(Shared::new(settings(directory.path(), wrong_password)));
  let result = module.resolve().await;
  let error = match result {
    Ok(_) => panic!("conflicting credentials must prevent startup"),
    Err(error) => error,
  };
  let message = error.to_string();
  assert!(!message.contains(wrong_password));
  assert!(!message.contains(PASSWORD));
  assert!(!message.contains(TOKEN));
  let storage = module.storage().await.expect("existing storage");
  storage
    .login("composition.admin", PASSWORD, 100, 100)
    .await
    .expect("original account preserved");
  assert!(
    storage
      .login("composition.admin", wrong_password, 100, 100)
      .await
      .is_err()
  );
}
