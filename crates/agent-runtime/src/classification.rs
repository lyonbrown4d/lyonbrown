//! One bounded inference request without a Harness or session mutation.
use crate::{Result, RuntimeConfig, RuntimeError};
pub use model_provider::CLASSIFICATION_OUTPUT_LIMIT;
use tokio_util::sync::CancellationToken;

pub async fn classify_json(
  config: &RuntimeConfig,
  prompt: &str,
  cancellation: CancellationToken,
) -> Result<String> {
  if config.session_id.trim().is_empty() || config.owner_epoch == 0 {
    return Err(RuntimeError::Configuration(
      "session and a positive owner epoch are required".into(),
    ));
  }
  model_provider::classify_json(&crate::provider::model_config(config), prompt, cancellation)
    .await
    .map_err(|error| RuntimeError::Harness(error.to_string()))
}
