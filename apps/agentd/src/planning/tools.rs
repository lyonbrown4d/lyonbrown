use super::{context, schema};
use crate::Daemon;
use agent_core::{Run, Session};
use agent_planning::{Actor, PlanContext, PlanMutation};
use agent_runtime::{RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeSet, sync::Arc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct PlanTool {
  daemon: Daemon,
  binding: PlanContext,
  write: bool,
}
/// Called only with the existing host-authorized tool set; plans cannot expand it.
pub(crate) fn tools(
  daemon: Daemon,
  run: &Run,
  session: &Session,
  allowed: &BTreeSet<String>,
) -> Vec<Arc<dyn RuntimeTool>> {
  [false, true]
    .into_iter()
    .filter(|write| allowed.contains(if *write { "plan_update" } else { "plan_read" }))
    .map(|write| {
      Arc::new(PlanTool {
        daemon: daemon.clone(),
        binding: PlanContext {
          session_id: session.id.clone(),
          run_id: run.id.clone(),
          owner_epoch: session.owner_epoch,
        },
        write,
      }) as Arc<dyn RuntimeTool>
    })
    .collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
  plan_id: Option<Uuid>,
  after: Option<Uuid>,
  #[serde(default = "default_limit")]
  limit: u32,
}
fn default_limit() -> u32 {
  20
}
#[async_trait::async_trait]
impl RuntimeTool for PlanTool {
  fn definition(&self) -> ToolDefinition {
    schema::definition(self.write)
  }
  async fn execute(
    &self,
    call: ToolCall,
    cancel: CancellationToken,
  ) -> agent_runtime::Result<ToolOutput> {
    match self.perform(call, cancel).await {
      Ok(value) => Ok(ToolOutput {
        text: value.to_string(),
        is_error: false,
      }),
      Err(error) => Ok(ToolOutput {
        text: agent_storage::public_error_message(&error),
        is_error: true,
      }),
    }
  }
}
impl PlanTool {
  async fn perform(&self, call: ToolCall, cancel: CancellationToken) -> Result<serde_json::Value> {
    ensure!(!cancel.is_cancelled(), "plan operation cancelled");
    let current = context(&self.daemon, &self.binding.run_id).await?;
    ensure!(current == self.binding, "plan session ownership changed");
    let run = self
      .daemon
      .store
      .run(&self.binding.run_id)
      .await?
      .ok_or_else(|| anyhow::anyhow!("plan run unavailable"))?;
    ensure!(run.status == "running", "plan run is no longer active");
    if self.write {
      let mutation: PlanMutation = serde_json::from_value(call.arguments)?;
      ensure!(!cancel.is_cancelled(), "plan operation cancelled");
      Ok(serde_json::to_value(
        self
          .daemon
          .store
          .plan_apply(&self.binding, Actor::Model, &mutation)
          .await?,
      )?)
    } else {
      let read: Read = serde_json::from_value(call.arguments)?;
      if let Some(id) = read.plan_id {
        ensure!(read.after.is_none(), "get cannot include a list cursor");
        Ok(serde_json::to_value(
          super::get(&self.daemon, &self.binding.run_id, id).await?,
        )?)
      } else {
        Ok(serde_json::to_value(
          super::list(&self.daemon, &self.binding.run_id, read.after, read.limit).await?,
        )?)
      }
    }
  }
}
