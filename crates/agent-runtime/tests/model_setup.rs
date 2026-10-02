use std::collections::HashMap;

use agent_runtime::config::{LoadOptions, RuntimeConfig};
use agent_runtime::model_manager::model_setup_required;

#[tokio::test]
async fn default_llamacpp_config_without_a_cached_model_requires_setup() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let config = RuntimeConfig::load(options).unwrap();

  assert!(
    model_setup_required(&config, workspace.path(), home.path())
      .await
      .unwrap()
  );
}

#[tokio::test]
async fn remote_provider_does_not_require_local_model_setup() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  options.cli.provider = Some("ollama".into());
  let config = RuntimeConfig::load(options).unwrap();

  assert!(
    !model_setup_required(&config, workspace.path(), home.path())
      .await
      .unwrap()
  );
}

#[tokio::test]
async fn missing_relative_model_path_requires_setup() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.llamacpp.model_path = Some("models/missing.gguf".into());

  assert!(
    model_setup_required(&config, workspace.path(), home.path())
      .await
      .unwrap()
  );
}

#[tokio::test]
async fn missing_absolute_model_path_requires_setup() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.llamacpp.model_path = Some(home.path().join("missing.gguf"));

  assert!(
    model_setup_required(&config, workspace.path(), home.path())
      .await
      .unwrap()
  );
}

#[tokio::test]
async fn configured_model_directory_requires_setup() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.llamacpp.model_path = Some(workspace.path().to_path_buf());

  assert!(
    model_setup_required(&config, workspace.path(), home.path())
      .await
      .unwrap()
  );
}

#[tokio::test]
async fn existing_configured_model_file_does_not_require_setup() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let model = workspace.path().join("model.gguf");
  std::fs::write(&model, b"test fixture").unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.llamacpp.model_path = Some(model);

  assert!(
    !model_setup_required(&config, workspace.path(), home.path())
      .await
      .unwrap()
  );
}
