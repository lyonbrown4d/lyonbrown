use crate::{EmbeddingDescriptor, EmbeddingProvider, embedding::normalize};
use decision_core::BackendError;
use fastembed::{
  InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::Mutex};

/// An explicit deployment asset. SHA-256 is checked before passing bytes to the runtime.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerifiedAsset {
  pub path: PathBuf,
  pub sha256: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflinePooling {
  Mean,
  Cls,
}

/// Prepared assets only: this API never resolves a model name or accesses a model hub.
/// The application owns process-wide ORT initialization; the first loader selects its library.
/// Models with external ONNX initializers are unsupported by this byte-only configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OfflineEmbeddingConfig {
  pub model: VerifiedAsset,
  pub tokenizer: VerifiedAsset,
  pub config: VerifiedAsset,
  pub special_tokens: VerifiedAsset,
  pub tokenizer_config: VerifiedAsset,
  pub runtime_library: VerifiedAsset,
  pub model_revision: String,
  pub preprocessing_revision: String,
  pub license: String,
  pub dimensions: usize,
  pub max_tokens: usize,
  pub intra_threads: usize,
  pub max_asset_bytes: u64,
  pub pooling: OfflinePooling,
}

pub struct LocalFastEmbed {
  descriptor: EmbeddingDescriptor,
  model: Mutex<TextEmbedding>,
}
impl LocalFastEmbed {
  /// Asset I/O and model initialization run outside Tokio's core I/O workers.
  pub async fn load(config: OfflineEmbeddingConfig) -> Result<Self, BackendError> {
    tokio::task::spawn_blocking(move || Self::load_blocking(config))
      .await
      .map_err(|_| BackendError::Unavailable)?
  }
  fn load_blocking(config: OfflineEmbeddingConfig) -> Result<Self, BackendError> {
    if config.model_revision.trim().is_empty()
      || config.preprocessing_revision.trim().is_empty()
      || config.license.trim().is_empty()
      || config.dimensions == 0
      || config.dimensions > 65_536
      || config.max_tokens == 0
      || config.max_tokens > 1_048_576
      || config.intra_threads == 0
      || config.max_asset_bytes == 0
    {
      return Err(BackendError::InvalidInput);
    }
    let read = |asset: &VerifiedAsset| read_asset(asset, config.max_asset_bytes);
    let onnx = read(&config.model)?;
    let tokenizer_file = read(&config.tokenizer)?;
    let config_file = read(&config.config)?;
    let special_tokens_map_file = read(&config.special_tokens)?;
    let tokenizer_config_file = read(&config.tokenizer_config)?;
    // Verify every explicit asset before touching the process-wide native runtime.
    let _runtime = read(&config.runtime_library)?;
    let runtime =
      ort::init_from(&config.runtime_library.path).map_err(|_| BackendError::Unavailable)?;
    // Another component may already own the process-wide environment; ORT retains it.
    runtime.commit();
    let model = UserDefinedEmbeddingModel::new(
      onnx,
      TokenizerFiles {
        tokenizer_file,
        config_file,
        special_tokens_map_file,
        tokenizer_config_file,
      },
    )
    .with_pooling(match config.pooling {
      OfflinePooling::Mean => Pooling::Mean,
      OfflinePooling::Cls => Pooling::Cls,
    });
    let model = TextEmbedding::try_new_from_user_defined(
      model,
      InitOptionsUserDefined::new()
        .with_max_length(config.max_tokens)
        .with_intra_threads(config.intra_threads),
    )
    .map_err(|_| BackendError::Unavailable)?;
    let mut preprocessing = Sha256::new();
    for asset in [
      &config.tokenizer,
      &config.config,
      &config.special_tokens,
      &config.tokenizer_config,
    ] {
      preprocessing.update(asset.sha256.as_bytes());
    }
    preprocessing.update((config.max_tokens as u64).to_le_bytes());
    preprocessing.update((config.intra_threads as u64).to_le_bytes());
    preprocessing.update([match config.pooling {
      OfflinePooling::Mean => 0,
      OfflinePooling::Cls => 1,
    }]);
    let provider = Self {
      descriptor: EmbeddingDescriptor {
        model_revision: format!(
          "{};model-sha256={};runtime-sha256={}",
          config.model_revision, config.model.sha256, config.runtime_library.sha256
        ),
        preprocessing_revision: format!(
          "{};sha256={}",
          config.preprocessing_revision,
          hex::encode(preprocessing.finalize())
        ),
        dimensions: config.dimensions,
        local: true,
      },
      model: Mutex::new(model),
    };
    // A real inference validates tensor outputs and caller-declared dimensions at load time.
    provider.embed("embedding dimension validation")?;
    Ok(provider)
  }
}
impl EmbeddingProvider for LocalFastEmbed {
  fn descriptor(&self) -> EmbeddingDescriptor {
    self.descriptor.clone()
  }
  fn embed(&self, text: &str) -> Result<Vec<f32>, BackendError> {
    if text.trim().is_empty() {
      return Err(BackendError::InvalidInput);
    }
    let mut model = self.model.lock().map_err(|_| BackendError::Unavailable)?;
    let mut embeddings = model
      .embed(vec![text], Some(1))
      .map_err(|_| BackendError::Unavailable)?;
    if embeddings.len() != 1 {
      return Err(BackendError::Unavailable);
    }
    normalize(
      embeddings.pop().ok_or(BackendError::Unavailable)?,
      self.descriptor.dimensions,
    )
  }
}
fn read_asset(asset: &VerifiedAsset, max_bytes: u64) -> Result<Vec<u8>, BackendError> {
  let expected = hex::decode(&asset.sha256).map_err(|_| BackendError::InvalidInput)?;
  if expected.len() != 32 {
    return Err(BackendError::InvalidInput);
  }
  let metadata = fs::metadata(&asset.path).map_err(|_| BackendError::InvalidInput)?;
  if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
    return Err(BackendError::InvalidInput);
  }
  let bytes = fs::read(&asset.path).map_err(|_| BackendError::InvalidInput)?;
  if bytes.is_empty()
    || bytes.len() as u64 > max_bytes
    || Sha256::digest(&bytes).as_slice() != expected.as_slice()
  {
    return Err(BackendError::InvalidInput);
  }
  Ok(bytes)
}
