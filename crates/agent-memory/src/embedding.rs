use crate::{valid_vector, validate_descriptor};
use memory_core::{EmbeddingDescriptor, MemoryEmbedding, MemoryError, MemoryResult};
use reqwest::{Client, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// OpenAI-compatible HTTP embeddings; credentials never enter a domain record.
pub struct HttpEmbedding {
  client: Client,
  endpoint: Url,
  descriptor: EmbeddingDescriptor,
}
impl HttpEmbedding {
  pub fn new(
    endpoint: &str,
    api_key: Option<SecretString>,
    descriptor: EmbeddingDescriptor,
  ) -> MemoryResult<Self> {
    validate_descriptor(&descriptor)?;
    let mut endpoint = Url::parse(endpoint).map_err(|_| MemoryError::Invalid)?;
    if !matches!(endpoint.scheme(), "http" | "https")
      || endpoint.host_str().is_none()
      || !endpoint.username().is_empty()
      || endpoint.password().is_some()
      || endpoint.query().is_some()
      || endpoint.fragment().is_some()
    {
      return Err(MemoryError::Invalid);
    }
    let path = endpoint.path().trim_end_matches('/');
    let path = if path.is_empty() {
      "/v1/embeddings".into()
    } else if path.ends_with("/embeddings") {
      path.into()
    } else {
      format!("{path}/embeddings")
    };
    endpoint.set_path(&path);
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(key) = api_key {
      if key.expose_secret().is_empty() || key.expose_secret().len() > 8192 {
        return Err(MemoryError::Invalid);
      }
      let mut value =
        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", key.expose_secret()))
          .map_err(|_| MemoryError::Invalid)?;
      value.set_sensitive(true);
      headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    let client = Client::builder()
      .default_headers(headers)
      .redirect(reqwest::redirect::Policy::none())
      .retry(reqwest::retry::never())
      .timeout(std::time::Duration::from_secs(5))
      .build()
      .map_err(|_| MemoryError::EmbeddingUnavailable)?;
    Ok(Self {
      client,
      endpoint,
      descriptor,
    })
  }
  async fn request(&self, text: &str) -> MemoryResult<Vec<f32>> {
    let request = EmbeddingRequest {
      model: &self.descriptor.model,
      input: text,
      encoding_format: "float",
    };
    let mut response = self
      .client
      .post(self.endpoint.clone())
      .json(&request)
      .send()
      .await
      .map_err(|_| MemoryError::EmbeddingUnavailable)?;
    if !response.status().is_success()
      || response
        .content_length()
        .is_some_and(|length| length > 1_048_576)
    {
      return Err(MemoryError::EmbeddingUnavailable);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
      .chunk()
      .await
      .map_err(|_| MemoryError::EmbeddingUnavailable)?
    {
      if body.len().saturating_add(chunk.len()) > 1_048_576 {
        return Err(MemoryError::EmbeddingUnavailable);
      }
      body.extend_from_slice(&chunk);
    }
    let mut data = serde_json::from_slice::<EmbeddingResponse>(&body)
      .map_err(|_| MemoryError::EmbeddingUnavailable)?
      .data;
    if data.len() != 1 {
      return Err(MemoryError::EmbeddingUnavailable);
    }
    let value = data.pop().ok_or(MemoryError::EmbeddingUnavailable)?;
    if value.index != 0 || !valid_vector(&value.embedding, &self.descriptor) {
      return Err(MemoryError::EmbeddingUnavailable);
    }
    Ok(value.embedding)
  }
}
#[async_trait::async_trait]
impl MemoryEmbedding for HttpEmbedding {
  fn descriptor(&self) -> EmbeddingDescriptor {
    self.descriptor.clone()
  }
  async fn embed(&self, text: &str, cancel: CancellationToken) -> MemoryResult<Vec<f32>> {
    if text.trim().is_empty() || text.len() > 16_384 {
      return Err(MemoryError::Invalid);
    }
    tokio::select! {biased; _=cancel.cancelled()=>Err(MemoryError::Cancelled), result=self.request(text)=>result}
  }
}
#[derive(Serialize)]
struct EmbeddingRequest<'a> {
  model: &'a str,
  input: &'a str,
  encoding_format: &'static str,
}
#[derive(Deserialize)]
struct EmbeddingResponse {
  data: Vec<EmbeddingData>,
}
#[derive(Deserialize)]
struct EmbeddingData {
  index: u32,
  embedding: Vec<f32>,
}
