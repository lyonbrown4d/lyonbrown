use crate::{
  GenerationRequest, GenerationSummary, InferenceBackend, InferenceChunk, InferenceError,
  ModelInfo, Result,
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{RwLock, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
struct Loaded {
  backend: Arc<dyn InferenceBackend>,
  permits: Arc<Semaphore>,
}
pub struct ModelRegistry {
  models: RwLock<BTreeMap<String, Arc<Loaded>>>,
  permits: Arc<Semaphore>,
}
struct ManagedBackend {
  registry: Arc<ModelRegistry>,
  id: String,
  info: ModelInfo,
}

#[async_trait::async_trait]
impl InferenceBackend for ManagedBackend {
  fn info(&self) -> ModelInfo {
    self.info.clone()
  }
  async fn generate(
    &self,
    request: GenerationRequest,
    cancellation: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> Result<GenerationSummary> {
    self
      .registry
      .generate(&self.id, request, cancellation, output)
      .await
  }
}
impl ModelRegistry {
  pub fn new(max_parallel_requests: usize) -> Result<Self> {
    if max_parallel_requests == 0 || max_parallel_requests > Semaphore::MAX_PERMITS {
      return Err(InferenceError::InvalidRequest(
        "request concurrency must be positive".into(),
      ));
    }
    Ok(Self {
      models: RwLock::new(BTreeMap::new()),
      permits: Arc::new(Semaphore::new(max_parallel_requests)),
    })
  }
  /// Register only an already loaded backend. Build flags never imply availability.
  pub async fn register(
    &self,
    backend: Arc<dyn InferenceBackend>,
    parallel_requests: usize,
  ) -> Result<()> {
    let info = backend.info();
    if info.id.is_empty()
      || info.context_window == 0
      || parallel_requests == 0
      || parallel_requests > Semaphore::MAX_PERMITS
    {
      return Err(InferenceError::InvalidRequest(
        "invalid loaded model metadata or concurrency".into(),
      ));
    }
    let mut models = self.models.write().await;
    if models.contains_key(&info.id) {
      return Err(InferenceError::InvalidRequest(
        "model id already loaded".into(),
      ));
    }
    models.insert(
      info.id,
      Arc::new(Loaded {
        backend,
        permits: Arc::new(Semaphore::new(parallel_requests)),
      }),
    );
    Ok(())
  }
  pub async fn remove(&self, id: &str) -> bool {
    self.models.write().await.remove(id).is_some()
  }
  pub async fn models(&self) -> Vec<ModelInfo> {
    self
      .models
      .read()
      .await
      .values()
      .map(|loaded| loaded.backend.info())
      .collect()
  }
  pub async fn backend(self: &Arc<Self>, id: &str) -> Result<Arc<dyn InferenceBackend>> {
    let info = self
      .models
      .read()
      .await
      .get(id)
      .map(|loaded| loaded.backend.info())
      .ok_or_else(|| InferenceError::ModelUnavailable(id.into()))?;
    Ok(Arc::new(ManagedBackend {
      registry: self.clone(),
      id: id.into(),
      info,
    }))
  }
  pub async fn generate(
    &self,
    id: &str,
    request: GenerationRequest,
    cancellation: CancellationToken,
    output: mpsc::Sender<InferenceChunk>,
  ) -> Result<GenerationSummary> {
    let loaded = self
      .models
      .read()
      .await
      .get(id)
      .cloned()
      .ok_or_else(|| InferenceError::ModelUnavailable(id.into()))?;
    crate::contracts::validate(&loaded.backend.info(), &request)?;
    let _local = tokio::select! {biased;_=cancellation.cancelled()=>return Err(InferenceError::Cancelled),permit=loaded.permits.clone().acquire_owned()=>permit.map_err(|error|InferenceError::Backend(error.to_string()))?};
    let _global = tokio::select! {biased;_=cancellation.cancelled()=>return Err(InferenceError::Cancelled),permit=self.permits.clone().acquire_owned()=>permit.map_err(|error|InferenceError::Backend(error.to_string()))?};
    loaded.backend.generate(request, cancellation, output).await
  }
}
