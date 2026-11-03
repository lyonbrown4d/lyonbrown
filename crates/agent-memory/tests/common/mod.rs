#![allow(dead_code)]
use memory_core::*;
use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;
pub fn descriptor() -> EmbeddingDescriptor {
  EmbeddingDescriptor {
    id: "fixture".into(),
    model: "fixture-model".into(),
    dimensions: 2,
    configuration_revision: "one".into(),
  }
}
pub fn item(scope: MemoryScope) -> MemoryItem {
  MemoryItem {
    id: uuid::Uuid::new_v4().to_string(),
    scope,
    kind: MemoryKind::Fact,
    state: MemoryState::Active,
    content: "fixture fact".into(),
    revision: 1,
    created_at_ms: 0,
    updated_at_ms: 0,
    expires_at_ms: None,
  }
}
pub fn mutation(item: MemoryItem) -> MemoryMutation {
  MemoryMutation {
    expected_revision: item.revision - 1,
    item,
    source: MemorySource::Explicit {
      request_id: uuid::Uuid::new_v4().to_string(),
    },
  }
}
#[derive(Default)]
pub struct Repository {
  pub items: Mutex<HashMap<String, MemoryItem>>,
  pub job: Mutex<Option<MemoryJob>>,
  pub saved: Mutex<Vec<MemoryMutation>>,
  pub finishes: Mutex<Vec<(u32, MemoryJobState)>>,
}
impl Repository {
  pub fn put(&self, item: MemoryItem) {
    self
      .items
      .lock()
      .expect("items")
      .insert(item.id.clone(), item);
  }
  pub fn queue(&self, item: &MemoryItem, kind: MemoryJobKind) {
    *self.job.lock().expect("job") = Some(MemoryJob {
      id: uuid::Uuid::new_v4().to_string(),
      memory_id: item.id.clone(),
      revision: item.revision,
      kind,
      state: MemoryJobState::Pending,
      attempts: 0,
      error_category: None,
    });
  }
}
#[async_trait::async_trait]
impl MemoryRepository for Repository {
  async fn list(
    &self,
    scopes: &[MemoryScope],
    states: &[MemoryState],
    after: Option<&str>,
    limit: u32,
  ) -> MemoryResult<MemoryPage> {
    let mut items = self
      .items
      .lock()
      .expect("items")
      .values()
      .filter(|item| {
        scopes.contains(&item.scope)
          && states.contains(&item.state)
          && after.is_none_or(|id| item.id.as_str() > id)
      })
      .cloned()
      .collect::<Vec<_>>();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items.truncate(limit as usize);
    Ok(MemoryPage {
      items,
      next_cursor: None,
    })
  }
  async fn get(&self, id: &str) -> MemoryResult<Option<MemoryItem>> {
    Ok(self.items.lock().expect("items").get(id).cloned())
  }
  async fn save(&self, mutation: MemoryMutation) -> MemoryResult<MemoryItem> {
    self.put(mutation.item.clone());
    self.saved.lock().expect("saved").push(mutation.clone());
    Ok(mutation.item)
  }
  async fn forget(&self, id: &str, expected: u64) -> MemoryResult<MemoryItem> {
    let mut all = self.items.lock().expect("items");
    let item = all.get_mut(id).ok_or(MemoryError::NotFound)?;
    if item.revision != expected {
      return Err(MemoryError::Conflict);
    }
    item.revision += 1;
    item.content.clear();
    item.state = MemoryState::Deleted;
    Ok(item.clone())
  }
  async fn claim_job(&self) -> MemoryResult<Option<MemoryJob>> {
    let mut lock = self.job.lock().expect("job");
    let Some(job) = lock
      .as_mut()
      .filter(|job| job.state == MemoryJobState::Pending)
    else {
      return Ok(None);
    };
    job.state = MemoryJobState::Running;
    job.attempts += 1;
    Ok(Some(job.clone()))
  }
  async fn claim_job_for(&self, _: &EmbeddingDescriptor) -> MemoryResult<Option<MemoryJob>> {
    self.claim_job().await
  }
  async fn finish_job(
    &self,
    id: &str,
    attempt: u32,
    state: MemoryJobState,
    error: Option<&str>,
  ) -> MemoryResult<()> {
    let mut lock = self.job.lock().expect("job");
    let job = lock.as_mut().ok_or(MemoryError::NotFound)?;
    if job.id != id || job.attempts != attempt || job.state != MemoryJobState::Running {
      return Err(MemoryError::Conflict);
    }
    job.state = state;
    job.error_category = error.map(str::to_owned);
    self
      .finishes
      .lock()
      .expect("finishes")
      .push((attempt, state));
    Ok(())
  }
  async fn retry_job(&self, id: &str) -> MemoryResult<()> {
    let mut lock = self.job.lock().expect("job");
    let job = lock.as_mut().ok_or(MemoryError::NotFound)?;
    if job.id != id
      || !matches!(
        job.state,
        MemoryJobState::Failed | MemoryJobState::Uncertain | MemoryJobState::Pending
      )
    {
      return Err(MemoryError::Conflict);
    }
    job.state = MemoryJobState::Pending;
    Ok(())
  }
  async fn retry_job_for(&self, id: &str, _: Option<&EmbeddingDescriptor>) -> MemoryResult<()> {
    self.retry_job(id).await
  }
  async fn jobs(&self, id: &str) -> MemoryResult<Vec<MemoryJob>> {
    Ok(
      self
        .job
        .lock()
        .expect("job")
        .iter()
        .filter(|job| job.memory_id == id)
        .cloned()
        .collect(),
    )
  }
  async fn recover_jobs(&self) -> MemoryResult<u64> {
    let mut lock = self.job.lock().expect("job");
    if let Some(job) = lock
      .as_mut()
      .filter(|job| job.state == MemoryJobState::Running)
    {
      job.state = MemoryJobState::Uncertain;
      Ok(1)
    } else {
      Ok(0)
    }
  }
  async fn project_scope(&self, _: &str, _: &str) -> MemoryResult<MemoryScope> {
    Err(MemoryError::Forbidden)
  }
  async fn start_rebuild(
    &self,
    _: &str,
    _: &[MemoryScope],
    _: &EmbeddingDescriptor,
    _: bool,
    _: i64,
  ) -> MemoryResult<MemoryRebuild> {
    Err(MemoryError::Storage)
  }
  async fn list_rebuilds(
    &self,
    _: &[MemoryScope],
    _: Option<&str>,
    _: u32,
  ) -> MemoryResult<MemoryRebuildPage> {
    Err(MemoryError::Storage)
  }
  async fn advance_rebuild(&self, _: &EmbeddingDescriptor, _: u32, _: i64) -> MemoryResult<bool> {
    Ok(false)
  }
  async fn bind_run(&self, _: &str, _: &str, _: u64, _: &[MemoryItem]) -> MemoryResult<()> {
    Ok(())
  }
}
pub struct Embedding {
  pub cancel: bool,
  pub revise: Option<Arc<Repository>>,
}
#[async_trait::async_trait]
impl MemoryEmbedding for Embedding {
  fn descriptor(&self) -> EmbeddingDescriptor {
    descriptor()
  }
  async fn embed(&self, _: &str, _: CancellationToken) -> MemoryResult<Vec<f32>> {
    if self.cancel {
      return Err(MemoryError::Cancelled);
    }
    if let Some(repo) = &self.revise {
      for item in repo.items.lock().expect("items").values_mut() {
        item.revision += 1;
        item.state = MemoryState::Deleted;
        item.content.clear();
      }
    }
    Ok(vec![0.25, 0.75])
  }
}
#[derive(Default)]
pub struct Index {
  pub hits: Mutex<Vec<VectorHit>>,
  pub queries: Mutex<Vec<VectorQuery>>,
  pub upserts: Mutex<Vec<IndexedMemory>>,
  pub deletes: Mutex<Vec<(String, u64)>>,
  pub uncertain: bool,
  pub late_attempt: Option<Arc<Repository>>,
}
#[async_trait::async_trait]
impl VectorIndex for Index {
  async fn upsert(&self, points: Vec<IndexedMemory>, _: CancellationToken) -> IndexResult<()> {
    self.upserts.lock().expect("upserts").extend(points);
    if let Some(repo) = &self.late_attempt {
      repo
        .job
        .lock()
        .expect("job")
        .as_mut()
        .expect("claimed")
        .attempts += 1;
    }
    if self.uncertain {
      Err(IndexError::Uncertain)
    } else {
      Ok(())
    }
  }
  async fn delete(&self, id: &str, through: u64, _: CancellationToken) -> IndexResult<()> {
    self
      .deletes
      .lock()
      .expect("deletes")
      .push((id.into(), through));
    Ok(())
  }
  async fn search(&self, query: VectorQuery, _: CancellationToken) -> IndexResult<Vec<VectorHit>> {
    self.queries.lock().expect("queries").push(query);
    Ok(self.hits.lock().expect("hits").clone())
  }
  async fn status(&self) -> IndexResult<VectorStatus> {
    Ok(VectorStatus {
      ready: true,
      backend: "fixture".into(),
      detail: "raw remote diagnostic must not escape".into(),
    })
  }
  async fn maintain(&self, _: CancellationToken) -> IndexResult<()> {
    Ok(())
  }
}
