//! The run event stream and the terminal-status helper used by chat operations.

use crate::{connection::Update, events, tabs::TabSender};
use agent_client::AgentClient;

pub(crate) fn terminal(status: &str) -> bool {
  matches!(
    status.to_lowercase().as_str(),
    "completed" | "failed" | "cancelled" | "uncertain"
  )
}

pub(crate) async fn watch(
  client: AgentClient,
  id: String,
  sender: TabSender,
  after_cursor: u64,
) -> anyhow::Result<()> {
  let mut stream = client.watch_run(&id, after_cursor).await?;
  while let Some(event) = stream.message().await? {
    let approval_required = events::is_approval_required(&event.kind);
    let completed = events::is_terminal(&event.kind);
    let _ = sender.send(Update::Event(event)).await;
    if approval_required {
      let _ = sender
        .send(Update::Approvals(client.list_approvals(&id).await?))
        .await;
    }
    if completed {
      let _ = sender.send(Update::Run(client.get_run(&id).await?)).await;
      return Ok(());
    }
  }
  let run = client.get_run(&id).await?;
  let _ = sender.send(Update::Run(run)).await;
  Ok(())
}
