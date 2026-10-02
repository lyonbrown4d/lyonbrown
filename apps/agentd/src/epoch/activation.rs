use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_app::{spawn_runtime, spawn_runtime_with_approvals};
use agent_runtime::composition::{CompositionRoot, RuntimePaths};
use agent_runtime::config::RuntimeConfig;
use agent_runtime::model_manager::{ModelPreset, available_memory_bytes, verify_model_file};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use crate::model_setup::{PreparedRuntime, RuntimeActivator, SetupPlan, managed_runtime_config};
use crate::runtime::{COMMAND_CAPACITY, DaemonPaths, approval_runtime};

pub async fn build_active_runtime(
  config: RuntimeConfig,
  paths: &DaemonPaths,
) -> Result<PreparedRuntime> {
  let (approval, approvals) = approval_runtime(&config);
  let root = CompositionRoot::bootstrap(
    config,
    RuntimePaths::new(paths.workspace().to_path_buf(), paths.home().to_path_buf()),
    Arc::new(approval),
  )
  .await
  .context("failed to initialize agent runtime")?;
  let kernel = root
    .build_kernel()
    .context("failed to build agent kernel")?;
  let (controller, runtime) = match approvals {
    Some(approvals) => spawn_runtime_with_approvals(kernel, approvals, COMMAND_CAPACITY),
    None => spawn_runtime(kernel, COMMAND_CAPACITY),
  };
  Ok(PreparedRuntime::new(controller, runtime))
}

#[derive(Clone)]
pub struct ProductionRuntimeActivator {
  config: RuntimeConfig,
  paths: DaemonPaths,
  preset: Option<&'static ModelPreset>,
  memory: Arc<dyn MemoryProbe>,
}

pub trait MemoryProbe: Send + Sync {
  fn available_memory_bytes(&self) -> u64;
}

struct SystemMemoryProbe;

impl MemoryProbe for SystemMemoryProbe {
  fn available_memory_bytes(&self) -> u64 {
    available_memory_bytes()
  }
}

struct FixedMemoryProbe(u64);

impl MemoryProbe for FixedMemoryProbe {
  fn available_memory_bytes(&self) -> u64 {
    self.0
  }
}

impl ProductionRuntimeActivator {
  pub fn new(config: RuntimeConfig, paths: DaemonPaths, plan: &SetupPlan) -> Self {
    Self::with_dependencies(
      config,
      paths,
      plan.managed_preset(),
      Arc::new(SystemMemoryProbe),
    )
  }

  #[doc(hidden)]
  pub fn with_available_memory(
    config: RuntimeConfig,
    paths: DaemonPaths,
    plan: &SetupPlan,
    available_memory: u64,
  ) -> Self {
    Self::with_dependencies(
      config,
      paths,
      plan.managed_preset(),
      Arc::new(FixedMemoryProbe(available_memory)),
    )
  }

  #[doc(hidden)]
  pub fn with_dependencies(
    config: RuntimeConfig,
    paths: DaemonPaths,
    preset: Option<&'static ModelPreset>,
    memory: Arc<dyn MemoryProbe>,
  ) -> Self {
    Self {
      config,
      paths,
      preset,
      memory,
    }
  }

  #[doc(hidden)]
  pub async fn validated_config_for_installed_model(
    &self,
    installed_model: &Path,
  ) -> Result<RuntimeConfig> {
    let preset = self
      .preset
      .context("runtime setup is not a managed model")?;
    self
      .validate_installed_model(installed_model, preset)
      .await?;
    managed_runtime_config(
      self.config.clone(),
      preset,
      installed_model,
      self.memory.available_memory_bytes(),
    )
  }

  async fn validate_installed_model(
    &self,
    installed_model: &Path,
    preset: &ModelPreset,
  ) -> Result<()> {
    if !installed_model.is_absolute() {
      bail!("managed model installer returned a non-absolute path");
    }
    let expected = self
      .paths
      .home()
      .join(".agent")
      .join("models")
      .join(preset.filename);
    if installed_model != expected {
      bail!("managed model installer returned a path other than the expected managed model path");
    }
    self.validate_expected_parents(&expected).await?;
    let metadata = tokio::fs::symlink_metadata(installed_model)
      .await
      .context("failed to inspect installed model")?;
    if is_link_or_reparse(&metadata) {
      bail!("managed model path is a symlink or reparse point");
    }
    if !metadata.is_file() {
      bail!("managed model installer did not return a regular file");
    }
    let installed = tokio::fs::canonicalize(installed_model)
      .await
      .context("failed to canonicalize installed model")?;
    let expected = tokio::fs::canonicalize(&expected)
      .await
      .context("failed to canonicalize expected managed model")?;
    if installed != expected {
      bail!("managed model installer returned a path other than the expected managed model path");
    }
    managed_runtime_config(
      self.config.clone(),
      preset,
      installed_model,
      self.memory.available_memory_bytes(),
    )?;
    verify_model_file(installed_model, preset)
      .await
      .context("managed model cache verification failed")?;
    Ok(())
  }

  async fn validate_expected_parents(&self, expected: &Path) -> Result<()> {
    let mut parent = expected.parent();
    while let Some(path) = parent {
      if path == self.paths.home() {
        break;
      }
      let metadata = tokio::fs::symlink_metadata(path)
        .await
        .context("failed to inspect managed model parent")?;
      if is_link_or_reparse(&metadata) {
        bail!("managed model parent is a symlink or reparse point");
      }
      parent = path.parent();
    }
    Ok(())
  }
}

#[async_trait]
impl RuntimeActivator for ProductionRuntimeActivator {
  async fn activate(&self, installed_model: PathBuf) -> Result<PreparedRuntime> {
    let config = self
      .validated_config_for_installed_model(&installed_model)
      .await?;
    build_active_runtime(config, &self.paths).await
  }
}

fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
  if metadata.file_type().is_symlink() {
    return true;
  }
  #[cfg(windows)]
  {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
  }
  #[cfg(not(windows))]
  {
    false
  }
}
