use decision_core::BackendError;

/// Identifies the complete embedding/preprocessing contract, independently of a runtime library.
#[derive(Clone, Debug)]
pub struct EmbeddingDescriptor {
  pub model_revision: String,
  pub preprocessing_revision: String,
  pub dimensions: usize,
  /// Remote providers must report false, so local-only requests never disclose text.
  pub local: bool,
}

/// Synchronous inference port. The backend invokes this only in bounded blocking workers.
/// Implementations must not log input, and must report their actual data boundary.
pub trait EmbeddingProvider: Send + Sync {
  fn descriptor(&self) -> EmbeddingDescriptor;
  fn embed(&self, text: &str) -> Result<Vec<f32>, BackendError>;
}

pub(crate) fn normalize(vector: Vec<f32>, dimensions: usize) -> Result<Vec<f32>, BackendError> {
  if vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()) {
    return Err(BackendError::InvalidInput);
  }
  let norm = vector
    .iter()
    .map(|&value| f64::from(value).powi(2))
    .sum::<f64>()
    .sqrt();
  if !norm.is_finite() || norm == 0.0 {
    return Err(BackendError::InvalidInput);
  }
  Ok(
    vector
      .into_iter()
      .map(|value| (f64::from(value) / norm) as f32)
      .collect(),
  )
}

pub(crate) fn similarity(left: &[f32], right: &[f32]) -> f64 {
  #[cfg(feature = "semantic-local")]
  {
    f64::from(fastembed::similarity::cosine_similarity(left, right)).clamp(-1.0, 1.0)
  }
  #[cfg(not(feature = "semantic-local"))]
  {
    left
      .iter()
      .zip(right)
      .map(|(&a, &b)| f64::from(a) * f64::from(b))
      .sum::<f64>()
      .clamp(-1.0, 1.0)
  }
}
