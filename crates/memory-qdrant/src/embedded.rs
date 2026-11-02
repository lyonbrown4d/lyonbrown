use crate::payload::{self, TIMEOUT, VECTOR};
use memory_core::{
  EmbeddingDescriptor, IndexError, IndexResult, IndexedMemory, VectorHit, VectorIndex, VectorQuery,
  VectorStatus,
};
use qdrant_edge::*;
use std::{fs::File, path::Path, sync::Arc};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

struct OwnedShard {
  shard: Option<EdgeShard>,
  lock: Option<File>,
}
impl Drop for OwnedShard {
  fn drop(&mut self) {
    let shard = self.shard.take();
    let lock = self.lock.take();
    // Edge's destructor flushes synchronously. Keep it off the async worker and
    // retain the native ownership lock until the final flush has completed.
    let close = move || {
      drop(shard);
      drop(lock);
    };
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
      runtime.spawn_blocking(close);
    } else {
      // Outside a runtime there is no async worker to block. Thread creation
      // failure drops the captured owner on this synchronous caller.
      let _ = std::thread::Builder::new()
        .name("memory-index-close".into())
        .spawn(close);
    }
  }
}

pub struct QdrantEmbeddedIndex {
  shard: Arc<OwnedShard>,
  admission: Arc<Semaphore>,
  descriptor: EmbeddingDescriptor,
}

impl QdrantEmbeddedIndex {
  pub async fn open(path: &Path, descriptor: EmbeddingDescriptor) -> IndexResult<Self> {
    payload::descriptor(&descriptor)?;
    let path = path.to_path_buf();
    let expected = descriptor.clone();
    let task = tokio::task::spawn_blocking(move || {
      let (shard, lock) = crate::embedded_init::open(&path, &expected)?;
      Ok(Arc::new(OwnedShard {
        shard: Some(shard),
        lock: Some(lock),
      }))
    });
    let shard = tokio::time::timeout(TIMEOUT, task)
      .await
      .map_err(|_| IndexError::Timeout)?
      .map_err(|_| IndexError::Unavailable)??;
    Ok(Self {
      shard,
      admission: Arc::new(Semaphore::new(1)),
      descriptor,
    })
  }

  async fn run<T: Send + 'static>(
    &self,
    cancel: CancellationToken,
    mutation: bool,
    operation: impl FnOnce(&EdgeShard) -> IndexResult<T> + Send + 'static,
  ) -> IndexResult<T> {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let permit = tokio::select! {
      biased;
      _ = cancel.cancelled() => return Err(IndexError::Cancelled),
      result = tokio::time::timeout_at(deadline, self.admission.clone().acquire_owned()) =>
        result.map_err(|_| IndexError::Timeout)?.map_err(|_| IndexError::Unavailable)?,
    };
    if cancel.is_cancelled() {
      return Err(IndexError::Cancelled);
    }
    let shard = self.shard.clone();
    let task = tokio::task::spawn_blocking(move || {
      let _permit = permit;
      operation(shard.shard.as_ref().ok_or(IndexError::Unavailable)?)
    });
    tokio::select! {
      biased;
      result = task => result.map_err(|_| if mutation { IndexError::Uncertain } else { IndexError::Unavailable })?,
      _ = cancel.cancelled() => Err(if mutation { IndexError::Uncertain } else { IndexError::Cancelled }),
      _ = tokio::time::sleep_until(deadline) => Err(if mutation { IndexError::Uncertain } else { IndexError::Timeout }),
    }
  }
}

fn filter(query: &VectorQuery) -> IndexResult<Filter> {
  let mut must = payload::fields(&query.embedding)
    .into_iter()
    .map(|(key, value)| serde_json::json!({"key": key, "match": {"value": value}}))
    .collect::<Vec<_>>();
  let scopes = query
    .scopes
    .iter()
    .map(|scope| {
      let (kind, id) = payload::scope(scope);
      serde_json::json!({"must": [
        {"key": "scope_kind", "match": {"value": kind}},
        {"key": "scope_id", "match": {"value": id}}
      ]})
    })
    .collect::<Vec<_>>();
  must.push(serde_json::json!({"should": scopes}));
  serde_json::from_value(serde_json::json!({"must": must})).map_err(|_| IndexError::Invalid)
}

