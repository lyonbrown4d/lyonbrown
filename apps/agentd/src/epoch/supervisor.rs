use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use agent_local::ScopeFingerprint;
use anyhow::{Context, Result, anyhow, bail};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

use super::publisher::DescriptorPublisher;
use super::running::RunningEpoch;
use crate::model_setup::{ModelSetupCoordinator, PreparedRuntime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutoverOutcome {
  Switched,
  KeptCurrent,
}

pub struct EpochSupervisor {
  pub(super) publisher: Arc<dyn DescriptorPublisher>,
  pub(super) coordinator: ModelSetupCoordinator,
  pub(super) scope: ScopeFingerprint,
  pub(super) stop_timeout: Duration,
  pub(super) active: Option<RunningEpoch>,
  activation_ack_gate: Option<Arc<ActivationAckGate>>,
}

#[doc(hidden)]
#[derive(Default)]
pub struct ActivationAckGate {
  entered: Notify,
  release: Notify,
}

impl ActivationAckGate {
  pub async fn wait_until_entered(&self) {
    self.entered.notified().await;
  }

  pub fn release(&self) {
    self.release.notify_one();
  }

  async fn pause(&self) {
    self.entered.notify_one();
    self.release.notified().await;
  }
}

impl EpochSupervisor {
  pub fn new(
    publisher: Arc<dyn DescriptorPublisher>,
    coordinator: ModelSetupCoordinator,
    scope: ScopeFingerprint,
    stop_timeout: Duration,
  ) -> Self {
    Self {
      publisher,
      coordinator,
      scope,
      stop_timeout,
      active: None,
      activation_ack_gate: None,
    }
  }

  #[doc(hidden)]
  pub fn with_activation_ack_gate(mut self, gate: Arc<ActivationAckGate>) -> Self {
    self.activation_ack_gate = Some(gate);
    self
  }

  pub(super) async fn pause_after_activation_ack(&self) {
    if let Some(gate) = &self.activation_ack_gate {
      gate.pause().await;
    }
  }

  pub async fn start_initial(&mut self, prepared: PreparedRuntime) -> Result<()> {
    if self.active.is_some() {
      bail!("agentd epoch supervisor is already running");
    }
    let epoch = self.start_unpublished(prepared).await?;
    if let Err(error) = self.publisher.publish(&epoch.descriptor) {
      epoch.stop().await;
      return Err(error).context("failed to publish initial agentd epoch");
    }
    self.active = Some(epoch);
    Ok(())
  }

  pub async fn supervise<F>(
    &mut self,
    replacements: &mut mpsc::UnboundedReceiver<PreparedRuntime>,
    shutdown: F,
  ) -> Result<()>
  where
    F: Future<Output = ()> + Send + 'static,
  {
    let cancellation = CancellationToken::new();
    let shutdown_cancellation = cancellation.clone();
    let shutdown_task = tokio::spawn(async move {
      shutdown.await;
      shutdown_cancellation.cancel();
    });
    let result = loop {
      let event = {
        let active = self
          .active
          .as_mut()
          .ok_or_else(|| anyhow!("agentd epoch supervisor is not running"))?;
        tokio::select! {
          biased;
          _ = cancellation.cancelled() => SupervisorEvent::Shutdown,
          error = active.unexpected_exit() => SupervisorEvent::Exited(error),
          replacement = replacements.recv() => SupervisorEvent::Replacement(replacement),
        }
      };
      match event {
        SupervisorEvent::Shutdown => {
          break Ok(());
        }
        SupervisorEvent::Replacement(Some(prepared)) => {
          match self.cutover_guarded(prepared, Some(&cancellation)).await {
            Ok(super::cutover::GuardedCutover::Outcome(_)) => {}
            Ok(super::cutover::GuardedCutover::Shutdown) => {
              break Ok(());
            }
            Err(error) => break Err(error),
          }
        }
        SupervisorEvent::Replacement(None) => {
          break Err(anyhow!(
            "model setup replacement channel closed unexpectedly"
          ));
        }
        SupervisorEvent::Exited(error) => {
          break Err(error);
        }
      }
    };
    shutdown_task.abort();
    self.shutdown_with_pending(replacements).await;
    result
  }

  pub async fn shutdown(&mut self) -> Result<()> {
    self.stop_active().await;
    Ok(())
  }

  pub(super) async fn start_unpublished(&self, prepared: PreparedRuntime) -> Result<RunningEpoch> {
    RunningEpoch::start(
      prepared,
      self.coordinator.clone(),
      self.scope.clone(),
      self.stop_timeout,
    )
    .await
  }

  pub(super) async fn stop_active(&mut self) {
    if let Some(active) = self.active.take() {
      active.stop().await;
    }
  }

  async fn shutdown_with_pending(
    &mut self,
    replacements: &mut mpsc::UnboundedReceiver<PreparedRuntime>,
  ) {
    replacements.close();
    let coordinator = self.coordinator.clone();
    let active = self.active.take();
    let stop_active = async move {
      if let Some(active) = active {
        active.stop().await;
      }
    };
    tokio::join!(coordinator.shutdown(), stop_active);
    while let Some(prepared) = replacements.recv().await {
      crate::runtime_cleanup::dispose_prepared(prepared, self.stop_timeout).await;
    }
  }
}

enum SupervisorEvent {
  Shutdown,
  Replacement(Option<PreparedRuntime>),
  Exited(anyhow::Error),
}
