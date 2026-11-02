//! Authenticated local-owner memory management. Model tools use proposals separately.
mod access;
#[cfg(test)]
mod tests;
mod wire;
use crate::Daemon;
use access::selected_scope;
use agent_protocol::v1::{
  self,
  memory_service_server::{MemoryService, MemoryServiceServer},
};
use agent_security::Authenticate;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};
use wire::failure;

pub fn memory_service(
  daemon: Daemon,
) -> InterceptedService<MemoryServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  InterceptedService::new(
    MemoryServiceServer::new(daemon)
      .max_decoding_message_size(128 * 1024)
      .max_encoding_message_size(2 * 1024 * 1024),
    auth,
  )
}
#[tonic::async_trait]
impl MemoryService for Daemon {
  async fn list_memories(
    &self,
    request: Request<v1::ListMemoriesRequest>,
  ) -> Result<Response<v1::Memories>, Status> {
    let input = request.into_inner();
    let states = wire::states(&input.states)?;
    let limit = page_limit(input.limit, 50)?;
    let access = self.memory_access(&input.session_id).await?;
    let after = if input.after_id.is_empty() {
      None
    } else {
      Some(input.after_id.as_str())
    };
    let page = self
      .memory
      .list(&access, &states, after, limit)
      .await
      .map_err(failure)?;
    Ok(Response::new(v1::Memories {
      items: page.items.into_iter().map(wire::record).collect(),
      next_cursor: page.next_cursor.unwrap_or_default(),
    }))
  }
  async fn get_memory(
    &self,
    request: Request<v1::GetMemoryRequest>,
  ) -> Result<Response<v1::MemoryRecord>, Status> {
    let input = request.into_inner();
    let access = self.memory_access(&input.session_id).await?;
    let item = self
      .memory
      .get(&access, &input.memory_id)
      .await
      .map_err(failure)?
      .ok_or_else(|| Status::not_found("memory not found"))?;
    Ok(Response::new(wire::record(item)))
  }
  async fn put_memory(
    &self,
    request: Request<v1::PutMemoryRequest>,
  ) -> Result<Response<v1::MemoryRecord>, Status> {
    let input = request.into_inner();
    let access = self.memory_access(&input.session_id).await?;
    let scope = selected_scope(&access, input.scope_kind)?;
    // This authenticated user RPC never accepts a source marker from the client.
    let mutation = wire::put(input, scope)?;
    let item = self.memory.save(&access, mutation).await.map_err(failure)?;
    self.events.notify_waiters();
    Ok(Response::new(wire::record(item)))
  }
  async fn forget_memory(
    &self,
    request: Request<v1::ForgetMemoryRequest>,
  ) -> Result<Response<v1::MemoryRecord>, Status> {
    let input = request.into_inner();
    let access = self.memory_access(&input.session_id).await?;
    let item = self
      .memory
      .forget(&access, &input.memory_id, input.expected_revision)
      .await
      .map_err(failure)?;
    self.events.notify_waiters();
    Ok(Response::new(wire::record(item)))
  }
  async fn search_memories(
    &self,
    request: Request<v1::SearchMemoriesRequest>,
  ) -> Result<Response<v1::MemoryMatches>, Status> {
    let input = request.into_inner();
    let limit = page_limit(input.limit, 10)?;
    let access = self.memory_access(&input.session_id).await?;
    let items = self
      .memory
      .search(&access, &input.query, limit, self.stopping.child_token())
      .await
      .map_err(failure)?;
    Ok(Response::new(v1::MemoryMatches {
      items: items
        .into_iter()
        .map(|value| v1::MemoryMatch {
          memory: Some(wire::record(value.item)),
          score: value.score,
        })
        .collect(),
    }))
  }
  async fn list_memory_jobs(
    &self,
    request: Request<v1::GetMemoryRequest>,
  ) -> Result<Response<v1::MemoryJobs>, Status> {
    let input = request.into_inner();
    let access = self.memory_access(&input.session_id).await?;
    let jobs = self
      .memory
      .jobs(&access, &input.memory_id)
      .await
      .map_err(failure)?;
    Ok(Response::new(v1::MemoryJobs {
      jobs: jobs.into_iter().map(wire::job).collect(),
    }))
  }
  async fn retry_memory_job(
    &self,
    request: Request<v1::RetryMemoryJobRequest>,
  ) -> Result<Response<v1::Empty>, Status> {
    let input = request.into_inner();
    let access = self.memory_access(&input.session_id).await?;
    self
      .memory
      .retry_job(&access, &input.memory_id, &input.job_id)
      .await
      .map_err(failure)?;
    self.events.notify_waiters();
    Ok(Response::new(v1::Empty {}))
  }
  async fn get_memory_status(
    &self,
    _: Request<v1::Empty>,
  ) -> Result<Response<v1::MemoryStatus>, Status> {
    Ok(Response::new(wire::status(
      self.memory.status().await.map_err(failure)?,
    )))
  }
}
fn page_limit(value: u32, default: u32) -> Result<u32, Status> {
  if value > 100 {
    return Err(Status::invalid_argument(
      "memory page limit must be at most 100",
    ));
  }
  Ok(if value == 0 { default } else { value })
}
