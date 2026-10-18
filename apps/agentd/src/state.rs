use crate::config::Config;
use agent_security::NodeLease;
use agent_storage::Store;
use anyhow::Result;
use std::{
  collections::HashMap,
  sync::{Arc, OnceLock, atomic::AtomicBool},
};
use tokio::sync::{Mutex, Notify, Semaphore};
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
pub struct Daemon {
  pub config: Config,
  pub store: Store,
  pub node_id: String,
  pub(crate) token: Arc<String>,
  pub(crate) services: fluxdi::Shared<RuntimeServices>,
  pub(crate) decision: fluxdi::Shared<agent_decision::DecisionEngine>,
  pub(crate) models: fluxdi::Shared<crate::models::Models>,
  pub(crate) mcp: fluxdi::Shared<crate::mcp::Connections>,
  pub(crate) _lease: Arc<NodeLease>,
}
impl Daemon {
  pub async fn open(config: Config) -> Result<Self> {
    let models = crate::models::Models::load(&config).await?;
    let daemon = crate::composition::build(config, models)?;
    daemon.mcp.initialize(&daemon.config).await?;
    Ok(daemon)
  }
  /// Embed an explicitly loaded inference registry; no global registry or hidden test mode.
  pub async fn open_with_models(
    config: Config,
    registry: Arc<agent_inference::ModelRegistry>,
  ) -> Result<Self> {
    agent_config::validate_local_models(&config.local_models)?;
    let models = crate::models::Models::from_registry(&config, registry).await?;
    let daemon = crate::composition::build(config, models)?;
    daemon.mcp.initialize(&daemon.config).await?;
    Ok(daemon)
  }
  pub fn local_models(&self) -> Vec<agent_inference::ModelInfo> {
    self.models.all()
  }
  pub async fn recover(&self) -> Result<()> {
    let operations_uncertain = self.store.recover_operations()?;
    tracing::info!(
      operations_uncertain,
      "reconciled interrupted native operations"
    );
    let browser_uncertain = self.store.recover_browser_actions()?;
    tracing::info!(browser_uncertain, "reconciled interrupted browser actions");
    let interrupted = self.store.recover_interrupted()?;
    tracing::info!(uncertain = interrupted.len(), "reconciled interrupted runs");
    for run in self.store.pending_runs()? {
      if run.session_id.starts_with("worker:") {
        self.store.update_run(
          &run.id,
          agent_core::RunStatus::Uncertain,
          "",
          "worker task interrupted before execution; reconcile without replay",
        )?;
      } else {
        self.launch(run).await?;
      }
    }
    Ok(())
  }
  pub async fn shutdown(&self) {
    let worker = {
      let _submission = self.submission.lock().await;
      let worker = self.peer_worker.lock().await.clone();
      self.stopping.cancel();
      worker
    };
    {
      let active = self.active.lock().await;
      for token in active.values() {
        token.cancel();
      }
    }
    let wait = async {
      loop {
        let notify = self.events.notified();
        tokio::pin!(notify);
        notify.as_mut().enable();
        if self.active.lock().await.is_empty() {
          break;
        }
        notify.await;
      }
    };
    let drain = async {
      tokio::join!(wait, async {
        if let Some(worker) = worker {
          worker.shutdown().await;
        }
      });
    };
    if tokio::time::timeout(std::time::Duration::from_secs(17), drain)
      .await
      .is_err()
    {
      tracing::warn!(
        "daemon shutdown deadline expired; unfinished runs reconcile as uncertain on restart"
      );
    }
    self.mcp.cancel();
  }
  pub fn auth_token(&self) -> &str {
    &self.token
  }
  pub(crate) async fn active_count(&self) -> usize {
    let local = self.active.lock().await.len();
    // Unknown remote outcomes also defer upgrades until explicit reconciliation.
    let remote = self.store.active_master_tasks(&self.node_id).unwrap_or_else(|error| {
      tracing::error!(error=%error, "could not determine in-flight Master tasks; deferring idle maintenance");
      1
    });
    let peer = self.peer_worker.lock().await.clone();
    local.saturating_add(remote)
      + match peer {
        Some(worker) => worker.active_count().await,
        None => 0,
      }
  }
}

#[doc(hidden)]
pub struct RuntimeServices {
  pub(crate) browser: Arc<crate::browser::BrowserRouter>,
  pub(crate) computer: Arc<crate::computer::ComputerRouter>,
  pub(crate) workspace_writes: Mutex<()>,
  pub(crate) events: Arc<Notify>,
  pub(crate) approvals: Arc<Notify>,
  pub(crate) active: Arc<Mutex<HashMap<String, CancellationToken>>>,
  pub(crate) permits: Arc<Semaphore>,
  pub(crate) stopping: CancellationToken,
  pub(crate) submission: Arc<Mutex<()>>,
  pub(crate) draining: Arc<AtomicBool>,
  pub(crate) peer_worker: Mutex<Option<agent_distributed::worker::WorkerControl>>,
  pub(crate) relay: OnceLock<agent_distributed::relay_transport::RelayHandle>,
}
impl RuntimeServices {
  pub(crate) fn new(max_runs: usize) -> Self {
    Self {
      browser: Arc::new(Default::default()),
      computer: Arc::new(Default::default()),
      workspace_writes: Mutex::new(()),
      events: Arc::new(Notify::new()),
      approvals: Arc::new(Notify::new()),
      active: Arc::new(Mutex::new(HashMap::new())),
      permits: Arc::new(Semaphore::new(max_runs)),
      stopping: CancellationToken::new(),
      draining: Arc::new(AtomicBool::new(false)),
      peer_worker: Mutex::new(None),
      relay: OnceLock::new(),
      submission: Arc::new(Mutex::new(())),
    }
  }
}
impl std::ops::Deref for Daemon {
  type Target = RuntimeServices;
  fn deref(&self) -> &Self::Target {
    &self.services
  }
}
