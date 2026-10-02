mod model_setup_support;

use std::collections::HashMap;

use agent_app::RuntimePhase;
use agent_runtime::config::{LoadOptions, RuntimeConfig};
use agent_runtime::model_manager::default_preset;
use agentd::model_setup::{ModelSetupCoordinator, inspect_setup_plan};
use agentd::runtime::{
  DaemonPaths, ProductionRuntimeActivator, build_active_runtime, build_runtime,
  build_runtime_from_setup_plan, managed_runtime_config,
};

use model_setup_support::{FakeActivator, FakeInstaller, InstallMode};

#[tokio::test]
async fn missing_default_model_starts_a_control_plane_runtime() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let config = RuntimeConfig::load(options).unwrap();
  let fingerprint = config.fingerprint().unwrap();
  let paths = DaemonPaths::resolve(workspace.path(), home.path(), &fingerprint).unwrap();

  let (controller, runtime) = build_runtime(config, &paths).await.unwrap();

  assert_eq!(
    controller.snapshot().phase,
    RuntimePhase::ModelSetupRequired
  );
  drop(controller);
  runtime.join().await.unwrap();
}

#[tokio::test]
async fn missing_configured_model_starts_a_control_plane_runtime() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.llamacpp.model_path = Some("models/missing.gguf".into());
  let fingerprint = config.fingerprint().unwrap();
  let paths = DaemonPaths::resolve(workspace.path(), home.path(), &fingerprint).unwrap();

  let (controller, runtime) = build_runtime(config, &paths).await.unwrap();

  assert_eq!(
    controller.snapshot().phase,
    RuntimePhase::ModelSetupRequired
  );
  drop(controller);
  runtime.join().await.unwrap();
}

#[tokio::test]
async fn shared_setup_plan_keeps_coordinator_and_runtime_startup_consistent() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let model = workspace.path().join("configured.gguf");
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.llamacpp.model_path = Some("configured.gguf".into());
  config.storage.enabled = false;
  let fingerprint = config.fingerprint().unwrap();
  let paths = DaemonPaths::resolve(workspace.path(), home.path(), &fingerprint).unwrap();
  let plan = inspect_setup_plan(&config, &paths).await.unwrap();

  tokio::fs::write(model, b"model").await.unwrap();
  let (coordinator, _) = ModelSetupCoordinator::from_plan(
    plan.clone(),
    FakeInstaller::new(InstallMode::Failure),
    FakeActivator::success(),
  );
  let (controller, runtime) = build_runtime_from_setup_plan(config, &paths, &plan)
    .await
    .unwrap();

  assert_eq!(
    coordinator.snapshot().unwrap().phase,
    agent_app::ModelSetupPhase::Required
  );
  assert_eq!(
    controller.snapshot().phase,
    RuntimePhase::ModelSetupRequired
  );
  drop(controller);
  runtime.join().await.unwrap();
}

#[tokio::test]
async fn active_runtime_builder_never_returns_the_waiting_runtime() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let mut config = RuntimeConfig::load(options).unwrap();
  config.model.provider = "ollama".into();
  config.storage.enabled = false;
  let fingerprint = config.fingerprint().unwrap();
  let paths = DaemonPaths::resolve(workspace.path(), home.path(), &fingerprint).unwrap();

  let prepared = build_active_runtime(config, &paths).await.unwrap();

  assert_eq!(prepared.controller.snapshot().phase, RuntimePhase::Ready);
  let shutdown = prepared.runtime.shutdown_handle();
  shutdown.request_shutdown();
  drop(prepared.controller);
  prepared.runtime.join().await.unwrap();
}

#[tokio::test]
async fn production_activator_rejects_an_unverified_expected_model() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut options = LoadOptions::for_workspace(workspace.path());
  options.global_config = home.path().join("missing.toml");
  options.environment = Some(HashMap::new());
  let config = RuntimeConfig::load(options).unwrap();
  let fingerprint = config.fingerprint().unwrap();
  let paths = DaemonPaths::resolve(workspace.path(), home.path(), &fingerprint).unwrap();
  let plan = inspect_setup_plan(&config, &paths).await.unwrap();
  let installed = home
    .path()
    .join(".agent/models")
    .join(default_preset().filename);
  tokio::fs::create_dir_all(installed.parent().unwrap())
    .await
    .unwrap();
  tokio::fs::write(&installed, b"model").await.unwrap();
  let activator = ProductionRuntimeActivator::new(config, paths, &plan);

  let error = activator
    .validated_config_for_installed_model(&installed)
    .await
    .unwrap_err();

  assert!(
    error
      .chain()
      .any(|cause| cause.to_string().contains("digest")),
    "expected a digest verification failure, got: {error:#}"
  );
}

