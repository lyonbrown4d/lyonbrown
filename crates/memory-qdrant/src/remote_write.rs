//! The high-level Qdrant point API retries unavailable writes automatically.
//! Use its public generated gRPC API so the caller owns uncertain outcomes.
use crate::payload::TIMEOUT;
use memory_core::{IndexError, IndexResult};
use qdrant_client::qdrant::{
  DeletePoints, PointsOperationResponse, UpsertPoints, points_client::PointsClient,
};
use secrecy::{ExposeSecret, SecretString};
use tonic::{
  Request,
  metadata::{Ascii, MetadataValue},
  transport::{Channel, ClientTlsConfig, Endpoint},
};

pub(crate) struct RemoteWrite {
  client: PointsClient<Channel>,
  key: Option<MetadataValue<Ascii>>,
}
impl RemoteWrite {
  pub(crate) fn new(endpoint: &str, key: Option<&SecretString>) -> IndexResult<Self> {
    let key = key
      .map(|key| -> IndexResult<MetadataValue<Ascii>> {
        let mut value: MetadataValue<Ascii> = key
          .expose_secret()
          .parse()
          .map_err(|_| IndexError::Invalid)?;
        value.set_sensitive(true);
        Ok(value)
      })
      .transpose()?;
    let mut channel = Endpoint::from_shared(endpoint.to_owned())
      .map_err(|_| IndexError::Invalid)?
      .timeout(TIMEOUT)
      .connect_timeout(TIMEOUT);
    if endpoint.starts_with("https://") {
      channel = channel
        .tls_config(ClientTlsConfig::new().with_native_roots())
        .map_err(|_| IndexError::Invalid)?;
    } else if !endpoint.starts_with("http://") {
      return Err(IndexError::Invalid);
    }
    Ok(Self {
      client: PointsClient::new(channel.connect_lazy()),
      key,
    })
  }

  fn request<T>(&self, message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.set_timeout(TIMEOUT);
    if let Some(key) = &self.key {
      request.metadata_mut().insert("api-key", key.clone());
    }
    request
  }

  pub(crate) async fn upsert(&self, request: UpsertPoints) -> IndexResult<PointsOperationResponse> {
    self
      .client
      .clone()
      .upsert(self.request(request))
      .await
      .map(|result| result.into_inner())
      .map_err(|_| IndexError::Uncertain)
  }

  pub(crate) async fn delete(&self, request: DeletePoints) -> IndexResult<PointsOperationResponse> {
    self
      .client
      .clone()
      .delete(self.request(request))
      .await
      .map(|result| result.into_inner())
      .map_err(|_| IndexError::Uncertain)
  }
}
