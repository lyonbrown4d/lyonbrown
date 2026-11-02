use crate::payload::{self, TIMEOUT, VECTOR};
use memory_core::{
  EmbeddingDescriptor, IndexError, IndexResult, IndexedMemory, VectorHit, VectorIndex, VectorQuery,
  VectorStatus,
};
use qdrant_client::{Payload, Qdrant, qdrant::*};
use secrecy::{ExposeSecret, SecretString};
use std::{collections::HashMap, future::Future, sync::Arc};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub struct QdrantRemoteIndex {
  client: Qdrant,
  writes: crate::remote_write::RemoteWrite,
  collection: String,
  descriptor: EmbeddingDescriptor,
  admission: Arc<Semaphore>,
}

impl QdrantRemoteIndex {
  /// Connects to Qdrant Server 1.20.x over gRPC. Existing collections must use
  /// the named `memory` vector with the requested dimensions and cosine metric.
  pub async fn connect(
    endpoint: &str,
    api_key: Option<SecretString>,
    collection: &str,
    descriptor: EmbeddingDescriptor,
  ) -> IndexResult<Self> {
    payload::descriptor(&descriptor)?;
    if !payload::text(collection)
      || collection.len() > 128
      || !collection
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
      return Err(IndexError::Invalid);
    }
    let writes = crate::remote_write::RemoteWrite::new(endpoint, api_key.as_ref())?;
    let client = Qdrant::from_url(endpoint)
      // The SDK's default compatibility check joins a synchronous helper
      // runtime. Perform the strict check asynchronously below instead.
      .skip_compatibility_check()
      .api_key(api_key.as_ref().map(|key| key.expose_secret().to_owned()))
      .timeout(TIMEOUT)
      .build()
      .map_err(|_| IndexError::Invalid)?;
    let index = Self {
      client,
      writes,
      collection: collection.into(),
      descriptor,
      admission: Arc::new(Semaphore::new(1)),
    };
    index
      .run(CancellationToken::new(), true, async {
        let health = index
          .client
          .health_check()
          .await
          .map_err(|_| IndexError::Unavailable)?;
        if !health.version.starts_with("1.20.") {
          return Err(IndexError::Unsupported);
        }
        if !index
          .client
          .collection_exists(&index.collection)
          .await
          .map_err(|_| IndexError::Unavailable)?
        {
          let mut vectors = VectorsConfigBuilder::default();
          vectors.add_named_vector_params(
            VECTOR,
            VectorParamsBuilder::new(index.descriptor.dimensions as u64, Distance::Cosine),
          );
          index
            .client
            .create_collection(
              CreateCollectionBuilder::new(&index.collection).vectors_config(vectors),
            )
            .await
            .map_err(|_| IndexError::Uncertain)?;
        }
        index.verify_collection().await
      })
      .await?;
    Ok(index)
  }

  async fn verify_collection(&self) -> IndexResult<()> {
    let response = self
      .client
      .collection_info(&self.collection)
      .await
      .map_err(|_| IndexError::Unavailable)?;
    let vectors = response
      .result
      .and_then(|r| r.config)
      .and_then(|c| c.params)
      .and_then(|p| p.vectors_config)
      .and_then(|v| v.config)
      .ok_or(IndexError::Unsupported)?;
    let params = match vectors {
      vectors_config::Config::ParamsMap(params) => params
        .map
        .get(VECTOR)
        .cloned()
        .ok_or(IndexError::Unsupported)?,
      _ => return Err(IndexError::Unsupported),
    };
    if params.size != self.descriptor.dimensions as u64
      || params.distance != Distance::Cosine as i32
    {
      return Err(IndexError::Unsupported);
    }
    Ok(())
  }

  async fn run<T>(
    &self,
    cancel: CancellationToken,
    mutation: bool,
    operation: impl Future<Output = IndexResult<T>>,
  ) -> IndexResult<T> {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let _permit = tokio::select! {
      biased;
      _ = cancel.cancelled() => return Err(IndexError::Cancelled),
      result = tokio::time::timeout_at(deadline, self.admission.acquire()) =>
        result.map_err(|_| IndexError::Timeout)?.map_err(|_| IndexError::Unavailable)?,
    };
    if cancel.is_cancelled() {
      return Err(IndexError::Cancelled);
    }
    tokio::select! {
      biased;
      _ = cancel.cancelled() => Err(if mutation { IndexError::Uncertain } else { IndexError::Cancelled }),
      result = operation => result,
      _ = tokio::time::sleep_until(deadline) => Err(if mutation { IndexError::Uncertain } else { IndexError::Timeout }),
    }
  }
}

