//! Shared model protocol adapters. No Agent Loop, session or storage ownership.
#![forbid(unsafe_code)]
mod classification;
mod configuration;
mod local;
pub use classification::{CLASSIFICATION_OUTPUT_LIMIT, classify_json};
pub use configuration::configured;
pub use local::classify_local_json;

/// Credentials are intentionally neither serializable nor Debug.
#[derive(Clone)]
pub struct ModelConfig {
  pub provider: String,
  pub model: String,
  pub api_key: String,
  pub base_url: Option<String>,
  pub system_prompt: Option<String>,
  pub context_window: u32,
  pub max_output_tokens: u32,
  pub supports_images: bool,
}
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
  #[error("provider configuration: {0}")]
  Configuration(String),
  #[error("model request: {0}")]
  Request(String),
}
pub type Result<T> = std::result::Result<T, ProviderError>;
