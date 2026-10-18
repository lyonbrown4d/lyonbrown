use crate::{InferenceError, ModelInfo, Result};
use mistralrs::{DeviceMapSetting, GgufModelBuilder, Model, ModelBuilder, TokenSource};
use std::path::Path;

pub(crate) async fn directory(
  id: String,
  path: &Path,
  context_window: u32,
  supports_tools: bool,
  supports_images: bool,
  force_cpu: bool,
) -> Result<(Model, ModelInfo)> {
  let path = std::fs::canonicalize(path).map_err(backend_error)?;
  if !path.is_dir() || !path.join("config.json").is_file() || !path.join("tokenizer.json").is_file()
  {
    return Err(InferenceError::InvalidRequest(
      "a complete local model directory with config.json and tokenizer.json is required".into(),
    ));
  }
  let mut builder = ModelBuilder::new(path.to_string_lossy())
    .with_token_source(TokenSource::None)
    .with_max_num_seqs(1);
  if force_cpu {
    // An explicitly selected CPU does not need automatic device placement.
    // Keep loading usable when host memory introspection is unavailable.
    builder = builder
      .with_force_cpu()
      .with_device_mapping(DeviceMapSetting::dummy());
  }
  let model = builder.build().await.map_err(backend_error)?;
  finish(model, id, context_window, supports_tools, supports_images)
}

pub(crate) async fn gguf(
  id: String,
  path: &Path,
  context_window: u32,
  supports_tools: bool,
  force_cpu: bool,
) -> Result<(Model, ModelInfo)> {
  let path = std::fs::canonicalize(path).map_err(backend_error)?;
  if !path.is_file() {
    return Err(InferenceError::InvalidRequest(
      "a local GGUF file is required".into(),
    ));
  }
  let parent = path
    .parent()
    .ok_or_else(|| InferenceError::InvalidRequest("GGUF parent directory missing".into()))?;
  let file = path
    .file_name()
    .ok_or_else(|| InferenceError::InvalidRequest("GGUF filename missing".into()))?;
  let mut builder = GgufModelBuilder::new(parent.to_string_lossy(), vec![file.to_string_lossy()])
    .with_token_source(TokenSource::None)
    .with_max_num_seqs(1);
  if force_cpu {
    // An explicitly selected CPU does not need automatic device placement.
    // Keep loading usable when host memory introspection is unavailable.
    builder = builder
      .with_force_cpu()
      .with_device_mapping(DeviceMapSetting::dummy());
  }
  let model = builder.build().await.map_err(backend_error)?;
  finish(model, id, context_window, supports_tools, false)
}
fn finish(
  model: Model,
  id: String,
  context_window: u32,
  supports_tools: bool,
  supports_images: bool,
) -> Result<(Model, ModelInfo)> {
  if supports_images
    && !matches!(
      model.config().map_err(backend_error)?.category,
      mistralrs::ModelCategory::Multimodal { .. }
    )
  {
    return Err(InferenceError::InvalidRequest(
      "loaded model has no multimodal pipeline".into(),
    ));
  }
  let actual = model
    .max_sequence_length()
    .map_err(backend_error)?
    .and_then(|size| u32::try_from(size).ok())
    .unwrap_or(context_window);
  if id.is_empty() || context_window == 0 || context_window > actual {
    return Err(InferenceError::InvalidRequest(
      "invalid model identity or configured context window".into(),
    ));
  }
  let info = ModelInfo {
    id,
    backend: "mistral.rs".into(),
    context_window,
    supports_tools,
    supports_images,
  };
  Ok((model, info))
}
fn backend_error(error: impl std::fmt::Display) -> InferenceError {
  InferenceError::Backend(error.to_string())
}
