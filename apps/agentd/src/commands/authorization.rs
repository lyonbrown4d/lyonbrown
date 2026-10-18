use super::CommandTool;
use agent_workspace::process::{CommandAuthorization, CommandRequest, ResolvedCommand};
use serde_json::json;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
use tokio_util::sync::CancellationToken;

pub(super) struct Authorization {
  pub tool: CommandTool,
  pub call_id: String,
  pub cancel: CancellationToken,
  pub began: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl CommandAuthorization for Authorization {
  async fn authorize(
    &self,
    request: &CommandRequest,
    resolved: &ResolvedCommand,
  ) -> anyhow::Result<bool> {
    let exact = json!({"session_id":self.tool.session_id,"owner_epoch":self.tool.epoch,
      "run_id":self.tool.run_id,"call_id":self.call_id,"action_id":request.action_id,
      "executable":resolved.executable,"arguments":request.arguments,"cwd":resolved.cwd,
      "timeout_ms":request.timeout_ms,"output_limit_bytes":request.output_limit_bytes});
    let approval_id = crate::authorization::wait_for_approval(
      &self.tool.daemon,
      &self.tool.run_id,
      self.tool.epoch,
      "execute_command",
      &exact,
      self.cancel.clone(),
    )
    .await?;
    self.tool.fence(&self.cancel)?;
    self
      .tool
      .daemon
      .store
      .begin_approved_operation(agent_storage::ApprovedOperation {
        run_id: &self.tool.run_id,
        owner_epoch: self.tool.epoch,
        action_id: &request.action_id,
        tool: "execute_command",
        arguments: &exact,
        summary: json!({}),
        approval_id: &approval_id,
      })?;
    self.began.store(true, Ordering::Release);
    self.tool.daemon.events.notify_waiters();
    Ok(true)
  }
}
