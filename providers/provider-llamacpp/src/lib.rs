// Embedded, in-process llama.cpp model provider.
//
// The native binding is intentionally opt-in through the `llama-cpp` feature, so normal
// workspace builds do not compile llama.cpp. Configuration and GGUF validation remain
// available without the feature; constructing the provider then returns [`ProviderError::Unavailable`].
//
// GGUF models must contain `tokenizer.chat_template` metadata; the provider deliberately
// returns an error instead of guessing a fallback template. External acceptance can be run with
// a real model via `LYONBROWN_GGUF_MODEL=/path/model.gguf cargo test -p provider-llamacpp
// --features llama-cpp live_completion_smoke_test -- --ignored`.

#[cfg(feature = "llama-cpp")]
use std::ops::Range;
use std::path::{Path, PathBuf};

#[cfg(feature = "llama-cpp")]
use agent_core::{Locality, ModelCapabilities, ModelTier};
use agent_model::ModelError;
#[cfg(feature = "llama-cpp")]
use agent_model::{ModelRequest, Role};

#[cfg(feature = "llama-cpp")]
use std::num::NonZeroU32;
#[cfg(feature = "llama-cpp")]
use std::sync::Arc;

#[cfg(feature = "llama-cpp")]
use agent_core::StopReason;
#[cfg(feature = "llama-cpp")]
use agent_core::TokenUsage;
#[cfg(feature = "llama-cpp")]
use agent_model::ModelEvent;
#[cfg(feature = "llama-cpp")]
use agent_model::{ModelProvider, ModelStream};
#[cfg(feature = "llama-cpp")]
use async_trait::async_trait;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::context::params::LlamaContextParams;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::llama_backend::LlamaBackend;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::llama_batch::LlamaBatch;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::model::params::LlamaModelParams;
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
#[cfg(feature = "llama-cpp")]
use llama_cpp_2::sampling::LlamaSampler;
#[cfg(feature = "llama-cpp")]
use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore, mpsc};
#[cfg(feature = "llama-cpp")]
use tokio_stream::wrappers::ReceiverStream;

/// A validated local GGUF model descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufModel {
  id: String,
  path: PathBuf,
  context_size: u32,
  max_output_tokens: u32,
}

impl GgufModel {
  /// Validates and canonicalizes a model file.
  pub fn new(
    id: impl Into<String>,
    path: impl AsRef<Path>,
    context_size: u32,
    max_output_tokens: u32,
  ) -> Result<Self, ProviderError> {
    let id = id.into();
    if id.trim().is_empty() {
      return Err(ProviderError::InvalidConfig(
        "model id must not be empty".into(),
      ));
    }
    if path.as_ref().extension().and_then(|value| value.to_str()) != Some("gguf") {
      return Err(ProviderError::InvalidConfig(
        "model path must have the .gguf extension".into(),
      ));
    }
    let path = path.as_ref().canonicalize().map_err(|error| {
      ProviderError::InvalidConfig(format!("cannot resolve model path: {error}"))
    })?;
    if !path.is_file() {
      return Err(ProviderError::InvalidConfig(
        "model path must be a regular file".into(),
      ));
    }
    if context_size == 0 || context_size > i32::MAX as u32 {
      return Err(ProviderError::InvalidConfig(
        "context size must be between 1 and i32::MAX".into(),
      ));
    }
    if max_output_tokens == 0 || max_output_tokens > context_size {
      return Err(ProviderError::InvalidConfig(
        "max output tokens must be between 1 and the context size".into(),
      ));
    }
    Ok(Self {
      id,
      path,
      context_size,
      max_output_tokens,
    })
  }

  pub fn id(&self) -> &str {
    &self.id
  }

  pub fn path(&self) -> &Path {
    &self.path
  }

  pub fn context_size(&self) -> u32 {
    self.context_size
  }

  pub fn max_output_tokens(&self) -> u32 {
    self.max_output_tokens
  }
}

/// Default maximum number of prompt tokens decoded by one native call.
pub const DEFAULT_BATCH_SIZE: u32 = 512;
/// Hard limit for prompt batch sizing to keep native allocations bounded.
pub const MAX_BATCH_SIZE: u32 = 4_096;

/// Runtime settings for one embedded llama.cpp model.
#[derive(Debug, Clone, PartialEq)]
pub struct LlamaCppConfig {
  pub model: GgufModel,
  pub threads: u32,
  pub batch_threads: u32,
  /// Maximum prompt tokens decoded per native batch.
  pub batch_size: u32,
  pub gpu_layers: u32,
  pub temperature: f32,
  pub seed: u32,
}

impl LlamaCppConfig {
  pub fn validate(&self) -> Result<(), ProviderError> {
    if self.threads == 0 || self.threads > i32::MAX as u32 {
      return Err(ProviderError::InvalidConfig(
        "threads must be between 1 and i32::MAX".into(),
      ));
    }
    if self.batch_threads == 0 || self.batch_threads > i32::MAX as u32 {
      return Err(ProviderError::InvalidConfig(
        "batch threads must be between 1 and i32::MAX".into(),
      ));
    }
    if self.batch_size == 0
      || self.batch_size > MAX_BATCH_SIZE
      || self.batch_size > self.model.context_size
    {
      return Err(ProviderError::InvalidConfig(format!(
        "batch size must be between 1 and {} and no larger than the context size",
        MAX_BATCH_SIZE
      )));
    }
    if self.gpu_layers > i32::MAX as u32 {
      return Err(ProviderError::InvalidConfig(
        "GPU layers must not exceed i32::MAX".into(),
      ));
    }
    if !self.temperature.is_finite() || self.temperature < 0.0 {
      return Err(ProviderError::InvalidConfig(
        "temperature must be finite and non-negative".into(),
      ));
    }
    Ok(())
  }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
  #[error("invalid llama.cpp configuration: {0}")]
  InvalidConfig(String),
  #[error("invalid llama.cpp request: {0}")]
  InvalidRequest(String),
  #[error("embedded llama.cpp support is unavailable; enable the `llama-cpp` feature")]
  Unavailable,
  #[error("embedded llama.cpp provider is busy with another inference")]
  Busy,
  #[error("llama.cpp failure: {0}")]
  Native(String),
}

impl From<ProviderError> for ModelError {
  fn from(error: ProviderError) -> Self {
    Self::Other(error.to_string())
  }
}

#[cfg(feature = "llama-cpp")]
mod helpers;
mod native;

#[cfg(feature = "llama-cpp")]
use helpers::*;
pub use native::LlamaCppProvider;
