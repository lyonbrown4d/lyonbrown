use std::time::{SystemTime, UNIX_EPOCH};

use agent_client::AgentClient;
use agent_protocol::v1;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use crate::task_command::{Command, Dispatch, TaskCommand};

/// Executes each management operation once; a transport failure never resubmits a task.
pub async fn execute(client: &AgentClient, command: Command) -> Result<Value> {
  let Command::Task { command } = command else {
    anyhow::bail!("computer commands require the interactive runtime entry");
  };
  let node = client.node().await?;
  ensure!(
    node
      .capabilities
      .iter()
      .any(|capability| capability == "tasks.manage"),
    "daemon does not support task management"
  );
  let task = match command {
    TaskCommand::List {
      session,
      after,
      page_size,
    } => {
      let page = client
        .list_tasks(&session, after.as_deref().unwrap_or(""), page_size)
        .await?;
      return Ok(json!({
        "tasks": page.tasks.iter().map(task_json).collect::<Vec<_>>(),
        "next_task_id": page.next_task_id,
      }));
    }
    TaskCommand::Get { task_id } => client.task(&task_id).await?,
    TaskCommand::Dispatch(args) => {
      let session = client
        .list_sessions()
        .await?
        .into_iter()
        .find(|session| session.id == args.session)
        .context("session not found")?;
      let parent = client.get_run(&args.parent_run).await?;
      let envelope = dispatch_envelope(&args, &session, &parent, &node, now_ms()?)?;
      client
        .dispatch_task(args.target.node(), envelope)
        .await
        .with_context(|| recovery(&args.task_id))?
    }
    TaskCommand::Reconcile(args) => client
      .reconcile_task(args.target.node(), &args.task_id)
      .await
      .with_context(|| recovery(&args.task_id))?,
    TaskCommand::Cancel(args) => client
      .cancel_task(args.target.node(), &args.task_id)
      .await
      .with_context(|| recovery(&args.task_id))?,
    TaskCommand::Accept { task, accepted } => client
      .accept_task_result(task.target.node(), &task.task_id, accepted)
      .await
      .with_context(|| recovery(&task.task_id))?,
  };
  Ok(task_json(&task))
}

fn recovery(id: &str) -> String {
  format!(
    "Task {id} operation failed; its remote outcome may be uncertain. Use task get {id} and task reconcile {id} with the original Worker target. Preserve the original task ID, attempt ID and idempotency key; do not dispatch a fresh identity to retry"
  )
}

fn now_ms() -> Result<i64> {
  Ok(i64::try_from(
    SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
  )?)
}

/// Construct only daemon-owned session and owner fields, with the requested bounded scope.
pub fn dispatch_envelope(
  args: &Dispatch,
  session: &v1::Session,
  parent: &v1::Run,
  node: &v1::Node,
  now_ms: i64,
) -> Result<v1::TaskEnvelope> {
  ensure!(
    session.id == args.session,
    "session differs from requested session"
  );
  ensure!(
    session.owner_node_id == node.id,
    "session is not owned by this Master"
  );
  ensure!(
    parent.id == args.parent_run && parent.session_id == session.id && parent.status == "running",
    "parent run must be active in the task session"
  );
  ensure!(
    !args.instructions.trim().is_empty(),
    "task instructions must be nonempty"
  );
  ensure!(
    (1..=86_400).contains(&args.deadline_seconds),
    "task deadline must be within the next 24 hours"
  );
  ensure!(
    args.capability.len() <= 2
      && args
        .capability
        .iter()
        .all(|cap| matches!(cap.as_str(), "workspace.read" | "workspace.write.approval")),
    "unsupported task capability scope"
  );
  let envelope = v1::TaskEnvelope {
    task_id: args.task_id.clone(),
    attempt_id: args.attempt_id.clone(),
    parent_run_id: parent.id.clone(),
    session_id: session.id.clone(),
    owner_node_id: node.id.clone(),
    owner_epoch: session.owner_epoch,
    workspace: session.workspace.clone(),
    instructions: args.instructions.clone(),
    capabilities: args.capability.clone(),
    idempotency_key: args.idempotency_key.clone(),
    deadline_ms: now_ms
      .checked_add(i64::from(args.deadline_seconds) * 1000)
      .context("task deadline overflow")?,
  };
  let payload_size = [
    &envelope.task_id,
    &envelope.attempt_id,
    &envelope.parent_run_id,
    &envelope.session_id,
    &envelope.owner_node_id,
    &envelope.workspace,
    &envelope.instructions,
    &envelope.idempotency_key,
  ]
  .iter()
  .map(|value| value.len())
  .sum::<usize>()
    + envelope.capabilities.iter().map(String::len).sum::<usize>();
  ensure!(
    payload_size <= 64 * 1024,
    "task payload must be at most 64 KiB"
  );
  Ok(envelope)
}

fn task_json(task: &v1::Task) -> Value {
  let envelope = task.envelope.as_ref().map(|value| {
    json!({
      "task_id": value.task_id, "attempt_id": value.attempt_id,
      "parent_run_id": value.parent_run_id, "session_id": value.session_id,
      "owner_node_id": value.owner_node_id, "owner_epoch": value.owner_epoch,
      "workspace": value.workspace, "instructions": value.instructions,
      "capabilities": value.capabilities, "idempotency_key": value.idempotency_key,
      "deadline_ms": value.deadline_ms,
    })
  });
  json!({"envelope": envelope, "status": task.status,
    "attempt_status": task.attempt_status, "result": task.result,
    "error": task.error, "cursor": task.cursor})
}
