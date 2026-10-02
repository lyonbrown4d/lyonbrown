mod model_setup_support;

use std::sync::atomic::Ordering;

use agent_app::{ModelSetupAction, ModelSetupPhase, ModelSetupSource, RuntimeCommandError};
use agentd::model_setup::ModelSetupCoordinator;

use model_setup_support::{
  FakeActivator, FakeInstaller, Harness, InstallMode, daemon_paths, default_config, preset_id,
};

#[tokio::test]
async fn startup_only_inspects_state_and_never_invokes_the_installer() {
  let harness = Harness::managed(InstallMode::Failure, false).await;

  assert_eq!(harness.installer.calls.load(Ordering::SeqCst), 0);
  let view = harness.coordinator.snapshot().unwrap();
  assert_eq!(view.phase, ModelSetupPhase::Required);
  assert_eq!(view.allowed_actions, vec![ModelSetupAction::Download]);
  assert_eq!(view.managed_model.unwrap().id, preset_id());
}

#[tokio::test]
async fn configured_missing_path_is_required_but_cannot_be_downloaded() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut config = default_config(workspace.path(), home.path());
  config.llamacpp.model_path = Some("models/missing.gguf".into());
  let paths = daemon_paths(&config, workspace.path(), home.path());
  let installer = FakeInstaller::new(InstallMode::Failure);
  let (coordinator, _) = ModelSetupCoordinator::from_runtime(
    &config,
    &paths,
    installer.clone(),
    FakeActivator::success(),
  )
  .await
  .unwrap();

  let view = coordinator.snapshot().unwrap();
  assert_eq!(view.source, ModelSetupSource::ConfiguredPath);
  assert_eq!(view.phase, ModelSetupPhase::Required);
  assert!(view.allowed_actions.is_empty());
  assert_eq!(
    coordinator.start(preset_id().into()).await,
    Err(RuntimeCommandError::ModelDownloadUnavailable)
  );
  assert_eq!(installer.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn non_llamacpp_provider_has_no_setup_state_or_download() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let mut config = default_config(workspace.path(), home.path());
  config.model.provider = "ollama".into();
  let paths = daemon_paths(&config, workspace.path(), home.path());
  let (coordinator, _) = ModelSetupCoordinator::from_runtime(
    &config,
    &paths,
    FakeInstaller::new(InstallMode::Failure),
    FakeActivator::success(),
  )
  .await
  .unwrap();

  assert_eq!(coordinator.snapshot(), None);
  assert!(!coordinator.advertises_download());
  assert_eq!(
    coordinator.start(preset_id().into()).await,
    Err(RuntimeCommandError::ModelDownloadUnavailable)
  );
}

#[tokio::test]
async fn clients_can_only_request_the_configured_catalog_model_id() {
  let harness = Harness::managed(InstallMode::Failure, false).await;

  assert_eq!(
    harness
      .coordinator
      .start("https://evil.invalid/model".into())
      .await,
    Err(RuntimeCommandError::InvalidModelId {
      requested: "https://evil.invalid/model".into(),
      configured: preset_id().into(),
    })
  );
  assert_eq!(harness.installer.calls.load(Ordering::SeqCst), 0);
}
