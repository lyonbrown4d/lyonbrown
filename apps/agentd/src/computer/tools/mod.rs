mod action;
mod input;
mod lease;
mod observe;

use super::router::PinnedHost;
use crate::Daemon;
use agent_computer::LeaseToken;
use agent_computer_runtime::{Operation, Reply, RunBinding};
use agent_core::{Run, Session};
use agent_runtime::{RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use anyhow::{Result, ensure};
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
struct ObservationReference {
  id: Uuid,
  revision: u64,
  digest: [u8; 32],
}
#[derive(Default)]
struct State {
  lease: Option<LeaseToken>,
  observation: Option<ObservationReference>,
}
struct Context {
  daemon: Daemon,
  binding: RunBinding,
  host: PinnedHost,
  state: Mutex<State>,
  lease_watch: watch::Sender<Option<LeaseToken>>,
  lease_ms: u64,
}
pub(crate) struct RunTools {
  context: Arc<Context>,
  keeper: tokio::task::JoinHandle<()>,
}

pub(crate) async fn tools(
  daemon: Daemon,
  run: &Run,
  session: &Session,
  cancellation: CancellationToken,
) -> Option<RunTools> {
  if !daemon.config.computer_tools {
    return None;
  }
  let host = daemon
    .computer
    .pin(&session.id, session.owner_epoch)
    .await
    .ok()?;
  let binding = RunBinding {
    daemon_session_id: session.id.clone(),
    run_id: run.id.clone(),
    owner_epoch: session.owner_epoch,
  };
  daemon.validate_computer_run(&binding).ok()?;
  // Permission inspection has no capture or input and cannot prompt the OS.
  // Presence alone is insufficient to offer control to the decision model.
  let Ok(Reply::Capabilities(capabilities)) = daemon
    .computer
    .request(
      &host,
      binding.clone(),
      Operation::Capabilities,
      cancellation.clone(),
    )
    .await
    .ok()?
  else {
    return None;
  };
  if !capabilities.can_control() || cancellation.is_cancelled() {
    return None;
  }
  daemon.validate_computer_run(&binding).ok()?;
  let lease_ms = daemon.config.computer_lease_secs.checked_mul(1000)?;
  if !(10_000..=300_000).contains(&lease_ms) {
    return None;
  }
  let (lease_watch, _) = watch::channel(None);
  let context = Arc::new(Context {
    daemon,
    binding,
    host,
    state: Mutex::new(State::default()),
    lease_watch,
    lease_ms,
  });
  let keeper = lease::supervise(&context, cancellation);
  Some(RunTools { context, keeper })
}

impl RunTools {
  pub(crate) fn definitions(&self) -> Vec<Arc<dyn RuntimeTool>> {
    ["computer_observe", "computer_act"]
      .into_iter()
      .map(|name| {
        Arc::new(ComputerTool {
          context: self.context.clone(),
          name,
        }) as Arc<dyn RuntimeTool>
      })
      .collect()
  }

  pub(crate) async fn close(&self) {
    self.keeper.abort();
    self.context.lease_watch.send_replace(None);
    let lease = {
      let mut state = self.context.state.lock().await;
      state.observation = None;
      state.lease.take()
    };
    if let Some(lease) = lease {
      let request = self.context.daemon.computer.request(
        &self.context.host,
        self.context.binding.clone(),
        Operation::Revoke { lease },
        CancellationToken::new(),
      );
      if !matches!(
        tokio::time::timeout(std::time::Duration::from_secs(2), request).await,
        Ok(Ok(Ok(Reply::Revoked)))
      ) {
        self
          .context
          .daemon
          .computer
          .disconnect(
            &self.context.binding.daemon_session_id,
            self.context.host.generation,
          )
          .await;
      }
    }
  }
}

struct ComputerTool {
  context: Arc<Context>,
  name: &'static str,
}
#[async_trait::async_trait]
impl RuntimeTool for ComputerTool {
  fn definition(&self) -> ToolDefinition {
    input::definition(self.name)
  }
  async fn execute(
    &self,
    call: ToolCall,
    cancellation: CancellationToken,
  ) -> agent_runtime::Result<ToolOutput> {
    let result = if self.name == "computer_observe" {
      self.context.observe(call.arguments, cancellation).await
    } else {
      self.context.act(call, cancellation).await
    };
    match result {
      Ok(value) => Ok(ToolOutput { text: value.to_string(), is_error: false }),
      Err(error) => match error.downcast::<RuntimeError>() {
        Ok(error) => Err(error),
        // Native errors may contain private titles or text. Expose only the
        // bounded message supplied by our own adapter.
        Err(_) => Ok(ToolOutput {
          text: "Computer operation rejected; inspect permissions, scope and observation, then obtain a new observation before another action.".into(),
          is_error: true,
        }),
      },
    }
  }
}
impl Context {
  fn fence(&self, cancellation: &CancellationToken) -> Result<()> {
    ensure!(!cancellation.is_cancelled(), "computer operation cancelled");
    self.daemon.validate_computer_run(&self.binding)?;
    Ok(())
  }
  async fn request(&self, operation: Operation, cancellation: CancellationToken) -> Result<Reply> {
    self.fence(&cancellation)?;
    let result = self
      .daemon
      .computer
      .request(
        &self.host,
        self.binding.clone(),
        operation,
        cancellation.clone(),
      )
      .await?;
    self.fence(&cancellation)?;
    Ok(result?)
  }
}

impl Drop for Context {
  fn drop(&mut self) {
    if let Some(lease) = self.state.get_mut().lease.take()
      && let Ok(handle) = tokio::runtime::Handle::try_current()
    {
      let router = self.daemon.computer.clone();
      let host = self.host.clone();
      let binding = self.binding.clone();
      // Exact-token revocation cannot cancel a replacement controller's lease.
      handle.spawn(async move {
        let _ = router
          .request(
            &host,
            binding,
            Operation::Revoke { lease },
            CancellationToken::new(),
          )
          .await;
      });
    }
  }
}

impl Drop for RunTools {
  fn drop(&mut self) {
    self.keeper.abort();
  }
}
