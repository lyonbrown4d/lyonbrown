//! Framework embedding wire behind the memory domain port.
use crate::{valid_vector, validate_descriptor};
use memory_core::{EmbeddingDescriptor, MemoryEmbedding, MemoryError, MemoryResult};
use rig_core::{
  DynModel,
  operation::Embedding,
  providers::openai::{OpenAIConfig, wire::Auth},
};
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;

pub struct RigEmbedding {
  model: DynModel<Embedding>,
  descriptor: EmbeddingDescriptor,
}
impl RigEmbedding {
  /// SDK access is confined to the daemon composition and vector adapter.
  pub fn model(&self) -> DynModel<Embedding> {
    self.model.clone()
  }
  pub fn new(
    endpoint: &str,
    api_key: Option<SecretString>,
    descriptor: EmbeddingDescriptor,
  ) -> MemoryResult<Self> {
    validate_descriptor(&descriptor)?;
    let mut url = reqwest::Url::parse(endpoint).map_err(|_| MemoryError::Invalid)?;
    // Configuration accepts a provider base or an explicit embedding endpoint.
    // Rig owns request encoding and decoding; normalize only the base path.
    let path = url.path().trim_end_matches('/');
    let base = if path.is_empty() {
      "/v1"
    } else {
      path.strip_suffix("/embeddings").unwrap_or(path)
    }
    .to_owned();
    url.set_path(&base);
    let key = api_key
      .as_ref()
      .map(ExposeSecret::expose_secret)
      .unwrap_or("");
    if key.len() > 8192 || key.chars().any(char::is_control) || api_key.is_some() && key.is_empty()
    {
      return Err(MemoryError::Invalid);
    }
    let http =
      model_provider::configured_http(url.as_str(), std::time::Duration::from_secs(5), 1_048_576)
        .map_err(|_| MemoryError::Invalid)?;
    let mut provider = OpenAIConfig::new(key.to_owned()).with_base_url(url.as_str());
    provider.auth = Auth::OptionalBearer;
    let model = provider
      .connect(http)
      .embedding(
        descriptor.model.clone(),
        Some(descriptor.dimensions as usize),
      )
      .erase();
    Ok(Self { model, descriptor })
  }
  async fn request(&self, text: &str) -> MemoryResult<Vec<f32>> {
    // Call the batch operation to retain exact cardinality validation instead
    // of a convenience method that returns only the final vector.
    let response = self
      .model
      .call(vec![text.to_owned()])
      .await
      .map_err(|_| MemoryError::EmbeddingUnavailable)?;
    if response.embeddings.len() != 1 {
      return Err(MemoryError::EmbeddingUnavailable);
    }
    let embedding = response
      .embeddings
      .into_iter()
      .next()
      .ok_or(MemoryError::EmbeddingUnavailable)?;
    let vector = embedding
      .vec
      .into_iter()
      .map(|value| value as f32)
      .collect::<Vec<_>>();
    if !valid_vector(&vector, &self.descriptor) {
      return Err(MemoryError::EmbeddingUnavailable);
    }
    Ok(vector)
  }
}
#[async_trait::async_trait]
impl MemoryEmbedding for RigEmbedding {
  fn descriptor(&self) -> EmbeddingDescriptor {
    self.descriptor.clone()
  }
  async fn embed(&self, text: &str, cancel: CancellationToken) -> MemoryResult<Vec<f32>> {
    if text.trim().is_empty() || text.len() > 16_384 {
      return Err(MemoryError::Invalid);
    }
    tokio::select! { biased; _=cancel.cancelled()=>Err(MemoryError::Cancelled), result=self.request(text)=>result }
  }
}
