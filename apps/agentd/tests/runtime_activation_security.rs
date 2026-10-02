use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::config::{LoadOptions, RuntimeConfig};
use agent_runtime::model_manager::ModelPreset;
use agentd::runtime::{DaemonPaths, MemoryProbe, ProductionRuntimeActivator};
use sha2::{Digest, Sha256};

struct MutableMemoryProbe(AtomicU64);

impl MutableMemoryProbe {
  fn new(value: u64) -> Arc<Self> {
    Arc::new(Self(AtomicU64::new(value)))
  }

  fn set(&self, value: u64) {
    self.0.store(value, Ordering::SeqCst);
  }
}

impl MemoryProbe for MutableMemoryProbe {
  fn available_memory_bytes(&self) -> u64 {
    self.0.load(Ordering::SeqCst)
  }
}

fn preset(bytes: &[u8]) -> &'static ModelPreset {
  Box::leak(Box::new(ModelPreset {
    id: "activation-security-test",
    owner: "test",
    repository: "test",
    revision: "test",
    filename: "activation-security.gguf",
    sha256: Cow::Owned(format!("{:x}", Sha256::digest(bytes))),
    download_bytes: bytes.len() as u64,
    max_bytes: 1024,
    min_available_memory_bytes: 10,
  }))
}

fn config_and_paths(
  workspace: &std::path::Path,
  home: &std::path::Path,
) -> (RuntimeConfig, DaemonPaths) {
  let mut options = LoadOptions::for_workspace(workspace);
  options.global_config = home.join("missing.toml");
  options.environment = Some(HashMap::new());
  let config = RuntimeConfig::load(options).unwrap();
  let paths = DaemonPaths::resolve(workspace, home, &config.fingerprint().unwrap()).unwrap();
  (config, paths)
}

async fn model_path(
  home: &std::path::Path,
  preset: &ModelPreset,
  bytes: &[u8],
) -> std::path::PathBuf {
  let path = home.join(".agent/models").join(preset.filename);
  tokio::fs::create_dir_all(path.parent().unwrap())
    .await
    .unwrap();
  tokio::fs::write(&path, bytes).await.unwrap();
  path
}

#[tokio::test]
async fn memory_is_probed_at_each_activation_high_then_low() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let expected = b"GGUF-valid";
  let preset = preset(expected);
  let path = model_path(home.path(), preset, expected).await;
  let (config, paths) = config_and_paths(workspace.path(), home.path());
  let memory = MutableMemoryProbe::new(100);
  let activator =
    ProductionRuntimeActivator::with_dependencies(config, paths, Some(preset), memory.clone());
  memory.set(0);

  let error = activator
    .validated_config_for_installed_model(&path)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("available RAM"));
}

#[tokio::test]
async fn memory_is_probed_at_each_activation_low_then_high() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let expected = b"GGUF-valid";
  let preset = preset(expected);
  let path = model_path(home.path(), preset, expected).await;
  let (config, paths) = config_and_paths(workspace.path(), home.path());
  let memory = MutableMemoryProbe::new(0);
  let activator =
    ProductionRuntimeActivator::with_dependencies(config, paths, Some(preset), memory.clone());
  memory.set(100);

  let activated = activator
    .validated_config_for_installed_model(&path)
    .await
    .unwrap();

  assert_eq!(
    activated.llamacpp.model_path.as_deref(),
    Some(path.as_path())
  );
}

#[tokio::test]
async fn activation_rejects_a_tampered_expected_cache_file() {
  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let preset = preset(b"GGUF-original");
  let path = model_path(home.path(), preset, b"GGUF-tampered").await;
  let (config, paths) = config_and_paths(workspace.path(), home.path());
  let activator = ProductionRuntimeActivator::with_dependencies(
    config,
    paths,
    Some(preset),
    MutableMemoryProbe::new(100),
  );

  let error = activator
    .validated_config_for_installed_model(&path)
    .await
    .unwrap_err();

  assert!(
    error
      .chain()
      .any(|cause| cause.to_string().contains("digest")),
    "expected a digest verification failure, got: {error:#}"
  );
}

#[cfg(unix)]
#[tokio::test]
async fn activation_rejects_a_symlinked_cache_parent() {
  use std::os::unix::fs::symlink;

  let workspace = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let target = tempfile::tempdir().unwrap();
  let expected = b"GGUF-valid";
  let preset = preset(expected);
  tokio::fs::create_dir_all(home.path().join(".agent"))
    .await
    .unwrap();
  symlink(target.path(), home.path().join(".agent/models")).unwrap();
  let path = target.path().join(preset.filename);
  tokio::fs::write(&path, expected).await.unwrap();
  let lexical = home.path().join(".agent/models").join(preset.filename);
  let (config, paths) = config_and_paths(workspace.path(), home.path());
  let activator = ProductionRuntimeActivator::with_dependencies(
    config,
    paths,
    Some(preset),
    MutableMemoryProbe::new(100),
  );

  let error = activator
    .validated_config_for_installed_model(&lexical)
    .await
    .unwrap_err();

  assert!(
    error
      .to_string()
      .contains("parent is a symlink or reparse point")
  );
}
