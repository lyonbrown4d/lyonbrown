//! Authenticated reverse channel. The daemon authorizes every command before dispatch.
use agent_browser::{BrowserHost, Command, RUNTIME_PROTOCOL_VERSION, wire};
use agent_client::AgentClient;
use agent_protocol::v1::BrowserRuntimeResponse;
use anyhow::{Context, ensure};
use std::{collections::HashSet, path::Path};
use tokio_stream::wrappers::ReceiverStream;

pub struct RuntimeAttachment {
  pub host_id: String,
  task: tokio::task::JoinHandle<anyhow::Result<()>>,
  status: tokio::sync::watch::Receiver<Option<String>>,
  runtime: BrowserHost,
}
impl RuntimeAttachment {
  pub async fn shutdown(&mut self) -> anyhow::Result<()> {
    self.task.abort();
    let _ = (&mut self.task).await;
    self.runtime.request(Command::Shutdown).await?;
    Ok(())
  }

  pub fn error(&self) -> Option<String> {
    self.status.borrow().clone()
  }

  pub fn is_finished(&self) -> bool {
    self.task.is_finished()
  }
}
impl Drop for RuntimeAttachment {
  fn drop(&mut self) {
    self.task.abort();
  }
}

/// Launch the composed runtime and attach its private pipe to the authenticated daemon.
/// Dropping the lease disconnects pending work; neither layer retries browser actions.
pub async fn attach(client: AgentClient, profiles: &Path) -> anyhow::Result<RuntimeAttachment> {
  let runtime = crate::launch(profiles).await?;
  attach_host(client, runtime).await
}

/// Explicit executable injection for native embedding and end-to-end UI tests.
pub async fn attach_executable(
  client: AgentClient,
  executable: &Path,
  profiles: &Path,
) -> anyhow::Result<RuntimeAttachment> {
  attach_host(client, BrowserHost::spawn(executable, profiles).await?).await
}

/// Also accepts a separately launched runtime for native embedding and acceptance tests.
pub async fn attach_host(
  client: AgentClient,
  runtime: BrowserHost,
) -> anyhow::Result<RuntimeAttachment> {
  let host_id = uuid::Uuid::now_v7().to_string();
  let (sender, receiver) = tokio::sync::mpsc::channel(2);
  sender
    .send(BrowserRuntimeResponse {
      host_id: host_id.clone(),
      protocol_version: RUNTIME_PROTOCOL_VERSION,
      request_id: String::new(),
      payload: Vec::new(),
    })
    .await
    .context("runtime hello channel closed")?;
  let requests = client
    .attach_browser_runtime(ReceiverStream::new(receiver))
    .await?;
  let lease_id = host_id.clone();
  let (status_sender, status) = tokio::sync::watch::channel(None);
  let owned_runtime = runtime.clone();
  let task = tokio::spawn(async move {
    let result = async move {
      let mut requests = requests;
      let mut seen = HashSet::new();
      while let Some(request) = requests.message().await? {
        ensure!(
          !request.request_id.is_empty() && request.request_id.len() <= 128,
          "invalid browser runtime request identifier"
        );
        ensure!(
          seen.len() < 65_536 && seen.insert(request.request_id.clone()),
          "duplicate browser command or runtime lease limit reached"
        );
        let command: Command = wire::decode(&request.payload)?;
        let result = runtime.request(command).await;
        let payload = wire::encode(&result)?;
        sender
          .send(BrowserRuntimeResponse {
            host_id: lease_id.clone(),
            protocol_version: RUNTIME_PROTOCOL_VERSION,
            request_id: request.request_id,
            payload,
          })
          .await
          .context("daemon browser runtime channel closed")?;
      }
      anyhow::bail!("daemon disconnected the browser runtime; pending actions were not replayed")
    }
    .await;
    if let Err(error) = &result {
      let _ = status_sender.send(Some(error.to_string()));
    }
    result
  });
  Ok(RuntimeAttachment {
    host_id,
    task,
    status,
    runtime: owned_runtime,
  })
}
