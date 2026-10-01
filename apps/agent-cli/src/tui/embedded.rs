use std::sync::Arc;

use agent_app::{AgentClient, ApprovalInbox, spawn_runtime, spawn_runtime_with_approvals};
use agent_kernel::Kernel;
use anyhow::Result;

use super::runner::run_client;

pub async fn run_embedded(kernel: Kernel, approvals: Option<ApprovalInbox>) -> Result<()> {
  let (controller, actor_task) = match approvals {
    Some(approvals) => spawn_runtime_with_approvals(kernel, approvals, 32),
    None => spawn_runtime(kernel, 32),
  };
  let client: Arc<dyn AgentClient> = Arc::new(controller);
  let result = run_client(client).await;
  let close = actor_task.join().await;
  result?;
  close?;
  Ok(())
}
