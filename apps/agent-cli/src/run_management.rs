use agent_client::AgentClient;
use agent_protocol::v1;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

use crate::run_command::RunCommand;

pub async fn execute(client: &AgentClient, command: RunCommand) -> Result<Value> {
  ensure!(
    client
      .node()
      .await?
      .capabilities
      .iter()
      .any(|value| value == "runs.recover"),
    "daemon does not support run recovery"
  );
  let (recovery, action, prompt) = match command {
    RunCommand::Inspect { run_id } => {
      let value = client.run_recovery(&run_id).await?;
      let tasks = if let Some(run) = &value.run {
        client.recovery_tasks(&run.session_id, "").await?
      } else {
        v1::Tasks::default()
      };
      return Ok(json!({
        "run": value.run.as_ref().map(run_json), "owner_epoch": value.owner_epoch,
        "reason": value.reason, "pending_tools": value.pending_tools,
        "allowed_actions": value.allowed_actions.iter().map(|value| action_name(*value)).collect::<Vec<_>>(),
        "fresh_session_required": value.fresh_session_required,
        "resolution": value.resolution.as_ref().map(result_json),
        "unresolved_tasks": tasks.tasks.iter().map(|task| json!({
          "task_id": task.envelope.as_ref().map(|value| &value.task_id),
          "status": task.status, "attempt_status": task.attempt_status,
        })).collect::<Vec<_>>(),
        "next_task_id": tasks.next_task_id,
        "task_guidance": "Use task get/reconcile with the original paired Worker target, then cancel or explicitly accept/reject. For more pages use task list --session SESSION_ID --after NEXT_TASK_ID. Refresh run inspect afterward; the daemon checks all tasks atomically.",
      }));
    }
    RunCommand::Abandon(recovery) => (recovery, v1::RunRecoveryAction::Abandon, String::new()),
    RunCommand::Continue { recovery, prompt } => {
      (recovery, v1::RunRecoveryAction::ContinueNewSession, prompt)
    }
  };
  let info = client.run_recovery(&recovery.run_id).await?;
  let request = v1::RecoverRunRequest {
    run_id: recovery.run_id.clone(),
    owner_epoch: info
      .resolution
      .as_ref()
      .map_or(info.owner_epoch, |result| result.owner_epoch),
    action: action as i32,
    idempotency_key: recovery.idempotency_key,
    note: recovery.note,
    prompt,
  };
  let result = client.recover_run(request).await.map_err(|error| anyhow::anyhow!(
    "{error}; outcome may be unknown. Use /run inspect {} and preserve the original recovery key; do not submit a fresh recovery identity",
    recovery.run_id
  ))?;
  Ok(result_json(&result))
}

fn action_name(value: i32) -> &'static str {
  match v1::RunRecoveryAction::try_from(value) {
    Ok(v1::RunRecoveryAction::Abandon) => "abandon",
    Ok(v1::RunRecoveryAction::ContinueNewSession) => "continue_new_session",
    _ => "unspecified",
  }
}

fn run_json(run: &v1::Run) -> Value {
  json!({"id":run.id,"session_id":run.session_id,"status":run.status,
    "prompt":run.prompt,"result":run.result,"error":run.error})
}

fn result_json(result: &v1::RunRecoveryResult) -> Value {
  json!({
    "original_run": result.original_run.as_ref().map(run_json),
    "session": result.session.as_ref().map(|session| json!({"id":session.id,"title":session.title})),
    "run": result.run.as_ref().map(run_json), "action":action_name(result.action),"note":result.note,
    "owner_epoch":result.owner_epoch,"idempotency_key":result.idempotency_key,
    "guidance":"Original run remains uncertain and is never replayed. Use Tab to select the new session, if created, to follow the new run and approvals.",
  })
}
