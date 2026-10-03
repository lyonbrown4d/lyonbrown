use agent_core::{MAX_MODEL_ID_BYTES, MAX_MODEL_PROVIDER_BYTES};

use super::{ConfigLoadError, ModelConfig};

pub(super) fn validate_model_route(model: &ModelConfig) -> Result<(), ConfigLoadError> {
  if model.provider.trim().is_empty() {
    return Err(ConfigLoadError::Validation(
      "model.provider must not be empty".into(),
    ));
  }
  if model.provider.len() > MAX_MODEL_PROVIDER_BYTES {
    return Err(ConfigLoadError::Validation(format!(
      "model.provider must not exceed {MAX_MODEL_PROVIDER_BYTES} bytes"
    )));
  }
  if let Some(model) = &model.model
    && (model.trim().is_empty() || model.len() > MAX_MODEL_ID_BYTES)
  {
    return Err(ConfigLoadError::Validation(format!(
      "model.model must contain 1 to {MAX_MODEL_ID_BYTES} bytes"
    )));
  }
  Ok(())
}
