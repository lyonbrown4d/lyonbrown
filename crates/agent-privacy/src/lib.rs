//! Host-owned privacy transforms. Placeholders are references, not authorization.
//! Callers must select content fields and enforce authorization before resolving.
//! Mappings are memory-only and disappear when this service is dropped.

mod detector;
mod known_secrets;
mod placeholder;
mod service;

pub use detector::{Detection, Detector, Evidence, Kind, RuleDetector};
pub use service::PrivacyService;

/// Failures are intentionally free of input text and secret values.
#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
  #[error("privacy input or detector output exceeds a safety limit")]
  Limit,
  #[error("invalid persisted privacy mapping")]
  InvalidMapping,
  #[error("invalid detector span")]
  InvalidSpan,
  #[error("privacy detector initialization failed")]
  DetectorInitialization,
  #[error("secure random generation failed")]
  Random,
  #[error("placeholder is unavailable in this scope")]
  MissingMapping,
}