#[async_trait::async_trait]
impl VectorIndex for QdrantEmbeddedIndex {
  async fn upsert(&self, values: Vec<IndexedMemory>, cancel: CancellationToken) -> IndexResult<()> {
    payload::points(&values, &self.descriptor)?;
    let points = values
      .iter()
      .map(|value| {
        let payload =
          serde_json::to_value(payload::payload(value)).map_err(|_| IndexError::Invalid)?;
        Ok(
          PointStruct::new(
            PointId::Uuid(payload::point_id(value)?),
            Vectors::new_named([(VECTOR, value.vector.clone())]),
            payload,
          )
          .into(),
        )
      })
      .collect::<IndexResult<Vec<PointStructPersisted>>>()?;
    self
      .run(cancel, true, move |shard| {
        shard
          .update(UpdateOperation::PointOperation(
            PointOperations::UpsertPoints(PointInsertOperations::PointsList(points)),
          ))
          .map_err(|_| IndexError::Uncertain)?;
        shard.flush().map_err(|_| IndexError::Uncertain)
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
    let hi = (through_revision >> 32) as u32;
    let lo = through_revision as u32;
    let filter: Filter = serde_json::from_value(serde_json::json!({"must": [
      {"key": "memory_id", "match": {"value": id}},
      {"key": "content_revision", "range": {"lte": payload::revision_upper_bound(through_revision)}},
      {"should": [
        {"key": "content_revision_hi", "range": {"lt": hi}},
        {"must": [
          {"key": "content_revision_hi", "match": {"value": hi}},
          {"key": "content_revision_lo", "range": {"lte": lo}}
        ]}
      ]}
    ]}))
    .map_err(|_| IndexError::Invalid)?;
    self
      .run(cancel, true, move |shard| {
        shard
          .update(UpdateOperation::PointOperation(
            PointOperations::DeletePointsByFilter(filter),
          ))
          .map_err(|_| IndexError::Uncertain)?;
        shard.flush().map_err(|_| IndexError::Uncertain)
      })
      .await
  }

  async fn search(
    &self,
    query: VectorQuery,
    cancel: CancellationToken,
  ) -> IndexResult<Vec<VectorHit>> {
    payload::query(&query, &self.descriptor)?;
    let filter = filter(&query)?;
    self
      .run(cancel, false, move |shard| {
        let results = shard
          .query(
            QueryRequestBuilder::new(query.limit as usize)
              .query(ScoringQuery::Vector(QueryEnum::Nearest(NamedQuery {
                query: query.vector.into(),
                using: Some(VECTOR.to_owned()),
              })))
              .filter(filter)
              .with_payload(WithPayloadInterface::Bool(true))
              .build(),
          )
          .map_err(|_| IndexError::Unavailable)?;
        results
          .into_iter()
          .map(|point| {
            payload::hit(
              serde_json::to_value(point.payload).map_err(|_| IndexError::Unavailable)?,
              point.score,
            )
          })
          .collect()
      })
      .await
  }

  async fn status(&self) -> IndexResult<VectorStatus> {
    self
      .run(CancellationToken::new(), false, |shard| {
        shard.info().map_err(|_| IndexError::Unavailable)?;
        Ok(VectorStatus {
          ready: true,
          backend: "qdrant-edge-0.9.0".into(),
          detail: "Embedded cosine index; synchronous optimize and durable flush available".into(),
        })
      })
      .await
  }

  async fn maintain(&self, cancel: CancellationToken) -> IndexResult<()> {
    self
      .run(cancel, true, |shard| {
        shard.optimize().map_err(|_| IndexError::Uncertain)?;
        shard.flush().map_err(|_| IndexError::Uncertain)
      })
      .await
  }
}
