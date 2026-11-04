use crate::{MemoryService, now_ms, valid_vector};
use memory_core::*;
use tokio_util::sync::CancellationToken;

impl MemoryService {
  /// Claim and execute at most one durable intent. There is no implicit retry.
  pub async fn process_one(&self, cancel: CancellationToken) -> MemoryResult<bool> {
    if cancel.is_cancelled() {
      return Err(MemoryError::Cancelled);
    }
    let Some(index) = &self.index else {
      return Ok(false);
    };
    if let Some(embedding) = &self.embedding
      && self
        .repository
        .advance_rebuild(&embedding.descriptor(), 100, now_ms()?)
        .await?
    {
      return Ok(true);
    }
    let claimed = match &self.embedding {
      Some(embedding) => {
        self
          .repository
          .claim_job_for(&embedding.descriptor())
          .await?
      }
      None => self.repository.claim_job().await?,
    };
    let Some(job) = claimed else {
      return Ok(false);
    };
    let outcome = self.execute_job(&job, index.as_ref(), cancel).await;
    let (state, category) = match &outcome {
      Ok(()) => (MemoryJobState::Completed, None),
      Err(JobFailure::Failed(category)) => (MemoryJobState::Failed, Some(*category)),
      Err(JobFailure::Uncertain(category)) => (MemoryJobState::Uncertain, Some(*category)),
    };
    self
      .repository
      .finish_job(&job.id, job.attempts, state, category)
      .await?;
    match outcome {
      Err(JobFailure::Uncertain("cancelled")) => Err(MemoryError::Cancelled),
      _ => Ok(true),
    }
  }
  async fn execute_job(
    &self,
    job: &MemoryJob,
    index: &dyn VectorIndex,
    cancel: CancellationToken,
  ) -> Result<(), JobFailure> {
    if cancel.is_cancelled() {
      return Err(JobFailure::Uncertain("cancelled"));
    }
    match job.kind {
      MemoryJobKind::Delete => {
        let result = tokio::select! {biased; _=cancel.cancelled()=>return Err(JobFailure::Uncertain("cancelled")), result=index.delete(&job.memory_id,job.revision,cancel.child_token())=>result};
        result.map_err(index_failure)
      }
      MemoryJobKind::Upsert => {
        let current = self
          .repository
          .get(&job.memory_id)
          .await
          .map_err(|_| JobFailure::Failed("storage_unavailable"))?;
        let Some(item) =
          current.filter(|item| item.revision == job.revision && item.state == MemoryState::Active)
        else {
          return Ok(());
        };
        let now = now_ms().map_err(|_| JobFailure::Failed("clock_unavailable"))?;
        if item.expires_at_ms.is_some_and(|expiry| expiry <= now) {
          return Ok(());
        }
        let embedding = self
          .embedding
          .as_ref()
          .ok_or(JobFailure::Failed("embedding_unavailable"))?;
        let result = tokio::select! {biased; _=cancel.cancelled()=>return Err(JobFailure::Uncertain("cancelled")), result=embedding.embed(&item.content,cancel.child_token())=>result};
        let vector = result.map_err(|error| {
          if matches!(error, MemoryError::Cancelled) {
            JobFailure::Uncertain("cancelled")
          } else {
            JobFailure::Failed("embedding_unavailable")
          }
        })?;
        let descriptor = embedding.descriptor();
        if !valid_vector(&vector, &descriptor) {
          return Err(JobFailure::Failed("embedding_invalid"));
        }
        // The revision may have been forgotten or edited while embedding ran.
        let latest = self
          .repository
          .get(&job.memory_id)
          .await
          .map_err(|_| JobFailure::Failed("storage_unavailable"))?;
        let latest_now = now_ms().map_err(|_| JobFailure::Failed("clock_unavailable"))?;
        if !latest.is_some_and(|latest| {
          latest == item
            && latest.state == MemoryState::Active
            && latest
              .expires_at_ms
              .is_none_or(|expiry| expiry > latest_now)
        }) {
          return Ok(());
        }
        let point = IndexedMemory {
          id: item.id,
          revision: item.revision,
          scope: item.scope,
          embedding: descriptor,
          vector,
        };
        let result = tokio::select! {biased; _=cancel.cancelled()=>return Err(JobFailure::Uncertain("cancelled")), result=index.upsert(vec![point],cancel.child_token())=>result};
        result.map_err(index_failure)
      }
    }
  }
}
enum JobFailure {
  Failed(&'static str),
  Uncertain(&'static str),
}
fn index_failure(error: IndexError) -> JobFailure {
  match error {
    IndexError::Invalid => JobFailure::Failed("index_invalid"),
    IndexError::Unsupported => JobFailure::Failed("index_unsupported"),
    IndexError::Cancelled => JobFailure::Uncertain("cancelled"),
    IndexError::Timeout => JobFailure::Uncertain("index_timeout"),
    IndexError::Uncertain | IndexError::Unavailable => JobFailure::Uncertain("index_unconfirmed"),
  }
}