#[async_trait::async_trait]
impl VectorIndex for QdrantRemoteIndex {
  async fn upsert(&self, values: Vec<IndexedMemory>, cancel: CancellationToken) -> IndexResult<()> {
    payload::points(&values, &self.descriptor)?;
    let points = values
      .iter()
      .map(|value| {
        let payload = Payload::try_from(
          serde_json::to_value(payload::payload(value)).map_err(|_| IndexError::Invalid)?,
        )
        .map_err(|_| IndexError::Invalid)?;
        let vectors: HashMap<String, Vec<f32>> =
          HashMap::from([(VECTOR.into(), value.vector.clone())]);
        Ok(PointStruct::new(
          payload::point_id(value)?.to_string(),
          vectors,
          payload,
        ))
      })
      .collect::<IndexResult<Vec<_>>>()?;
    self
      .run(cancel, true, async {
        let response = self
          .writes
          .upsert(
            UpsertPointsBuilder::new(&self.collection, points)
              .wait(true)
              .build(),
          )
          .await
          .map_err(|_| IndexError::Uncertain)?;
        if response
          .result
          .is_none_or(|r| r.status != UpdateStatus::Completed as i32)
        {
          return Err(IndexError::Uncertain);
        }
        Ok(())
      })
      .await
  }

  async fn delete(
    &self,
    id: &str,
    through_revision: u64,
    cancel: CancellationToken,
  ) -> IndexResult<()> {
    payload::id(id)?;
    if through_revision == 0 || through_revision > i64::MAX as u64 {
      return Err(IndexError::Invalid);
    }
    let filter = crate::remote_filter::deletion(id, through_revision);
    self
      .run(cancel, true, async {
        let response = self
          .writes
          .delete(
            DeletePointsBuilder::new(&self.collection)
              .points(filter)
              .wait(true)
              .build(),
          )
          .await
          .map_err(|_| IndexError::Uncertain)?;
        if response
          .result
          .is_none_or(|r| r.status != UpdateStatus::Completed as i32)
        {
          return Err(IndexError::Uncertain);
        }
        Ok(())
      })
      .await
  }

  async fn search(
    &self,
    query: VectorQuery,
    cancel: CancellationToken,
  ) -> IndexResult<Vec<VectorHit>> {
    payload::query(&query, &self.descriptor)?;
    let filter = crate::remote_filter::filter(&query);
    self
      .run(cancel, false, async {
        let response = self
          .client
          .query(
            QueryPointsBuilder::new(&self.collection)
              .query(query.vector)
              .using(VECTOR)
              .filter(filter)
              .limit(query.limit as u64)
              .with_payload(true),
          )
          .await
          .map_err(|_| IndexError::Unavailable)?;
        response
          .result
          .into_iter()
          .map(|point| {
            let value = serde_json::Value::from(Payload::from(point.payload));
            payload::hit(value, point.score)
          })
          .collect()
      })
      .await
  }

  async fn status(&self) -> IndexResult<VectorStatus> {
    self
      .run(CancellationToken::new(), false, async {
        self.verify_collection().await?;
        Ok(VectorStatus {
          ready: true,
          backend: "qdrant-server-1.20".into(),
          detail: "Server-managed optimization; manual maintenance unsupported".into(),
        })
      })
      .await
  }

  async fn maintain(&self, cancel: CancellationToken) -> IndexResult<()> {
    if cancel.is_cancelled() {
      return Err(IndexError::Cancelled);
    }
    Err(IndexError::Unsupported)
  }
}