#[tokio::test]
async fn production_activator_rejects_a_different_absolute_model() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let config = default_runtime_config(workspace.path(), home.path());
  let paths = daemon_paths(&config, workspace.path(), home.path());
  let plan = inspect_setup_plan(&config, &paths).await.unwrap();
  let wrong = home.path().join("other.gguf");
  tokio::fs::write(&wrong, b"model").await.unwrap();
  let activator = ProductionRuntimeActivator::new(config, paths, &plan);

  let error = activator
    .validated_config_for_installed_model(&wrong)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("expected managed model path"));
}

#[tokio::test]
async fn production_activator_rejects_managed_activation_for_configured_path() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut config = default_runtime_config(workspace.path(), home.path());
  let configured = workspace.path().join("configured.gguf");
  tokio::fs::write(&configured, b"model").await.unwrap();
  config.llamacpp.model_path = Some(configured.clone());
  let paths = daemon_paths(&config, workspace.path(), home.path());
  let plan = inspect_setup_plan(&config, &paths).await.unwrap();
  let activator = ProductionRuntimeActivator::new(config, paths, &plan);

  let error = activator
    .validated_config_for_installed_model(&configured)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("not a managed model"));
}

#[tokio::test]
async fn production_activator_rejects_low_memory_after_download() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let config = default_runtime_config(workspace.path(), home.path());
  let paths = daemon_paths(&config, workspace.path(), home.path());
  let plan = inspect_setup_plan(&config, &paths).await.unwrap();
  let installed = home
    .path()
    .join(".agent/models")
    .join(default_preset().filename);
  tokio::fs::create_dir_all(installed.parent().unwrap())
    .await
    .unwrap();
  tokio::fs::write(&installed, b"model").await.unwrap();
  let activator = ProductionRuntimeActivator::with_available_memory(config, paths, &plan, 0);

  let error = activator
    .validated_config_for_installed_model(&installed)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("available RAM"));
}

#[test]
fn cached_managed_startup_rejects_low_memory() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let config = default_runtime_config(workspace.path(), home.path());
  let model = home
    .path()
    .join(".agent/models")
    .join(default_preset().filename);

  let error = managed_runtime_config(config, default_preset(), &model, 0).unwrap_err();

  assert!(error.to_string().contains("available RAM"));
}

#[cfg(unix)]
#[tokio::test]
async fn production_activator_rejects_a_symlink_at_the_expected_path() {
  use std::os::unix::fs::symlink;

  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let config = default_runtime_config(workspace.path(), home.path());
  let paths = daemon_paths(&config, workspace.path(), home.path());
  let plan = inspect_setup_plan(&config, &paths).await.unwrap();
  let expected = home
    .path()
    .join(".agent/models")
    .join(default_preset().filename);
  let target = home.path().join("target.gguf");
  tokio::fs::create_dir_all(expected.parent().unwrap())
    .await
    .unwrap();
  tokio::fs::write(&target, b"model").await.unwrap();
  symlink(&target, &expected).unwrap();
  let activator = ProductionRuntimeActivator::new(config, paths, &plan);

  let error = activator
    .validated_config_for_installed_model(&expected)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("symlink or reparse point"));
}

fn default_runtime_config(workspace: &std::path::Path, home: &std::path::Path) -> RuntimeConfig {
  let mut options = LoadOptions::for_workspace(workspace);
  options.global_config = home.join("missing.toml");
  options.environment = Some(HashMap::new());
  RuntimeConfig::load(options).unwrap()
}

fn daemon_paths(
  config: &RuntimeConfig,
  workspace: &std::path::Path,
  home: &std::path::Path,
) -> DaemonPaths {
  DaemonPaths::resolve(workspace, home, &config.fingerprint().unwrap()).unwrap()
}
