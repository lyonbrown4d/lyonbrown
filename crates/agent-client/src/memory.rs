//! Authenticated memory commands. No client-side authority or index SDK.
use crate::{AgentClient, v1};
use anyhow::Result;
impl AgentClient {
  pub async fn list_memories(&self, request: v1::ListMemoriesRequest) -> Result<v1::Memories> {
    Ok(
      self
        .memory
        .clone()
        .list_memories(request)
        .await?
        .into_inner(),
    )
  }
  pub async fn get_memory(&self, session: &str, id: &str) -> Result<v1::MemoryRecord> {
    Ok(
      self
        .memory
        .clone()
        .get_memory(v1::GetMemoryRequest {
          session_id: session.into(),
          memory_id: id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn put_memory(&self, request: v1::PutMemoryRequest) -> Result<v1::MemoryRecord> {
    Ok(self.memory.clone().put_memory(request).await?.into_inner())
  }
  pub async fn forget_memory(
    &self,
    session: &str,
    id: &str,
    expected_revision: u64,
  ) -> Result<v1::MemoryRecord> {
    Ok(
      self
        .memory
        .clone()
        .forget_memory(v1::ForgetMemoryRequest {
          session_id: session.into(),
          memory_id: id.into(),
          expected_revision,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn search_memories(
    &self,
    session: &str,
    query: &str,
    limit: u32,
  ) -> Result<v1::MemoryMatches> {
    Ok(
      self
        .memory
        .clone()
        .search_memories(v1::SearchMemoriesRequest {
          session_id: session.into(),
          query: query.into(),
          limit,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn memory_jobs(&self, session: &str, id: &str) -> Result<v1::MemoryJobs> {
    Ok(
      self
        .memory
        .clone()
        .list_memory_jobs(v1::GetMemoryRequest {
          session_id: session.into(),
          memory_id: id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn retry_memory_job(&self, session: &str, memory: &str, job: &str) -> Result<()> {
    self
      .memory
      .clone()
      .retry_memory_job(v1::RetryMemoryJobRequest {
        session_id: session.into(),
        memory_id: memory.into(),
        job_id: job.into(),
      })
      .await?;
    Ok(())
  }
  pub async fn memory_status(&self) -> Result<v1::MemoryStatus> {
    Ok(
      self
        .memory
        .clone()
        .get_memory_status(v1::Empty {})
        .await?
        .into_inner(),
    )
  }
}
