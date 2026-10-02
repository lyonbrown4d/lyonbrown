use std::path::PathBuf;

use agent_app::{
  ManagedModelView, ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView,
};
use agent_runtime::config::RuntimeConfig;
use agent_runtime::model_manager::{
  ModelPreset, available_memory_bytes, cached_model, requested_preset,
};
use anyhow::{Context, Result, bail};

use crate::runtime::DaemonPaths;

#[derive(Clone, Copy)]
pub(super) enum SetupTarget {
  Disabled,
  Unavailable,
  Managed(&'static ModelPreset),
}

#[derive(Clone)]
pub struct SetupPlan {
  pub(super) target: SetupTarget,
  pub(super) cache_dir: PathBuf,
  pub(super) initial: Option<ModelSetupView>,
  initial_runtime: InitialRuntime,
}

#[derive(Clone)]
enum InitialRuntime {
  Waiting,
  Active { managed_model_path: Option<PathBuf> },
}

impl SetupPlan {
  pub(crate) fn runtime_config(&self, config: RuntimeConfig) -> Result<Option<RuntimeConfig>> {
    match &self.initial_runtime {
      InitialRuntime::Waiting => Ok(None),
      InitialRuntime::Active { managed_model_path } => {
        if let Some(path) = managed_model_path {
          let SetupTarget::Managed(preset) = self.target else {
            bail!("managed runtime plan lost its catalog provenance");
          };
          return managed_runtime_config(config, preset, path, available_memory_bytes()).map(Some);
        }
        Ok(Some(config))
      }
    }
  }

  pub(crate) fn managed_preset(&self) -> Option<&'static ModelPreset> {
    match self.target {
      SetupTarget::Managed(preset) => Some(preset),
      SetupTarget::Disabled | SetupTarget::Unavailable => None,
    }
  }
}

#[doc(hidden)]
pub fn managed_runtime_config(
  mut config: RuntimeConfig,
  preset: &ModelPreset,
  model_path: &std::path::Path,
  available_memory: u64,
) -> Result<RuntimeConfig> {
  ensure_managed_memory(preset, available_memory)?;
  config.llamacpp.model_path = Some(model_path.to_path_buf());
  Ok(config)
}

fn ensure_managed_memory(preset: &ModelPreset, available_memory: u64) -> Result<()> {
  if available_memory < preset.min_available_memory_bytes {
    bail!(
      "model preset '{}' needs at least {:.1} GiB available RAM; found {:.1} GiB",
      preset.id,
      preset.min_available_memory_bytes as f64 / (1024_u64.pow(3)) as f64,
      available_memory as f64 / (1024_u64.pow(3)) as f64
    );
  }
  Ok(())
}

#[doc(hidden)]
pub async fn inspect_setup_plan(config: &RuntimeConfig, paths: &DaemonPaths) -> Result<SetupPlan> {
  let cache_dir = paths.home().join(".agent").join("models");
  if config.model.provider != "llamacpp" {
    return Ok(SetupPlan {
      target: SetupTarget::Disabled,
      cache_dir,
      initial: None,
      initial_runtime: InitialRuntime::Active {
        managed_model_path: None,
      },
    });
  }
  if let Some(configured) = &config.llamacpp.model_path {
    let path = if configured.is_absolute() {
      configured.clone()
    } else {
      paths.workspace().join(configured)
    };
    let exists = match tokio::fs::metadata(&path).await {
      Ok(metadata) => metadata.is_file(),
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
      Err(error) => return Err(error).context("failed to inspect configured model"),
    };
    return Ok(SetupPlan {
      target: SetupTarget::Unavailable,
      cache_dir,
      initial: (!exists).then(configured_path_required),
      initial_runtime: if exists {
        InitialRuntime::Active {
          managed_model_path: None,
        }
      } else {
        InitialRuntime::Waiting
      },
    });
  }
  let preset = requested_preset(config.model.model.as_deref())?;
  let cached = cached_model(&cache_dir, preset)
    .await
    .context("failed to inspect managed model cache")?;
  let ready = cached.is_some();
  Ok(SetupPlan {
    target: SetupTarget::Managed(preset),
    cache_dir,
    initial: Some(managed_view(
      preset,
      if ready {
        ModelSetupPhase::Ready
      } else {
        ModelSetupPhase::Required
      },
      None,
      0,
      None,
      if ready {
        Vec::new()
      } else {
        vec![ModelSetupAction::Download]
      },
    )),
    initial_runtime: match cached {
      Some(path) => InitialRuntime::Active {
        managed_model_path: Some(path),
      },
      None => InitialRuntime::Waiting,
    },
  })
}

pub(super) fn managed_view(
  preset: &ModelPreset,
  phase: ModelSetupPhase,
  operation_id: Option<String>,
  downloaded_bytes: u64,
  total_bytes: Option<u64>,
  allowed_actions: Vec<ModelSetupAction>,
) -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase,
    managed_model: Some(ManagedModelView {
      id: preset.id.into(),
      expected_download_bytes: preset.download_bytes,
      max_bytes: preset.max_bytes,
      min_available_memory_bytes: preset.min_available_memory_bytes,
      sha256: preset.sha256.to_string(),
    }),
    operation_id,
    downloaded_bytes,
    total_bytes,
    failure: None,
    allowed_actions,
  }
}

fn configured_path_required() -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ConfiguredPath,
    phase: ModelSetupPhase::Required,
    managed_model: None,
    operation_id: None,
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions: Vec::new(),
  }
}
