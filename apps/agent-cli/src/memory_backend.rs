//! One request per explicit operation. Client code never chooses scope IDs.
use crate::{memory_command::MemoryCommand, memory_view};
use agent_client::AgentClient;
use agent_protocol::v1;

pub(crate) async fn execute(
  client: &AgentClient,
  session: Option<&str>,
  command: MemoryCommand,
) -> anyhow::Result<(String, String)> {
  if command == MemoryCommand::Status {
    return Ok((
      "Memory status".into(),
      memory_view::status(&client.memory_status().await?),
    ));
  }
  let session = session.unwrap_or_default();
  match command {
    MemoryCommand::List { state, after } => {
      let filter = state.map(memory_view::state_name).unwrap_or("all");
      let page = client
        .list_memories(v1::ListMemoriesRequest {
          session_id: session.into(),
          states: state.into_iter().map(|value| value as i32).collect(),
          after_id: after,
          limit: 50,
        })
        .await?;
      Ok(("Memories".into(), memory_view::page(&page, filter)))
    }
    MemoryCommand::Get(id) => Ok((
      "Memory".into(),
      memory_view::record(&client.get_memory(session, &id).await?),
    )),
    MemoryCommand::Search(query) => Ok((
      "Memory search".into(),
      memory_view::matches(&client.search_memories(session, &query, 10).await?),
    )),
    MemoryCommand::Create {
      id,
      request,
      scope,
      kind,
      state,
      content,
    } => {
      let result = client
        .put_memory(v1::PutMemoryRequest {
          session_id: session.into(),
          id: id.clone(),
          request_id: request.clone(),
          scope_kind: scope as i32,
          kind: kind as i32,
          state: state as i32,
          content,
          expected_revision: 0,
          expires_at_ms: None,
        })
        .await
        .map_err(|error| write_error(error, &id, Some(&request)))?;
      Ok(("Memory saved".into(), memory_view::record(&result)))
    }
    MemoryCommand::Approve {
      id,
      revision,
      request,
    } => {
      let current = current(client, session, &id, revision).await?;
      anyhow::ensure!(
        current.state == v1::MemoryState::Proposed as i32,
        "Only a proposed memory can be approved"
      );
      let result = replace(
        client,
        session,
        current,
        revision,
        request,
        None,
        Some(v1::MemoryState::Active),
      )
      .await?;
      Ok(("Memory approved".into(), memory_view::record(&result)))
    }
    MemoryCommand::Edit {
      id,
      revision,
      request,
      content,
    } => {
      let current = current(client, session, &id, revision).await?;
      anyhow::ensure!(
        current.state != v1::MemoryState::Deleted as i32,
        "A forgotten memory cannot be edited"
      );
      let result = replace(
        client,
        session,
        current,
        revision,
        request,
        Some(content),
        None,
      )
      .await?;
      Ok(("Memory updated".into(), memory_view::record(&result)))
    }
    MemoryCommand::Forget { id, revision } => {
      let result = client
        .forget_memory(session, &id, revision)
        .await
        .map_err(|error| write_error(error, &id, None))?;
      Ok((
        "Memory forgotten; index deletion queued".into(),
        memory_view::record(&result),
      ))
    }
    MemoryCommand::Jobs(id) => Ok((
      "Memory index jobs".into(),
      memory_view::jobs(&client.memory_jobs(session, &id).await?),
    )),
    MemoryCommand::Retry { id, job } => {
      client
        .retry_memory_job(session, &id, &job)
        .await
        .map_err(|error| write_error(error, &id, None))?;
      Ok((
        "Memory job retry requested".into(),
        format!(
          "Memory: {id}\nJob: {job}\nUse /memory jobs {id} to inspect the persisted outcome."
        ),
      ))
    }
    MemoryCommand::Status => Err(anyhow::anyhow!("Memory status request was not dispatched")),
  }
}

async fn current(
  client: &AgentClient,
  session: &str,
  id: &str,
  revision: u64,
) -> anyhow::Result<v1::MemoryRecord> {
  let current = client.get_memory(session, id).await?;
  anyhow::ensure!(
    current.id == id && current.revision == revision,
    "Memory revision changed; inspect /memory get {id} before issuing a new command"
  );
  Ok(current)
}

async fn replace(
  client: &AgentClient,
  session: &str,
  current: v1::MemoryRecord,
  revision: u64,
  request: String,
  content: Option<String>,
  state: Option<v1::MemoryState>,
) -> anyhow::Result<v1::MemoryRecord> {
  let scope = current
    .scope
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("Memory scope is unavailable"))?;
  let id = current.id.clone();
  client
    .put_memory(v1::PutMemoryRequest {
      session_id: session.into(),
      id: id.clone(),
      request_id: request.clone(),
      scope_kind: scope.kind,
      kind: current.kind,
      state: state.map_or(current.state, |value| value as i32),
      content: content.unwrap_or(current.content),
      expected_revision: revision,
      expires_at_ms: current.expires_at_ms,
    })
    .await
    .map_err(|error| write_error(error, &id, Some(&request)))
}

fn write_error(error: anyhow::Error, id: &str, request: Option<&str>) -> anyhow::Error {
  if let Some(status) = error.downcast_ref::<tonic::Status>()
    && matches!(
      status.code(),
      tonic::Code::Aborted
        | tonic::Code::AlreadyExists
        | tonic::Code::InvalidArgument
        | tonic::Code::NotFound
        | tonic::Code::PermissionDenied
        | tonic::Code::Unauthenticated
        | tonic::Code::FailedPrecondition
    )
  {
    return anyhow::anyhow!(
      "Memory write rejected ({:?}). Memory: {id}; inspect it before another write",
      status.code()
    );
  }
  anyhow::anyhow!(
    "Memory write was not confirmed. Memory: {id}; request: {}. Inspect the memory and its jobs before another write; no request was retried",
    request.unwrap_or("revision/job guarded")
  )
}
