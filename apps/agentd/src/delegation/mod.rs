//! The Harness may propose delegation; only scoped exact approval can dispatch it.
mod input;
mod operations;
mod schema;

use std::{collections::BTreeSet, sync::Arc};

use agent_core::{Run, Session, Task};
use agent_runtime::{Result, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use anyhow::ensure;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::Daemon;

struct DelegationTool {
  daemon: Daemon,
  run_id: String,
  session_id: String,
  workspace: String,
  epoch: u64,
  targets: BTreeSet<String>,
  capabilities: BTreeSet<String>,
}

pub(crate) fn available_targets(daemon: &Daemon) -> anyhow::Result<BTreeSet<String>> {
  let manager = crate::peers::manager(daemon)?;
  Ok(
    daemon
      .config
      .delegation_targets
      .iter()
      .filter(|target| manager.trust(&target.node_id).is_ok())
      .map(|target| target.node_id.clone())
      .collect(),
  )
}

pub(crate) fn tools(
  daemon: Daemon,
  run: &Run,
  session: &Session,
  allowed_targets: BTreeSet<String>,
  allowed_capabilities: BTreeSet<String>,
) -> Vec<Arc<dyn RuntimeTool>> {
  let targets = daemon
    .config
    .delegation_targets
    .iter()
    .filter(|target| allowed_targets.contains(&target.node_id))
    .map(|target| target.node_id.clone())
    .collect::<BTreeSet<_>>();
  if targets.is_empty() || session.id.starts_with("worker:") {
    return Vec::new();
  }
  vec![Arc::new(DelegationTool {
    daemon,
    run_id: run.id.clone(),
    session_id: session.id.clone(),
    workspace: session.workspace.clone(),
    epoch: session.owner_epoch,
    targets,
    capabilities: allowed_capabilities,
  })]
}

#[async_trait::async_trait]
impl RuntimeTool for DelegationTool {
  fn definition(&self) -> ToolDefinition {
    schema::definition(&self.targets)
  }

  async fn execute(&self, call: ToolCall, cancel: CancellationToken) -> Result<ToolOutput> {
    match self.perform(call, cancel).await {
      Ok(value) => Ok(ToolOutput {
        text: value.to_string(),
        is_error: false,
      }),
      Err(error) => match error.downcast::<RuntimeError>() {
        Ok(error) => Err(error),
        Err(error) => Ok(ToolOutput {
          text: error.to_string(),
          is_error: true,
        }),
      },
    }
  }
}

impl DelegationTool {
  fn fence(&self, cancel: &CancellationToken) -> anyhow::Result<()> {
    ensure!(
      !cancel.is_cancelled(),
      "delegation cancelled before dispatch"
    );
    let session = self
      .daemon
      .store
      .session(&self.session_id)?
      .ok_or_else(|| anyhow::anyhow!("delegation session unavailable"))?;
    ensure!(
      session.owner_node_id == self.daemon.node_id
        && session.owner_epoch == self.epoch
        && session.workspace == self.workspace,
      "delegation session ownership changed"
    );
    let run = self
      .daemon
      .store
      .run(&self.run_id)?
      .ok_or_else(|| anyhow::anyhow!("delegation parent run unavailable"))?;
    ensure!(
      run.session_id == self.session_id && run.status == "running",
      "delegation parent run is no longer active"
    );
    Ok(())
  }

  fn target(&self, id: &str) -> anyhow::Result<agent_protocol::v1::Node> {
    ensure!(
      self.targets.contains(id),
      "delegation target is outside the execution plan"
    );
    let config = self
      .daemon
      .config
      .delegation_targets
      .iter()
      .find(|target| target.node_id == id)
      .ok_or_else(|| anyhow::anyhow!("delegation target is not configured"))?;
    agent_config::validate_delegation_targets(std::slice::from_ref(config))?;
    let trust = crate::peers::manager(&self.daemon)?.trust(id)?;
    let fingerprint = agent_distributed::identity::PublicIdentity {
      node_id: trust.node_id,
      certificate_pem: trust.certificate_pem,
      noise_public_key: trust.noise_public_key,
    }
    .pairing_fingerprint()?;
    Ok(agent_protocol::v1::Node {
      id: id.into(),
      endpoint: config.endpoint.clone(),
      fingerprint,
      ..Default::default()
    })
  }

  fn revalidate_target(&self, target: &agent_protocol::v1::Node) -> anyhow::Result<()> {
    ensure!(
      self.target(&target.id)? == *target,
      "delegation pairing changed after approval"
    );
    Ok(())
  }

  fn task(&self, id: &str, target: &str) -> anyhow::Result<Task> {
    ensure!(id.len() <= 256, "invalid task identifier");
    let task = self
      .daemon
      .store
      .task(id)?
      .ok_or_else(|| anyhow::anyhow!("delegation task unavailable"))?;
    ensure!(
      task.envelope.parent_run_id == self.run_id
        && task.envelope.session_id == self.session_id
        && task.envelope.owner_node_id == self.daemon.node_id
        && task.envelope.owner_epoch == self.epoch
        && self.daemon.store.task_target(id)?.as_deref() == Some(target),
      "delegation task is outside this parent run"
    );
    Ok(task)
  }

  async fn approve(&self, exact: &Value, cancel: &CancellationToken) -> anyhow::Result<()> {
    crate::authorization::approve(
      &self.daemon,
      &self.run_id,
      self.epoch,
      "delegate_task",
      exact,
      cancel.clone(),
    )
    .await?;
    self.fence(cancel)
  }
}

fn digest(task: &Task) -> String {
  let mut hash = Sha256::new();
  for value in [
    &task.result,
    &task.error,
    &task.status,
    &task.attempt_status,
  ] {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value.as_bytes());
  }
  hex::encode(hash.finalize())
}

fn output(task: Task) -> Value {
  json!({"untrusted_worker_content":true,"task_id":task.envelope.task_id,
    "attempt_id":task.envelope.attempt_id,"status":task.status,
    "attempt_status":task.attempt_status,"result_digest":digest(&task),
    "result":task.result.chars().take(16384).collect::<String>(),
    "error":task.error.chars().take(2048).collect::<String>(),
    "result_truncated":task.result.chars().count()>16384})
}
